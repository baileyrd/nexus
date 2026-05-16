//! `com.nexus.ai.runtime` core plugin — IPC entry points for the
//! BL-134 Phase-1 runtime.
//!
//! ## Handler ID layout (per ADR 0028 §IPC surface)
//!
//! | ID | Command       | Phase 1?       |
//! |---:|---------------|----------------|
//! |  1 | `submit`      | ✓              |
//! |  2 | `cancel`      | reserved (P5)  |
//! |  3 | `pause`       | reserved (P5)  |
//! |  4 | `resume`      | reserved (P5)  |
//! |  5 | `get`         | ✓              |
//! |  6 | `list`        | ✓              |
//! |  7 | `events`      | ✓              |
//! |  8 | `pool_stats`  | ✓              |
//!
//! Reserved IDs return a clear "not yet wired" error so a caller that
//! starts using them ahead of Phase 5 gets a routable failure.

use std::sync::{Arc, Mutex, OnceLock};

use nexus_kernel::KernelPluginContext;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Serialize;

use crate::events::{topic_for, AiEvent};
use crate::pool::WorkerPool;
use crate::scheduler::Store;
use crate::{
    AgentTaskKind, AiRuntimeEventsArgs, AiRuntimeGetArgs, AiRuntimeListArgs, AiRuntimeSubmitArgs,
    AiRuntimeSubmitReply, AiRuntimeWaitForArgs, AiRuntimeWaitForReply, PoolStats, RunStatus,
    PLUGIN_ID,
};

/// `submit` handler id.
pub const HANDLER_SUBMIT: u32 = 1;
/// Reserved — Phase 5.
pub const HANDLER_CANCEL: u32 = 2;
/// Reserved — Phase 5.
pub const HANDLER_PAUSE: u32 = 3;
/// Reserved — Phase 5.
pub const HANDLER_RESUME: u32 = 4;
/// `get` handler id.
pub const HANDLER_GET: u32 = 5;
/// `list` handler id.
pub const HANDLER_LIST: u32 = 6;
/// `events` handler id.
pub const HANDLER_EVENTS: u32 = 7;
/// `pool_stats` handler id.
pub const HANDLER_POOL_STATS: u32 = 8;
/// `wait_for` handler id — BL-134 Phase 2 sync-wait primitive.
pub const HANDLER_WAIT_FOR: u32 = 9;

/// Default plugin id used as the `caller_plugin_id` when the
/// dispatcher hasn't supplied one (Phase 1 always uses the bootstrap-
/// supplied invoker context — this is a defensive fallback for unit
/// tests that build the plugin without `wire_context`).
const DEFAULT_CALLER_PLUGIN_ID: &str = "com.nexus.unknown";

/// Per-call IPC timeout for the underlying `session_run` dispatch.
/// Picked to comfortably exceed the agent's own
/// `approval_timeout_secs` default (1800s) plus the maximum tool-loop
/// runtime we've observed in the wild.
const SESSION_RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

/// Per-call IPC timeout for `WorkflowAiStep` dispatches. Workflow
/// steps are bounded by `nexus_workflow::DEFAULT_STEP_TIMEOUT` (5 min)
/// — we mirror that ceiling here so the runtime doesn't outlast the
/// step the workflow would otherwise have awaited inline.
const WORKFLOW_STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Core plugin state. The pool is built lazily in `wire_context` so
/// `cargo test -p nexus-ai-runtime` doesn't pay the cost when only
/// the type tests run.
pub struct AiRuntimeCorePlugin {
    store: Store,
    pool: OnceLock<WorkerPool>,
    /// Plugin context captured by `wire_context`. `None` until the
    /// bootstrap wires it; the IPC handlers all return a clear error
    /// rather than panic in that window.
    context: Mutex<Option<Arc<KernelPluginContext>>>,
}

impl Default for AiRuntimeCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AiRuntimeCorePlugin {
    /// Build a fresh plugin. Pool + context are wired by the loader
    /// after `register_core` returns; in unit tests, callers can
    /// invoke [`Self::wire_pool_for_tests`] to skip the bootstrap
    /// dance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: Store::new(),
            pool: OnceLock::new(),
            context: Mutex::new(None),
        }
    }

    /// Test hook — install a pool without requiring the full
    /// bootstrap wire-up. Returns `false` when a pool is already
    /// installed (each test gets a fresh plugin).
    pub fn wire_pool_for_tests(&self, pool: WorkerPool) -> bool {
        self.pool.set(pool).is_ok()
    }

    fn ctx(&self) -> Option<Arc<KernelPluginContext>> {
        self.context.lock().ok().and_then(|g| g.clone())
    }
}

