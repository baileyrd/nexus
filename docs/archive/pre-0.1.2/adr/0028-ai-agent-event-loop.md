# ADR 0028: AI/Agent Event Loop — Unified Orchestration, Event Channel, and Worker Pool

**Date:** 2026-05-14 (proposed); Phase 1 landed 2026-05-15.
**Status:** Accepted (Phase 1 implemented; Phases 2–6 deferred to follow-up PRs).
**Related:** [ADR 0002](0002-hierarchical-capability-strings.md) (capabilities), [ADR 0004](0004-crate-boundaries-and-ownership.md) (crate boundaries), [ADR 0005](0005-single-dispatch-handler-ids.md) (handler IDs), [ADR 0011](0011-adopt-plugin-first-shell.md) (plugin-first shell), [ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) (native vs WASM), [ADR 0020](0020-popout-window-architecture.md) (popout windows), [ADR 0022](0022-per-handler-ai-capabilities.md) (per-handler AI caps), [ADR 0023](0023-unify-agent-on-ai-tool-registry.md) (agent on AI registry), [ADR 0024](0024-agent-session-tool-loop.md) (session tool loop).

## Status note (2026-05-15)

Phase 1 shipped on the `bl-134-ai-runtime` branch: new `crates/nexus-ai-runtime/` core plugin (`com.nexus.ai.runtime`, registered after `com.nexus.notifications` in bootstrap), 5 of 8 IPC handlers wired (`submit`, `get`, `list`, `events`, `pool_stats`), 3 reserved-for-Phase-5 handlers returning a typed "Phase 5" error (`cancel` / `pause` / `resume`), three new capabilities (`ai.runtime.submit` / `.control` / `.observe`) added to `nexus-plugin-api::Capability`, full per-handler cap matrix, ts-rs + schemars bindings emitted, dep-invariants row preventing CLI/TUI/MCP from direct-linking the new crate, 24 unit tests + 5 bootstrap-driven IPC tests. Phase-1 deviations from the original ADR text: (a) `AiEvent` correlation for `com.nexus.ai.stream_*` and `com.nexus.agent.round_*` topics is **deferred to Phase 2** since it requires a session-id mapping from underlying calls back to runtime task IDs (the event types still ship; only the bus republisher's tap is unwired). (b) Run-history persistence under `<forge>/.forge/ai-runtime/runs.db` is deferred — Phase 1's store is in-memory only. (c) Caller-id propagation through the runtime is recorded as `com.nexus.unknown` in Phase 1; the kernel context's plugin-id getter is private. Phase 2 lifts both as part of `delegate`'s migration.

## Context

Three independent gaps in the current AI/agent stack point at the same missing primitive.

### (a) Agent orchestration runtime

`com.nexus.agent::session_run` (ADR 0024 Phase 2b) is the canonical entry point for an LLM tool-loop. Today the handler awaits the whole session inline — capped at `DEFAULT_MAX_ITERATIONS = 32` rounds, default `approval_timeout_secs = 1800` — and only returns when the session terminates (`Complete` / `Aborted` / `MaxRounds` / `ApprovalTimeout`). There is no first-class concept of a *task*: a long-running unit of agent work that can be enqueued, observed mid-flight, paused, cancelled, retried, or composed with other tasks. Side-effects of this gap:

- The shell can only "run an agent" by holding open one IPC call per active agent; if the popout closes (ADR 0020) the session is orphaned and surfaces only as a transcript on disk.
- `delegate` (`HANDLER_DELEGATE = 24`) re-enters `handle_session_run` synchronously — there is no parallel composition primitive; the comment at `core_plugin.rs:2241` says "callers should fan out / chain `session_run` directly", which means the shell, not the agent, is responsible for concurrency.
- Cancellation works only by dropping the IPC future (which races with `BusBridgePolicy`'s pending-approvals map at `core_plugin.rs:196`).
- Retry policy lives entirely in `nexus-workflow`'s step retry (`Step.max_retries` / `retry_backoff` in `crates/nexus-workflow/src/lib.rs:185`), not in the agent. Re-running a failed agent step means re-issuing `session_run` and re-paying the tool-loop preamble.

### (c) Cross-subsystem AI event channel

Streaming AI events already exist as `Custom` topics on the kernel bus:

- `com.nexus.ai.stream_start` / `stream_chunk` / `stream_done` (`crates/nexus-ai/src/core_plugin.rs:1385`–`1430`)
- `com.nexus.agent.round_proposed` (`crates/nexus-agent/src/core_plugin.rs:1823`)
- `com.nexus.notifications.delivered` (`crates/nexus-notifications/src/lib.rs:74`)
- `ai_activity.appended` / `activity.appended` (`crates/nexus-types/src/activity.rs`)

These topics work but are typed only as `serde_json::Value` payloads (see `NexusEvent::Custom` in `crates/nexus-plugin-api/src/event.rs:70`). Consumers — the chat panel, the agent panel, the notifications transport, the activity log, the auto-notify subscriber for threshold-crossing runs (BL-133 follow-up) — each re-derive the payload shape from a JSON schema in their own crate or TypeScript module. The closed `NexusEvent` enum was the right shape for kernel lifecycle events; the AI/agent surface is the place where the lack of a typed cross-subsystem channel hurts most because the same payload is consumed in four places.

### (d) Background worker pool

LLM calls are externally-paced (provider RTT measured in seconds; tool-use rounds compound this). Today they share the tokio runtime that drives every other IPC dispatch:

- `KernelPluginContext::ipc_call` either polls a `CorePluginFuture` directly (sync handlers go through `tokio::task::spawn_blocking` per call at `context_impl.rs:166`) or awaits an async handler in-place. Heavy AI work runs on whatever runtime the invoker built — for CLI / TUI this is fine, but for the Tauri shell the same runtime serves UI invokes.
- `nexus-workflow`'s cron scheduler spawns its own tasks on the current `tokio::runtime::Handle::try_current()` (`crates/nexus-workflow/src/core_plugin.rs:359`).
- `nexus-ai::indexing_daemon` (`indexing_daemon.rs:32`) builds its own `tokio::runtime::Builder::new_current_thread()` on a dedicated OS thread for the embedding-backed indexing loop. This is the only piece of evidence that the system already knows AI work needs its own executor — but the pattern is one-off, only for indexing.

A single AI session with eight rounds, each running a 30 s tool call, occupies a tokio task for ~4 minutes. With four concurrent agent panels (Hermes Feature 7 / multi-archetype workflows / `delegate` fan-out) the shell's tokio runtime starts head-of-line blocking IPC dispatches against storage and the editor.

### Why the three are one problem

A "task" needs a queue (a). A queue needs an observation channel (c). An observation channel needs producers that don't starve the kernel runtime (d). Designing them separately produces three nearly-identical event surfaces (one per concern), three queues (worker pool, agent session map, workflow step queue), and three ways for the shell to find out a run is done. The current `BusBridgePolicy` pending-approvals map is the prototype of the queue; the AI stream topics are the prototype of the channel; the indexing daemon's dedicated runtime is the prototype of the pool. The decision below is to **promote those three prototypes into one named subsystem** instead of letting each grow independently.

## Decision

**Proposed.** Introduce one new core service plugin — **`nexus-ai-runtime`** (plugin id **`com.nexus.ai.runtime`**) — that owns the agent task scheduler, the typed AI event channel, and a dedicated tokio worker pool. The existing `nexus-agent`, `nexus-ai`, and `nexus-workflow` crates keep their handlers and library APIs; they delegate long-running AI work to the runtime via IPC.

### Crate layout

```
crates/nexus-ai-runtime/
├── Cargo.toml          # depends on: nexus-kernel, nexus-plugin-api, nexus-types
├── src/
│   ├── lib.rs          # AgentTask, AgentRun, AiEvent, RunStatus
│   ├── core_plugin.rs  # AiRuntimeCorePlugin — IPC handlers below
│   ├── scheduler.rs    # task queue, retry policy, cancel/pause, parallel groups
│   ├── pool.rs         # dedicated multi-thread tokio Runtime + bounded JoinSet
│   ├── events.rs       # typed AiEvent envelope + bus republisher
│   └── store.rs        # run-history persistence (sqlite — reusing nexus-kv shape)
```

The crate depends only on `nexus-kernel`, `nexus-plugin-api`, and `nexus-types`. It does **not** depend on `nexus-agent`, `nexus-ai`, or `nexus-workflow` — those continue to call it through `ipc_call`. This keeps the existing crate-boundary DAG (ADR 0004) intact: `nexus-bootstrap` is still the only crate that wires the runtime to its consumers.

### Proposed key types (all new, marked **proposed**)

```rust
// crates/nexus-ai-runtime/src/lib.rs

/// A unit of agent work the runtime schedules. Constructed by callers
/// (shell, CLI, workflow plugin), enqueued via `submit`, executed by
/// the worker pool, observed via `AiEvent` on the kernel bus.
pub struct AgentTask {
    pub task_id: uuid::Uuid,
    pub kind: AgentTaskKind,
    pub priority: TaskPriority,        // Background | Interactive | Critical
    pub policy: RetryPolicy,           // mirrors workflow Step retry shape
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
    pub parent: Option<uuid::Uuid>,    // for delegate / parallel composition
    pub caller_plugin_id: String,
    pub caller_caps: CapabilitySet,    // *snapshot* of caller caps at submit
}

pub enum AgentTaskKind {
    /// Drive a `com.nexus.agent::session_run` to completion.
    Session(SessionRunArgs),
    /// Drive a single `com.nexus.ai::stream_chat` turn.
    AiStream(StreamChatArgs),
    /// Run a workflow `notify` / `ai_prompt` / `ai_decision` step
    /// out-of-band. Workflow's own executor keeps the per-step
    /// retry loop; this kind exists so long AI steps don't block
    /// the workflow's sync dispatch path.
    WorkflowAiStep { workflow: String, step: usize, args: serde_json::Value },
}

/// Live state of a submitted task. Stored in the scheduler's in-memory
/// map and persisted to `<forge>/.forge/ai-runtime/runs.db`.
pub struct AgentRun {
    pub task_id: uuid::Uuid,
    pub status: RunStatus,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub attempts: u32,
    pub events: Vec<AiEvent>,          // bounded ring; full history in store
}

pub enum RunStatus {
    Queued, Running, Paused, Cancelled,
    Completed, Failed(String), Aborted(String), TimedOut,
}

/// Typed cross-subsystem AI/agent lifecycle envelope. Republished on
/// the kernel bus under `com.nexus.ai.runtime.*` topics; consumers
/// import `AiEvent` from `nexus-ai-runtime` (or its TS-rs generated
/// equivalent) instead of re-parsing JSON.
pub enum AiEvent {
    Submitted { task_id: Uuid, kind_label: &'static str, priority: TaskPriority },
    Started   { task_id: Uuid, attempt: u32 },
    TokenChunk{ task_id: Uuid, text: String },                 // wraps stream_chunk
    ToolCalled{ task_id: Uuid, tool_use_id: String, name: String, args_preview: String },
    ToolResult{ task_id: Uuid, tool_use_id: String, is_error: bool, summary: String },
    RoundProposed{ task_id: Uuid, round: u32, narration: String },   // wraps agent.round_proposed
    RoundDecided { task_id: Uuid, round: u32, decision_kind: &'static str },
    Paused   { task_id: Uuid, reason: String },
    Resumed  { task_id: Uuid },
    Cancelled{ task_id: Uuid, by: String },
    Finished { task_id: Uuid, outcome: SessionOutcome },
    Failed   { task_id: Uuid, error: String, retriable: bool },
}
```

### IPC surface (handler IDs are proposals; numbering follows ADR 0005)

| ID | Command | Args | Reply |
|---:|---|---|---|
| 1 | `submit` | `AgentTask` (JSON) | `{ task_id }` |
| 2 | `cancel` | `{ task_id, reason? }` | `{ cancelled: bool }` |
| 3 | `pause` | `{ task_id }` | `{ paused: bool }` |
| 4 | `resume` | `{ task_id }` | `{ resumed: bool }` |
| 5 | `get` | `{ task_id }` | `AgentRun` |
| 6 | `list` | `{ status?, limit?, since? }` | `[AgentRunSummary]` |
| 7 | `events` | `{ task_id, since_seq? }` | `[AiEvent]` (replay; live stream stays on bus) |
| 8 | `pool_stats` | `{}` | `{ workers, queued, running, max }` |

### Capability gates (proposed)

New capabilities, slotted under the existing `ai.*` namespace per ADR 0002 / 0022:

- **`ai.runtime.submit`** (Medium) — gate on `submit`. Lets a caller enqueue an agent task. Granted to `com.nexus.cli`, `com.nexus.tui`, `com.nexus.shell` (the invokers), to `com.nexus.workflow` (so the `notify` / `ai_prompt` step can punt to the runtime), and to `com.nexus.agent` itself (for `delegate`-shaped composition).
- **`ai.runtime.control`** (Medium) — gate on `cancel` / `pause` / `resume`. Separate from `submit` so a panel that displays runs but shouldn't control them can be wired with the smaller grant.
- **`ai.runtime.observe`** (Low) — gate on `get` / `list` / `events` / `pool_stats`. The shell's observability panel needs this; community plugins should not get it by default.

`ai.chat` / `ai.tools.write` / `ai.tools.mcp` (ADR 0022) continue to gate the **underlying** `stream_chat` / `session_run` / `propose_tool_calls` calls the runtime issues on behalf of the caller. Crucially, the runtime impersonates the caller's capability set when it issues those IPC calls — `AgentTask.caller_caps` is the snapshot. A task submitted by a workflow that lacks `ai.tools.write` cannot escalate by routing through the runtime.

### Worker pool

`pool.rs` constructs a dedicated `tokio::runtime::Builder::new_multi_thread()` Runtime with:

- `worker_threads = max(2, num_cpus() / 2)` (configurable via `<forge>/.forge/config.toml [ai_runtime].workers`)
- `thread_name = "nexus-ai-worker-{n}"`
- A bounded `tokio::task::JoinSet` per priority bucket so `Critical` tasks cannot be starved by a burst of `Background` indexing work.

The pool is owned by the core plugin; lifecycle hooks shut it down in `on_stop`. This is structurally identical to `nexus-ai::indexing_daemon::IndexingDaemon`'s dedicated runtime (`crates/nexus-ai/src/indexing_daemon.rs:32`) — that daemon migrates to the runtime's pool in a follow-up.

The runtime is **the only consumer that constructs a dedicated tokio Runtime**. `nexus-workflow`'s `tokio::runtime::Handle::try_current()` pattern stays as-is; cron-driven workflow steps submit `AgentTask`s instead of awaiting LLM calls inline.

### Event flow

```
                       Caller (shell / cli / workflow plugin / agent)
                              |
                              v
              ipc_call("com.nexus.ai.runtime", "submit", AgentTask)
                              |
                              v
                  +-----------+-----------+
                  | nexus-ai-runtime      |   AiEvent::Submitted -> bus
                  |  (core plugin)        |       (com.nexus.ai.runtime.submitted)
                  |                       |
                  |  scheduler -> pool    |
                  |     |                 |
                  |     +-- worker task --+--> ipc_call("com.nexus.agent","session_run",..)
                  |                       |        (uses AgentTask.caller_caps)
                  |                       |
                  |     <- subscribes to  |        nexus-agent publishes:
                  |        com.nexus.ai.* |        com.nexus.ai.stream_*
                  |        com.nexus.agent|        com.nexus.agent.round_proposed
                  |          .*           |
                  |                       |
                  |  wraps each into      |
                  |  typed AiEvent and    |
                  |  republishes under    |   com.nexus.ai.runtime.token_chunk
                  |  com.nexus.ai.runtime |   com.nexus.ai.runtime.tool_called
                  |  .* topics            |   com.nexus.ai.runtime.finished
                  +-----------------------+
                              |
                              v
                Editor / chat / notifications / shell / observability
                (one EventFilter::CustomPrefix("com.nexus.ai.runtime."))
```

Existing `com.nexus.ai.stream_*` and `com.nexus.agent.round_proposed` topics **stay**. The runtime's republisher is additive — consumers that want the typed cross-subsystem stream subscribe to `com.nexus.ai.runtime.*`; consumers that only care about raw token streams keep their existing subscription. This preserves the closed-enum + `Custom` design (ADR 0007) and avoids the temptation to add new `NexusEvent` variants.

### Integration with `nexus-workflow`

The `notify` step (BL-133 follow-up, `crates/nexus-workflow/src/core_plugin.rs:2128`) and `ai_prompt` / `ai_decision` steps remain executed by `nexus-workflow::executor`. Today's behaviour is preserved. The new integration is that a workflow step can optionally declare `async = true`, in which case the executor `ipc_call`s `com.nexus.ai.runtime::submit` and records the returned `task_id` in the run history instead of awaiting the AI step inline. The workflow's own retry policy (`max_retries` / `retry_backoff`) still applies — a `Failed` `AiEvent` decrements the retry counter and resubmits.

### Integration with `nexus-agent`

`com.nexus.agent` keeps its IPC surface unchanged. Internally, **callers that want long-running session orchestration go through the runtime**, not directly through `session_run`. The agent's `delegate` handler (`HANDLER_DELEGATE = 24`) becomes a thin shim that builds an `AgentTask { kind: Session(..), parent: Some(current_task) }` and submits it to the runtime. Parallel and pipeline composition (the retired BL-027 orchestrator) re-emerge as runtime concepts — `submit` can take a `parent` ref so the scheduler tracks DAGs of agent runs.

`BusBridgePolicy`'s `pending_approvals` map (`core_plugin.rs:196`) stays where it is. The runtime does not own approval state — approval is a per-session concern owned by `com.nexus.agent`, and the runtime simply forwards `RoundProposed` / `RoundDecided` events through its typed envelope.

### Integration with `nexus-notifications`

`com.nexus.notifications` already has the right shape — a pluggable `Transport` trait with four impls (`DesktopTransport`, `DiscordWebhook`, `TelegramBot`, `SmtpTransport`, see `crates/nexus-notifications/src/lib.rs:152`–`452`) and an event-driven entry on the bus (`com.nexus.notifications.delivered`, `lib.rs:74`). Three producers already use it: workflow's `notify` step, agent's auto-notify-on-threshold (BL-133 follow-up), and the shell's toast subscriber. **We do not introduce a parallel notification system**; the runtime's role is to (1) become a first-class *source* the notification subsystem can route, and (2) take the opportunity to fix the producer-side coupling that's about to bite once `AiEvent` adds a fourth high-volume source.

The concrete moves:

1. **`AiEvent` is a notification source.** A built-in subscriber inside `nexus-notifications` listens to `com.nexus.ai.runtime.*` and translates configured events (`Finished` with `outcome != Success`, `Failed`, `Paused { reason: "awaiting_approval" }`, and threshold-crossing `Started`/`Finished` pairs) into `Notification` records. The agent's existing threshold auto-notify migrates to listen on the runtime's typed `Finished` envelope instead of inspecting `session_run` return values.

2. **Pull producer→transport routing out of producers.** Today each producer (`notify` step, threshold auto-notify, toast subscriber) hardcodes which `Channel`/transport it targets. As `AiEvent` adds a fourth source this will scale poorly. Introduce a single forge-local config — `<forge>/.forge/notifications.toml` — that the notifications plugin reads on `on_start` and reloads via the existing file watcher. Schema (proposed):

   ```toml
   [sources.ai_runtime]
   on = ["finished:failed", "failed", "paused:awaiting_approval"]
   route = ["desktop", "telegram"]
   min_severity = "warn"
   quiet_hours = "22:00-07:00"

   [sources.workflow]
   on = ["step.failed", "run.completed"]
   route = ["desktop"]

   [channels.telegram]
   chat_id = "..."   # existing TelegramBot config moves here
   ```

   Producers stop choosing channels. They emit a `Notification` with a `source` tag; the notifications plugin's new router applies rules and fans out to transports. This is a refactor of `nexus-notifications`, not a new subsystem, and is tracked in **Phase 6** below.

3. **Notification center (history + read/unread) is a follow-up, not this ADR.** Today `notifications.delivered` is fire-and-forget. A small persistent table under `.forge/notifications/inbox.db` plus a shell panel querying it via a `com.nexus.notifications::list` handler is the natural next step. It re-uses the runtime's pattern (derived store under `.forge/`, listed via IPC, observable on the bus) and so belongs in a sibling ADR after 0028 lands. Listed under Open follow-ups.

No new dependency edge: `nexus-notifications` already subscribes to bus topics it doesn't own. `nexus-ai-runtime` does **not** depend on `nexus-notifications`; the integration is purely event-flow.

## Invariants preserved

The four invariants from `CLAUDE.md`:

1. **File-as-truth.** The runtime's run-history store at `<forge>/.forge/ai-runtime/runs.db` is rebuildable from the agent session transcripts already persisted under `.forge/agent/sessions/`; the runtime's store is a derived index, not a source of record.
2. **Microkernel isolation.** `nexus-ai-runtime` depends only on `nexus-kernel`, `nexus-plugin-api`, `nexus-types`. The kernel does not depend on it. `nexus-bootstrap` wires it as one more `CorePlugin` via `register_core` (ADR 0016). The dep-invariants test (`crates/nexus-bootstrap/tests/dep_invariants.rs`) gets one new row preventing CLI/TUI from depending on `nexus-ai-runtime` directly — callers route through `ipc_call`.
3. **IPC over direct calls.** Every consumer — CLI, TUI, MCP server, shell, workflow plugin, agent plugin — submits and observes via `ipc_call("com.nexus.ai.runtime", ...)`. The runtime in turn issues all its work via `ipc_call` against `com.nexus.agent` / `com.nexus.ai`; no direct linking.
4. **Capabilities gate everything.** Three new capabilities (`ai.runtime.submit` / `.control` / `.observe`) plus full capability snapshotting at submit time. A task cannot escalate beyond its caller's capability set.

## Consequences

### Positive

- **One queue, one observation surface.** The shell's agent panel, observability panel, popout-window agent renderer, CLI `nexus agent runs ls`, and MCP server's run-status tools all subscribe to one bus prefix and `ipc_call` one plugin.
- **Heavy AI work cannot starve the IPC runtime.** The dedicated multi-thread tokio Runtime is the sole executor for `Session` / `AiStream` / `WorkflowAiStep` tasks.
- **Typed events.** `AiEvent` lives in one Rust source and one ts-rs export (`packages/nexus-extension-api/src/generated/ipc/AiEvent.ts`); consumers stop reinventing payload shapes.
- **Composition primitives are explicit.** Parent/child task linkage replaces the comment-only "callers should fan out / chain `session_run` directly" stance — parallel and pipeline composition become library APIs again.
- **Cancellation works.** A run that is `Cancelled` cleans up its `BusBridgePolicy` slot via a signalled `oneshot::Receiver`.
- **Workflow / agent stay slim.** No new responsibilities in `nexus-agent` or `nexus-workflow`; the runtime is the only new owner.
- **Aligns with `nexus-ai::indexing_daemon`.** The one-off "I needed my own runtime" pattern becomes a system-wide primitive.

### Negative

- **A new core crate (24 → 25 service plugins, 28 → 29 workspace members).** ADR 0004's addendum lists 28; this proposal adds one.
- **Three new capabilities to grant correctly.** Initial grants are documented above; a wrong default risks either over-broad submission rights or a shell that can't list runs.
- **Republisher latency.** Every existing AI / agent event gets a typed wrapper before reaching the new `com.nexus.ai.runtime.*` topic. Sub-millisecond on modern hardware, but it doubles the bus traffic for token streams. Mitigation: token-stream republishing is opt-in per task (`AgentTask` field `publish_token_chunks: bool`, default `false`). Consumers that want raw tokens subscribe to the existing `com.nexus.ai.stream_chunk` topic directly.
- **Run-history persistence is a new derived store.** Adds one more rebuildable artefact under `.forge/` — invariant 1 is preserved but the operational surface grows.
- **`delegate` shim is a public-API change.** Today `delegate` returns the child `AgentSession` synchronously; under the proposal it returns a `task_id` and the caller is expected to subscribe. This is a meaningful caller contract change for any shell code already using `delegate`. Mitigation: keep a `delegate.v1` alias (per ADR 0021 handler-versioning) that retains the synchronous-await shape by internally polling the runtime until the child task finishes.

### Neutral

- Existing `com.nexus.ai.stream_*` and `com.nexus.agent.round_proposed` topics remain unchanged; this is purely additive. Callers that don't want the runtime can keep using the existing handlers as-is.
- The runtime is the right place for an eventual **task DAG visualiser** in the observability panel — but that's a follow-up, not part of this ADR.

## Alternatives considered

### A. Grow each subsystem independently

Add an in-process task queue to `nexus-agent`; add typed AI events to a new shared types module; spin up a worker pool inside the shell's Tauri runtime.

**Rejected.** This is the path of least resistance and the one we've been on. The current `BusBridgePolicy::pending_approvals` map already prototypes the queue, the `stream_*` topics prototype the channel, and `indexing_daemon` prototypes the pool. Doing all three separately means three crates own three half-features that need to be coordinated whenever one changes. The invariants stay technically satisfied but the operational picture is a mess — three observation surfaces, three retry stories, three lifecycle policies.

### B. Put the runtime in the kernel

Make the agent task queue a kernel primitive alongside the event bus and KV store.

**Rejected.** Violates invariant 2 (kernel never depends on a subsystem). The runtime needs to reason about `SessionOutcome`, `RoundDecision`, `StreamChat` payloads — all of which live in `nexus-agent` / `nexus-ai`. Routing those types through the kernel would either pull subsystem types into the kernel or force a generic-over-payload abstraction. ADR 0016 already settled this: heavy native subsystems are core plugins, not kernel facilities.

### C. Bury the runtime inside `nexus-agent`

Add scheduler, worker pool, and event channel as private modules under `nexus-agent/src/`.

**Rejected.** Three reasons. (1) `nexus-ai` has long-running concerns of its own (`indexing_daemon`, embedding warmup, RAG retrieval) that have nothing to do with the agent — they should not have to import `nexus-agent` to get a worker pool. (2) Workflow's `notify` / `ai_prompt` / `ai_decision` async-step ambitions also belong to neither agent nor AI. (3) Putting an N-thread tokio Runtime inside `nexus-agent` makes `nexus-agent` non-trivially expensive to load even for the CLI's `nexus agent list-archetypes` use case. The runtime is a *peer* of agent/ai/workflow, not a feature of any one of them.

### D. Wire the worker pool to the shell only

Spawn the AI worker pool only inside `shell/src-tauri` and route everything through Tauri commands.

**Rejected.** Violates invariant 3 (IPC over direct calls). The CLI / TUI / MCP server all need the same orchestration semantics. They cannot reach a shell-resident pool.

### E. Adopt a generic job queue (Celery-style, Faktory, SQLite-backed)

**Rejected.** Overkill for a single-process desktop app. Adds operational complexity (queue persistence semantics, retry-on-restart, exactly-once delivery, cross-process locking) that we don't need. The runtime's in-process scheduler plus a derived run-history store is sufficient for the design goals.

## Migration

1. **Phase 1 — runtime + observation only.** Land `nexus-ai-runtime` with `submit` / `get` / `list` / `events` / `pool_stats`. Implement only `AgentTaskKind::Session`. Republish existing `com.nexus.ai.*` and `com.nexus.agent.*` events as typed `AiEvent`s. Shell adds an observability panel reading from `events`. No caller changes yet.
2. **Phase 2 — agent delegation.** Re-implement `com.nexus.agent::delegate` (HANDLER_DELEGATE = 24) on top of the runtime. Add `delegate.v1` alias preserving the synchronous-await shape (ADR 0021).
3. **Phase 3 — workflow async steps.** Add `async = true` to workflow step parsing. `notify` / `ai_prompt` / `ai_decision` steps can opt in.
4. **Phase 4 — indexing daemon migration.** `nexus-ai::indexing_daemon` submits its work via the runtime; its bespoke `tokio::runtime::Builder::new_current_thread()` is removed.
5. **Phase 5 — cancellation + pause/resume.** Adds `cancel` / `pause` / `resume` handlers and the `Critical` priority bucket.
6. **Phase 6 — notification router refactor.** Move producer-side channel selection out of producers; introduce `<forge>/.forge/notifications.toml` and a router inside `nexus-notifications`. The runtime's `AiEvent` stream becomes the fourth notification source alongside workflow, agent threshold, and shell toast. Pre-req for the separate Notification-Center ADR.

Phases 1–2 land on top of the existing `session_run` machinery without behaviour changes; phases 3+ are opt-in. Phase 6 is independent and can land in parallel with Phase 1.

## Open follow-ups

- **DAG visualiser.** Parent/child task linkage admits a fan-out tree; the observability panel will want a renderer.
- **Distributed runtime.** Out of scope — a multi-host Nexus would need to push the runtime's queue into a real broker. Not on the roadmap.
- **Quotas per caller.** A workflow that submits a thousand AI steps in a second should hit a per-caller rate limit. Tracked separately.
- **Persistence across restart.** The Phase-1 run history is read-only after restart; resuming a Paused run after a process restart is a follow-up.
- **Notification Center (separate ADR).** Persistent inbox under `.forge/notifications/inbox.db` with read/unread state, filtering, and a shell panel. Builds on the Phase-6 router. Should be its own ADR (sibling to 0028) so the design discussion isn't buried inside the runtime ADR.

## References

- `crates/nexus-agent/src/session.rs` — current session loop primitives.
- `crates/nexus-agent/src/core_plugin.rs` — `BusBridgePolicy` and the `pending_approvals` map (prototype queue).
- `crates/nexus-ai/src/core_plugin.rs:1385` — `stream_*` bus publish helpers (prototype channel).
- `crates/nexus-ai/src/indexing_daemon.rs` — dedicated `tokio::runtime::Runtime` (prototype pool).
- `crates/nexus-workflow/src/core_plugin.rs:359` — workflow scheduler using `Handle::try_current()`.
- `crates/nexus-notifications/src/lib.rs` — toast subscriber and Telegram transport showing the auto-notify pattern.
- `crates/nexus-bootstrap/tests/dep_invariants.rs` — boundary tests; one new row covers the runtime.
- ADR 0004 — crate boundaries (single new row in the addendum once accepted).
- ADR 0011 — plugin-first shell; observability panel lands as `shell/src/plugins/nexus/aiRuntime/`.
- ADR 0016 — native plugin path used by the runtime.
- ADR 0020 — popout windows; observability subscribes via the same kernel bus, so popouts see runtime events for free.
- ADR 0021 — handler versioning convention for `delegate.v1`.
- ADR 0022 — per-handler AI capability inventory the new `ai.runtime.*` caps extend.
- ADR 0023 / 0024 — agent on AI tool registry and session loop the runtime drives.