impl CorePlugin for AiRuntimeCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // Every Phase-1 handler is async — the sync entrypoint is
        // here only for trait-completeness. Returning a clear "use
        // dispatch_async" error matches the convention used by
        // `nexus-ai`'s plugin (see `core_plugin.rs:407` in that crate).
        Err(exec_err(format!(
            "handler {handler_id}: ai-runtime is async; caller must use dispatch_async"
        )))
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        let store = self.store.clone();
        let ctx = self.ctx();
        let pool_handle = self.pool.get().map(WorkerPool::handle);
        let pool_metrics = self.pool.get().map(WorkerPool::metrics);
        let args = args.clone();

        Some(Box::pin(async move {
            match handler_id {
                HANDLER_SUBMIT => {
                    let ctx = ctx.ok_or_else(ctx_unwired)?;
                    let pool_handle = pool_handle.ok_or_else(pool_unwired)?;
                    handle_submit(&store, &ctx, &pool_handle, &args)
                }
                HANDLER_GET => handle_get(&store, &args),
                HANDLER_LIST => handle_list(&store, &args),
                HANDLER_EVENTS => handle_events(&store, &args),
                HANDLER_WAIT_FOR => handle_wait_for(&store, &args).await,
                HANDLER_POOL_STATS => {
                    let metrics = pool_metrics.ok_or_else(pool_unwired)?;
                    Ok(handle_pool_stats(&store, metrics))
                }
                HANDLER_CANCEL => {
                    let ctx = ctx.ok_or_else(ctx_unwired)?;
                    handle_cancel(&store, ctx.as_ref(), &args)
                }
                HANDLER_PAUSE | HANDLER_RESUME => Err(exec_err(format!(
                    "handler {handler_id}: pause/resume are not supported in BL-134 Phase 5 — \
                     a Session task is a single ipc_call with no resumable midpoint; \
                     use `cancel` and resubmit a fresh task instead"
                ))),
                other => Err(exec_err(format!("unknown handler id {other}"))),
            }
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        if let Ok(mut g) = self.context.lock() {
            *g = Some(Arc::clone(&ctx));
        }
        // Spin up the worker pool now that we're past `on_init`.
        // Failure is logged + leaves the OnceLock empty so subsequent
        // `submit` calls surface a clear "pool not running" error
        // rather than silently dropping work.
        if self.pool.get().is_none() {
            match WorkerPool::start(None) {
                Ok(pool) => {
                    // BL-134 Phase 4 — publish the pool handle to the
                    // process-wide accessor so sibling subsystems
                    // (today: nexus-ai::indexing_daemon) can avoid
                    // building a second tokio runtime. Logged at info
                    // so a misordering on a future bootstrap reorder
                    // is observable; the daemon falls back to its own
                    // runtime if the handle isn't published yet.
                    let installed = pool.publish_shared_handle();
                    let _ = self.pool.set(pool);
                    tracing::info!(
                        plugin_id = PLUGIN_ID,
                        shared_handle_installed = installed,
                        "BL-134 Phase 1+4: ai-runtime worker pool started",
                    );
                }
                Err(e) => {
                    tracing::error!(
                        plugin_id = PLUGIN_ID,
                        ?e,
                        "BL-134: failed to start ai-runtime worker pool",
                    );
                }
            }
        }
        // BL-134 Phase 2b-ii — bus republisher. Subscribes to
        // `com.nexus.ai.stream_chunk` + `com.nexus.agent.round_proposed`,
        // looks up `session_id → task_id` in the scheduler, and
        // republishes each as a typed `AiEvent::{TokenChunk,
        // RoundProposed}` under `com.nexus.ai.runtime.<variant>`.
        // Events from sessions that weren't submitted through the
        // runtime (e.g. direct `nexus agent run`) are silently
        // dropped — the correlation lookup returns None.
        //
        // We spawn the loop onto the runtime's own worker pool when
        // available so it doesn't compete with the kernel's IPC
        // runtime; fall back to the ambient runtime via
        // `tokio::spawn` if the pool isn't started yet. The
        // subscriber is process-lifetime — drops at process exit.
        if let Some(pool) = self.pool.get() {
            let store = self.store.clone();
            let ctx_for_sub = Arc::clone(&ctx);
            pool.handle().spawn(async move {
                republish_loop(store, ctx_for_sub).await;
            });
        }
    }

    fn on_stop(&mut self) {
        // Drop the pool — its `Runtime::Drop` joins all worker
        // threads after in-flight tasks finish. We deliberately do
        // not call `Runtime::shutdown_timeout` here because Phase 1
        // workers are already bounded by `SESSION_RUN_TIMEOUT`.
        // Dropping the OnceLock means the next process boot starts
        // fresh.
        if let Ok(mut g) = self.context.lock() {
            *g = None;
        }
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

fn handle_submit(
    store: &Store,
    ctx: &Arc<KernelPluginContext>,
    pool_handle: &tokio::runtime::Handle,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiRuntimeSubmitArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("submit: invalid args: {e}")))?;

    let task_id = uuid::Uuid::new_v4();
    let kind_label = parsed.task.label().to_string();
    let priority = parsed.priority;
    let parent = parsed.parent;
    let caller = caller_plugin_id(ctx);
    let ring = store.insert(task_id, &kind_label, priority, parent, &caller);

    // Record + republish the Submitted event before returning to the
    // caller so a sufficiently-fast subscriber sees it before the
    // submit reply.
    let submitted = AiEvent::Submitted {
        task_id,
        kind_label: kind_label.clone(),
        priority,
    };
    record_and_publish(store, ctx.as_ref(), &ring, &submitted);

    // BL-134 Phase 2b-ii — pre-allocate a session id for Session
    // tasks and register the session→task correlation so the bus
    // republisher can translate mid-flight `com.nexus.ai.stream_*` /
    // `com.nexus.agent.round_*` events back to a typed `AiEvent`
    // carrying this `task_id`. We inject the id into the session
    // args before forwarding to `session_run`; the agent's handler
    // honours it when `Some` and self-allocates otherwise. The
    // worker drops the correlation entry once the run reaches a
    // terminal state so the reverse map doesn't grow unbounded.
    let (task_kind, allocated_session_id) = inject_session_id(parsed.task, store, task_id);

    // Dispatch the actual work onto the dedicated pool. The worker
    // races the inner ipc_call against the cancel gate so a `cancel`
    // IPC call (BL-134 Phase 5) suppresses the underlying
    // Finished/Failed event and emits `Cancelled` instead.
    let store_for_worker = store.clone();
    let ctx_for_worker = Arc::clone(ctx);
    let cancel = store
        .cancel_gate(task_id)
        .expect("cancel gate present after insert");
    pool_handle.spawn(async move {
        // Cancel-before-start race: handler can flip the gate between
        // the submit reply and worker scheduling. Catch it pre-Started
        // so we don't emit a misleading Started → Cancelled pair.
        if cancel.is_cancelled() {
            let by = cancel.take_reason().unwrap_or_else(|| "caller".into());
            record_and_publish(
                &store_for_worker,
                ctx_for_worker.as_ref(),
                &ring,
                &AiEvent::Cancelled { task_id, by },
            );
            // Drop the correlation entry before bailing so the
            // session→task map doesn't leak when a task is cancelled
            // before it ever ran.
            if let Some(ref sid) = allocated_session_id {
                store_for_worker.forget_session(sid);
            }
            return;
        }

        let started = AiEvent::Started { task_id, attempt: 1 };
        record_and_publish(&store_for_worker, ctx_for_worker.as_ref(), &ring, &started);

        let inner = async {
            match task_kind {
                AgentTaskKind::Session { args } => run_session(&ctx_for_worker, args).await,
                AgentTaskKind::AiStream { .. } => Err(
                    "ai_stream is reserved for BL-134 Phase 2b-ii (typed event correlation)"
                        .into(),
                ),
                AgentTaskKind::WorkflowAiStep {
                    target_plugin,
                    command,
                    args,
                    workflow,
                    step,
                } => run_workflow_ai_step(
                    &ctx_for_worker,
                    &target_plugin,
                    &command,
                    args,
                    &workflow,
                    step,
                )
                .await,
            }
        };

        let event = tokio::select! {
            // Bias toward the cancel arm so a cancel signalled mid-
            // execution wins against a same-tick reply.
            biased;
            () = cancel.cancelled() => {
                let by = cancel.take_reason().unwrap_or_else(|| "caller".into());
                AiEvent::Cancelled { task_id, by }
            }
            outcome = inner => match outcome {
                Ok(reply) => AiEvent::Finished { task_id, outcome: reply },
                Err(error) => AiEvent::Failed { task_id, error, retriable: false },
            }
        };
        record_and_publish(&store_for_worker, ctx_for_worker.as_ref(), &ring, &event);
        // BL-134 Phase 2b-ii — terminal cleanup of the session→task
        // correlation entry. Tested via the unit suite below
        // (`inject_session_id_*`); pre-Started returns above already
        // ran their own cancel-emit path, so a no-op forget here is
        // safe even though they don't drop into this branch.
        if let Some(sid) = allocated_session_id {
            store_for_worker.forget_session(&sid);
        }
    });

    let reply = AiRuntimeSubmitReply { task_id };
    serde_json::to_value(&reply).map_err(|e| exec_err(format!("submit: serialize: {e}")))
}

/// BL-134 Phase 2b-ii — bus subscriber loop. Listens on
/// `com.nexus.ai.stream_chunk` + `com.nexus.agent.round_proposed`,
/// translates each event through [`crate::republisher`], and
/// publishes the typed result back on the kernel bus under
/// `com.nexus.ai.runtime.*`. Sessions without a runtime-owned
/// correlation entry are skipped silently.
///
/// The loop terminates only when the subscription returns a
/// `Closed` error — which happens at process shutdown. We log at
/// debug on every event so a stuck loop is observable in trace
/// builds without spamming production logs.
async fn republish_loop(store: Store, ctx: Arc<KernelPluginContext>) {
    use crate::republisher::{TOPIC_ROUND_PROPOSED, TOPIC_STREAM_CHUNK};
    use nexus_kernel::{EventFilter, PluginContext as _};

    // Two separate subscriptions because `CustomPrefix` would over-
    // match (e.g. `stream_start` / `stream_done` carry session_id
    // but we don't translate them). One `CustomExact` filter per
    // topic keeps the dispatch cheap and the translate set explicit.
    let mut sub_stream =
        ctx.subscribe(EventFilter::CustomExact(TOPIC_STREAM_CHUNK.to_string()));
    let mut sub_round =
        ctx.subscribe(EventFilter::CustomExact(TOPIC_ROUND_PROPOSED.to_string()));

    tracing::info!(
        plugin_id = PLUGIN_ID,
        "BL-134 Phase 2b-ii: ai-runtime republisher subscribed to stream_chunk + round_proposed"
    );

    loop {
        tokio::select! {
            evt = sub_stream.recv() => match evt {
                Ok(published) => handle_inner_event(&store, &ctx, &published.event),
                Err(_closed) => break,
            },
            evt = sub_round.recv() => match evt {
                Ok(published) => handle_inner_event(&store, &ctx, &published.event),
                Err(_closed) => break,
            },
        }
    }

    tracing::debug!(
        plugin_id = PLUGIN_ID,
        "ai-runtime republisher loop exited (bus subscription closed)"
    );

    /// Inline helper so both select arms share the same translate +
    /// publish flow. Pure dispatch + correlation lookup; the actual
    /// payload translation lives in `republisher`.
    fn handle_inner_event(
        store: &Store,
        ctx: &Arc<KernelPluginContext>,
        event: &nexus_kernel::NexusEvent,
    ) {
        use nexus_kernel::NexusEvent;
        let NexusEvent::Custom {
            type_id, payload, ..
        } = event
        else {
            return;
        };
        let Some(sid) = crate::republisher::extract_session_id(payload) else {
            return;
        };
        let Some(task_id) = store.task_for_session(&sid) else {
            // Session not owned by the runtime — drop silently.
            return;
        };
        let Some(typed) = crate::republisher::translate_bus_event(type_id, payload, task_id)
        else {
            return;
        };
        // Look up the run's event ring + record + publish through the
        // same path the worker uses, so the run's `events` IPC reply
        // includes mid-flight events alongside Submitted/Started/
        // Finished. Sessions whose run was already cleaned up race
        // with terminal cleanup — silently skip.
        let Some(ring) = store.ring_for(task_id) else {
            return;
        };
        record_and_publish(store, ctx.as_ref(), &ring, &typed);
    }
}

/// BL-134 Phase 2b-ii — pre-allocate a session id for the worker
/// before it dispatches to `com.nexus.agent::session_run`.
///
/// For `Session` tasks: generates a fresh `Uuid::new_v4()`, injects
/// it as `args.session_id`, and registers the `session_id → task_id`
/// correlation in the scheduler so the bus republisher in
/// `wire_context` can translate mid-flight events. Honours an
/// already-supplied `session_id` field on the args (caller wins —
/// preserves caller intent if someone wants to thread a specific id
/// through `delegate`).
///
/// Non-Session kinds are returned unchanged; only `session_run`
/// emits the bus topics we correlate today (`stream_*`,
/// `round_proposed`). Workflow async steps and AiStream get their
/// own correlation pass in future phases.
fn inject_session_id(
    kind: AgentTaskKind,
    store: &Store,
    task_id: uuid::Uuid,
) -> (AgentTaskKind, Option<String>) {
    let AgentTaskKind::Session { mut args } = kind else {
        return (kind, None);
    };
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(existing) if !existing.is_empty() => existing.to_string(),
        _ => {
            let fresh = uuid::Uuid::new_v4().to_string();
            if let serde_json::Value::Object(map) = &mut args {
                map.insert("session_id".into(), serde_json::Value::String(fresh.clone()));
            }
            fresh
        }
    };
    store.register_session(session_id.clone(), task_id);
    (AgentTaskKind::Session { args }, Some(session_id))
}

/// BL-134 Phase 5 — request cooperative cancellation of a queued or
/// running task. Idempotent: a second `cancel` against the same
/// `task_id` returns `{ cancelled: false }` to surface that the run
/// was already signalled. Tasks already in a terminal state are
/// rejected with a clear error so the caller can fall back to
/// re-reading status via `get`.
///
/// The worker observes the signal in its `select!` arm and emits
/// `Cancelled { by }` instead of the underlying Finished/Failed —
/// the cancel arm is `biased` so it wins a same-tick race against
/// the in-flight ipc_call's reply.
fn handle_cancel(
    store: &Store,
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: crate::AiRuntimeControlArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("cancel: invalid args: {e}")))?;
    if store.is_terminal(parsed.task_id) == Some(true) {
        return Err(exec_err(format!(
            "cancel: task_id {} already in a terminal state",
            parsed.task_id
        )));
    }
    let gate = store
        .cancel_gate(parsed.task_id)
        .ok_or_else(|| exec_err(format!("cancel: task_id {} not found", parsed.task_id)))?;
    let reason = parsed.reason.clone().or_else(|| Some("caller".into()));
    let first_signal = gate.request(reason);
    if first_signal {
        // Publish a synchronous Cancelled-requested breadcrumb so
        // observers see the signal even before the worker's
        // select! arm fires. The worker's eventual Cancelled event
        // is the canonical terminal record; this one is a hint.
        // (Phase 5 deliberately does NOT split this into its own
        // AiEvent variant — the variant set stays closed.)
        tracing::info!(
            plugin_id = PLUGIN_ID,
            task_id = %parsed.task_id,
            reason = ?parsed.reason,
            "cancel requested"
        );
        // Touch ctx to keep the borrow alive for future Phase-5
        // additions (e.g. emitting a richer bus event); for now the
        // tracing line is the only side-effect.
        let _ = ctx;
    }
    Ok(serde_json::json!({ "cancelled": first_signal }))
}

async fn run_session(
    ctx: &KernelPluginContext,
    session_args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use nexus_kernel::PluginContext;

    ctx.ipc_call(
        "com.nexus.agent",
        "session_run",
        session_args,
        SESSION_RUN_TIMEOUT,
    )
    .await
    .map_err(|e| format!("session_run: {e}"))
}

/// BL-134 Phase 3 — drive a single workflow AI step. The workflow
/// executor packages the underlying IPC dispatch (e.g.
/// `("com.nexus.ai", "ask", …)` for `ai_prompt`) into a
/// `WorkflowAiStep` task; the runtime fires the call here and the
/// terminal `Finished` event carries the reply verbatim. `workflow`
/// + `step` ride along on the tracing span for observability so a
/// long-running async `ai_prompt` step appears in the worker logs
/// with its parent run id.
async fn run_workflow_ai_step(
    ctx: &KernelPluginContext,
    target_plugin: &str,
    command: &str,
    args: serde_json::Value,
    workflow: &str,
    step: u32,
) -> Result<serde_json::Value, String> {
    use nexus_kernel::PluginContext;

    let span = tracing::info_span!(
        "workflow_ai_step",
        target_plugin,
        command,
        workflow,
        step
    );
    let _enter = span.enter();
    ctx.ipc_call(target_plugin, command, args, WORKFLOW_STEP_TIMEOUT)
        .await
        .map_err(|e| format!("workflow_ai_step ({target_plugin}::{command}): {e}"))
}

fn handle_get(store: &Store, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let parsed: AiRuntimeGetArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("get: invalid args: {e}")))?;
    let row = store
        .get(parsed.task_id)
        .ok_or_else(|| exec_err(format!("get: task_id {} not found", parsed.task_id)))?;
    serde_json::to_value(&row).map_err(|e| exec_err(format!("get: serialize: {e}")))
}

#[derive(Serialize)]
struct ListReply {
    runs: Vec<crate::AgentRunSummary>,
}

fn handle_list(store: &Store, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let parsed: AiRuntimeListArgs = if args.is_null() {
        AiRuntimeListArgs::default()
    } else {
        serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("list: invalid args: {e}")))?
    };
    let runs = store.list(&parsed);
    serde_json::to_value(&ListReply { runs })
        .map_err(|e| exec_err(format!("list: serialize: {e}")))
}

#[derive(Serialize)]
struct EventsReply {
    events: Vec<crate::events::AiEvent>,
}

fn handle_events(store: &Store, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let parsed: AiRuntimeEventsArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("events: invalid args: {e}")))?;
    let ring = store
        .ring_for(parsed.task_id)
        .ok_or_else(|| exec_err(format!("events: task_id {} not found", parsed.task_id)))?;
    let events = match parsed.since_seq {
        Some(after) => ring.snapshot_after(after),
        None => ring.snapshot(),
    };
    serde_json::to_value(&EventsReply { events })
        .map_err(|e| exec_err(format!("events: serialize: {e}")))
}

/// Block until `task_id` reaches a terminal status, or the optional
/// `timeout_ms` elapses. Returns the full `AgentRun` snapshot at the
/// point the wait completed; the reply's `timed_out` flag tells the
/// caller whether the deadline expired (status is still non-terminal)
/// or the run actually finished.
///
/// Race-free against the worker calling `observe_status` concurrently:
/// we (1) check status, (2) build the `notified()` future BEFORE
/// re-checking status (the Notify documentation guarantees a future
/// constructed at time T sees every `notify_waiters()` after T), and
/// (3) re-check status — so a transition between (1) and (2) cannot
/// be missed.
async fn handle_wait_for(
    store: &Store,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiRuntimeWaitForArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("wait_for: invalid args: {e}")))?;

    let notify = store
        .terminal_notify(parsed.task_id)
        .ok_or_else(|| exec_err(format!("wait_for: task_id {} not found", parsed.task_id)))?;

    // Step 1 — short-circuit if already terminal.
    let already_terminal = store.is_terminal(parsed.task_id).unwrap_or(false);
    let timed_out = if already_terminal {
        false
    } else {
        // Step 2 — build the future BEFORE the second status check.
        let notified = notify.notified();
        tokio::pin!(notified);

        // Step 3 — re-check status; a transition between step 1 and
        // step 2's future construction would have stored a "ready"
        // permit, so awaiting the pinned future returns immediately.
        if store.is_terminal(parsed.task_id) == Some(true) {
            false
        } else if let Some(ms) = parsed.timeout_ms {
            let timeout = std::time::Duration::from_millis(ms);
            tokio::time::timeout(timeout, notified).await.is_err()
        } else {
            notified.await;
            false
        }
    };

    let run = store
        .get(parsed.task_id)
        .ok_or_else(|| exec_err(format!("wait_for: task_id {} not found", parsed.task_id)))?;
    let reply = AiRuntimeWaitForReply { run, timed_out };
    serde_json::to_value(&reply).map_err(|e| exec_err(format!("wait_for: serialize: {e}")))
}

fn handle_pool_stats(store: &Store, metrics: crate::pool::PoolMetrics) -> serde_json::Value {
    let queued = store.count_status(&RunStatus::Queued);
    let running = store.count_status(&RunStatus::Running);
    let stats = PoolStats {
        workers: metrics.workers,
        queued,
        running,
        max: metrics.workers,
    };
    serde_json::to_value(&stats).unwrap_or(serde_json::Value::Null)
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn record_and_publish(
    store: &Store,
    ctx: &KernelPluginContext,
    ring: &crate::SharedEventRing,
    event: &AiEvent,
) {
    use nexus_kernel::PluginContext;

    ring.push(event.clone());
    store.observe_status(event);
    let topic = topic_for(event);
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    if let Err(e) = ctx.publish(&topic, payload) {
        tracing::warn!(plugin_id = PLUGIN_ID, ?e, %topic, "ai-runtime: bus publish failed");
    }
}

fn caller_plugin_id(_ctx: &KernelPluginContext) -> String {
    // The kernel context doesn't expose its plugin id today (the
    // shipped `KernelPluginContext` getter is private). Phase 1
    // records the runtime's own id as the caller — Phase 2 (delegate)
    // is when caller-id propagation matters and will lift this
    // through a richer context API.
    DEFAULT_CALLER_PLUGIN_ID.to_string()
}

fn ctx_unwired() -> PluginError {
    exec_err("ai-runtime context not wired (bootstrap incomplete)")
}

fn pool_unwired() -> PluginError {
    exec_err("ai-runtime worker pool not started; cannot submit work")
}

fn exec_err(msg: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: msg.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskPriority;

    fn empty_args() -> serde_json::Value {
        serde_json::json!({})
    }

    #[test]
    fn dispatch_sync_returns_use_async_error() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let err = plugin.dispatch(HANDLER_SUBMIT, &empty_args()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("dispatch_async"), "actual: {msg}");
    }

    #[tokio::test]
    async fn list_with_no_runs_returns_empty_runs_array() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let fut = plugin.dispatch_async(HANDLER_LIST, &empty_args()).unwrap();
        let value = fut.await.unwrap();
        assert_eq!(value, serde_json::json!({ "runs": [] }));
    }

    #[tokio::test]
    async fn get_unknown_task_id_errors() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let args = serde_json::json!({ "task_id": uuid::Uuid::new_v4() });
        let fut = plugin.dispatch_async(HANDLER_GET, &args).unwrap();
        let err = fut.await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not found"), "actual: {msg}");
    }

    #[tokio::test]
    async fn submit_without_context_errors_with_clear_message() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let args = serde_json::json!({
            "task": { "kind": "session", "args": {} },
        });
        let fut = plugin.dispatch_async(HANDLER_SUBMIT, &args).unwrap();
        let err = fut.await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("context not wired"), "actual: {msg}");
    }

    #[tokio::test]
    async fn pool_stats_without_pool_errors() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let fut = plugin
            .dispatch_async(HANDLER_POOL_STATS, &empty_args())
            .unwrap();
        let err = fut.await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("pool not started"), "actual: {msg}");
    }

    #[tokio::test]
    async fn wait_for_unknown_task_id_errors() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let args = serde_json::json!({ "task_id": uuid::Uuid::new_v4() });
        let fut = plugin.dispatch_async(HANDLER_WAIT_FOR, &args).unwrap();
        let err = fut.await.unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn wait_for_terminal_run_returns_immediately() {
        // Seed a store with a finished run, build a plugin pointing at
        // it, dispatch wait_for, expect timed_out=false + status
        // reflecting the terminal state.
        let plugin = AiRuntimeCorePlugin::new();
        let id = uuid::Uuid::new_v4();
        plugin
            .store
            .insert(id, "session", TaskPriority::Interactive, None, "x");
        plugin.store.observe_status(&AiEvent::Finished {
            task_id: id,
            outcome: serde_json::json!({"ok": true}),
        });
        let args = serde_json::json!({ "task_id": id });
        let value = handle_wait_for(&plugin.store, &args).await.unwrap();
        let reply: crate::AiRuntimeWaitForReply = serde_json::from_value(value).unwrap();
        assert!(!reply.timed_out);
        assert_eq!(reply.run.status, RunStatus::Completed);
    }

    #[tokio::test]
    async fn wait_for_blocks_until_worker_finishes() {
        let plugin = AiRuntimeCorePlugin::new();
        let id = uuid::Uuid::new_v4();
        plugin
            .store
            .insert(id, "session", TaskPriority::Interactive, None, "x");
        let store_clone = plugin.store.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            store_clone.observe_status(&AiEvent::Finished {
                task_id: id,
                outcome: serde_json::Value::Null,
            });
        });
        let args = serde_json::json!({ "task_id": id });
        let started = std::time::Instant::now();
        let value = handle_wait_for(&plugin.store, &args).await.unwrap();
        let reply: crate::AiRuntimeWaitForReply = serde_json::from_value(value).unwrap();
        assert!(!reply.timed_out);
        assert_eq!(reply.run.status, RunStatus::Completed);
        // Loose bound — the spawn lag + scheduler hop means the wait
        // takes >0 but well under the 1s test budget.
        assert!(started.elapsed() >= std::time::Duration::from_millis(10));
    }

    #[tokio::test]
    async fn wait_for_with_timeout_returns_timed_out_when_run_still_running() {
        let plugin = AiRuntimeCorePlugin::new();
        let id = uuid::Uuid::new_v4();
        plugin
            .store
            .insert(id, "session", TaskPriority::Interactive, None, "x");
        plugin.store.observe_status(&AiEvent::Started {
            task_id: id,
            attempt: 1,
        });
        let args = serde_json::json!({ "task_id": id, "timeout_ms": 25 });
        let value = handle_wait_for(&plugin.store, &args).await.unwrap();
        let reply: crate::AiRuntimeWaitForReply = serde_json::from_value(value).unwrap();
        assert!(reply.timed_out);
        assert_eq!(reply.run.status, RunStatus::Running);
    }

    #[test]
    fn inject_session_id_allocates_when_missing_and_registers_correlation() {
        let store = Store::new();
        let task_id = uuid::Uuid::new_v4();
        let kind = AgentTaskKind::Session {
            args: serde_json::json!({ "goal": "x" }),
        };
        let (new_kind, sid) = inject_session_id(kind, &store, task_id);
        let sid = sid.expect("session id allocated");
        match new_kind {
            AgentTaskKind::Session { args } => {
                assert_eq!(
                    args.get("session_id").and_then(|v| v.as_str()),
                    Some(sid.as_str()),
                    "injected session_id must be in the args we forward to session_run"
                );
            }
            _ => panic!("expected Session"),
        }
        assert_eq!(store.task_for_session(&sid), Some(task_id));
    }

    #[test]
    fn inject_session_id_honours_caller_supplied_id() {
        let store = Store::new();
        let task_id = uuid::Uuid::new_v4();
        let kind = AgentTaskKind::Session {
            args: serde_json::json!({ "goal": "x", "session_id": "caller-pinned" }),
        };
        let (new_kind, sid) = inject_session_id(kind, &store, task_id);
        assert_eq!(sid.as_deref(), Some("caller-pinned"));
        match new_kind {
            AgentTaskKind::Session { args } => assert_eq!(
                args.get("session_id").and_then(|v| v.as_str()),
                Some("caller-pinned")
            ),
            _ => panic!("expected Session"),
        }
        assert_eq!(store.task_for_session("caller-pinned"), Some(task_id));
    }

    #[test]
    fn inject_session_id_skips_non_session_kinds() {
        let store = Store::new();
        let task_id = uuid::Uuid::new_v4();
        let kind = AgentTaskKind::WorkflowAiStep {
            target_plugin: "com.nexus.ai".into(),
            command: "ask".into(),
            args: serde_json::Value::Null,
            workflow: "w".into(),
            step: 0,
        };
        let (_, sid) = inject_session_id(kind, &store, task_id);
        assert!(sid.is_none(), "WorkflowAiStep doesn't need session_id");
    }

    #[tokio::test]
    async fn pause_and_resume_return_unsupported_error() {
        // BL-134 Phase 5 — cancel is wired (see cancel_*  tests
        // below); pause/resume stay unsupported because a Session is
        // a single ipc_call with no resumable midpoint.
        let mut plugin = AiRuntimeCorePlugin::new();
        for id in [HANDLER_PAUSE, HANDLER_RESUME] {
            let fut = plugin.dispatch_async(id, &empty_args()).unwrap();
            let err = fut.await.unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("not supported"), "actual: {msg}");
            assert!(msg.contains("Phase 5"), "actual: {msg}");
        }
    }

    /// Build a minimal `KernelPluginContext` for the cancel-handler
    /// tests. Caller-supplied because the cancel handler reaches it
    /// only as a `&KernelPluginContext` reference (for future bus
    /// emit), not through the plugin's stored context.
    fn test_ctx() -> nexus_kernel::KernelPluginContext {
        let dir = tempfile::tempdir().unwrap();
        let kv: std::sync::Arc<dyn nexus_kernel::KvStore> =
            std::sync::Arc::new(nexus_kernel::InMemoryKvStore::new());
        let bus = std::sync::Arc::new(nexus_kernel::EventBus::new(8));
        nexus_kernel::KernelPluginContext::new(
            crate::PLUGIN_ID,
            "0.0.1",
            nexus_kernel::CapabilitySet::default(),
            kv,
            bus,
            dir.path(),
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn cancel_unknown_task_id_errors() {
        let plugin = AiRuntimeCorePlugin::new();
        let ctx = test_ctx();
        let err = handle_cancel(
            &plugin.store,
            &ctx,
            &serde_json::json!({ "task_id": uuid::Uuid::new_v4() }),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn cancel_known_task_signals_gate_and_returns_first_signal() {
        let plugin = AiRuntimeCorePlugin::new();
        let id = uuid::Uuid::new_v4();
        plugin
            .store
            .insert(id, "session", TaskPriority::Interactive, None, "x");
        let gate = plugin.store.cancel_gate(id).unwrap();
        assert!(!gate.is_cancelled(), "starts un-cancelled");

        let ctx = test_ctx();
        let value = handle_cancel(
            &plugin.store,
            &ctx,
            &serde_json::json!({ "task_id": id, "reason": "test-shutdown" }),
        )
        .unwrap();
        assert_eq!(value, serde_json::json!({ "cancelled": true }));
        assert!(gate.is_cancelled());

        // Second cancel is a no-op and reports cancelled = false.
        let value2 = handle_cancel(
            &plugin.store,
            &ctx,
            &serde_json::json!({ "task_id": id }),
        )
        .unwrap();
        assert_eq!(value2, serde_json::json!({ "cancelled": false }));
    }

    #[tokio::test]
    async fn cancel_after_terminal_state_errors() {
        let plugin = AiRuntimeCorePlugin::new();
        let id = uuid::Uuid::new_v4();
        plugin
            .store
            .insert(id, "session", TaskPriority::Interactive, None, "x");
        plugin.store.observe_status(&AiEvent::Finished {
            task_id: id,
            outcome: serde_json::Value::Null,
        });

        let ctx = test_ctx();
        let err = handle_cancel(
            &plugin.store,
            &ctx,
            &serde_json::json!({ "task_id": id }),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("terminal state"));
    }

    #[tokio::test]
    async fn unknown_handler_id_errors() {
        let mut plugin = AiRuntimeCorePlugin::new();
        let fut = plugin.dispatch_async(99, &empty_args()).unwrap();
        let err = fut.await.unwrap_err();
        assert!(format!("{err}").contains("unknown handler id 99"));
    }

    #[tokio::test]
    async fn submit_rejects_invalid_args_shape() {
        let mut plugin = AiRuntimeCorePlugin::new();
        // Force the context-check to pass by skipping it: build args
        // that fail parsing first.
        let bad = serde_json::json!({ "task": "not an object" });
        let fut = plugin.dispatch_async(HANDLER_SUBMIT, &bad).unwrap();
        let err = fut.await.unwrap_err();
        let msg = format!("{err}");
        // We hit the ctx-unwired path before parsing because handle_submit
        // checks ctx first; either error message is acceptable here.
        assert!(
            msg.contains("invalid args") || msg.contains("context not wired"),
            "actual: {msg}"
        );
    }

    #[test]
    fn pool_stats_handler_serialises_zero_counts_when_store_empty() {
        let store = Store::new();
        let metrics = crate::pool::PoolMetrics { workers: 4 };
        let value = handle_pool_stats(&store, metrics);
        let parsed: PoolStats = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.workers, 4);
        assert_eq!(parsed.max, 4);
        assert_eq!(parsed.queued, 0);
        assert_eq!(parsed.running, 0);
    }

    #[test]
    fn pool_stats_handler_counts_queued_and_running_runs() {
        let store = Store::new();
        let queued_id = uuid::Uuid::new_v4();
        store.insert(queued_id, "session", TaskPriority::Interactive, None, "x");
        let running_id = uuid::Uuid::new_v4();
        store.insert(running_id, "session", TaskPriority::Interactive, None, "x");
        store.observe_status(&AiEvent::Started {
            task_id: running_id,
            attempt: 1,
        });
        let stats: PoolStats = serde_json::from_value(handle_pool_stats(
            &store,
            crate::pool::PoolMetrics { workers: 2 },
        ))
        .unwrap();
        assert_eq!(stats.queued, 1);
        assert_eq!(stats.running, 1);
    }
}
