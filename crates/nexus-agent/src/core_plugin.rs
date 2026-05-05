//! Core plugin wrapping the agent library.
//!
//! Registers as `com.nexus.agent`. Holds a [`KernelPluginContext`]
//! (supplied via [`CorePlugin::wire_context`] at bootstrap) so its
//! handlers can drive two bridges against the live runtime:
//!
//! - [`nexus_bootstrap::agent::AiChatDriver`]-shaped adapter over
//!   `com.nexus.ai::stream_chat` for planning.
//! - [`nexus_bootstrap::agent::KernelToolDispatcher`]-shaped adapter
//!   over `PluginContext::ipc_call` for executing plan steps.
//!
//! Because this module lives in `nexus-agent`, it re-implements the
//! two adapter shapes locally — keeping the library itself
//! kernel-free would otherwise force a circular dep on
//! `nexus-bootstrap`. The bridges here and in bootstrap are
//! intentionally identical in behaviour.
//!
//! # Handlers
//!
//! | Handler id | Command             | Purpose                               |
//! |-----------:|---------------------|---------------------------------------|
//! | 1          | `plan`              | Produce a [`Plan`] from a goal        |
//! | 2          | `run`               | Plan + execute; return Observation    |
//! | 3          | `run_plan`          | Execute a preset [`Plan`]             |
//! | 4          | `execute_step`      | Execute a single preset-plan step     |
//! | 5          | `history_list`      | List persisted plan histories         |
//! | 6          | `history_get`       | Load one persisted history entry      |
//! | 7          | `history_delete`    | Remove one persisted history entry    |
//! | 8          | `list_archetypes`   | Return the catalogue of archetype ids |
//!
//! Ids are append-only.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{
    build_archetype, orchestrator::AgentOrchestrator, Agent, AgentError, ChatDriver, LlmAgent,
    Plan, PlanExecutor, ToolCall, ToolDispatcher, TraceEntry, DEFAULT_SYSTEM_PROMPT,
};

/// Short archetype names accepted by [`crate::archetypes::resolve_prompt`].
/// Exposed via the `list_archetypes` handler so the shell's picker can
/// send any of these back as the `archetype` arg to `plan` / `run`
/// without guessing the expected case or prefix.
const ARCHETYPE_NAMES: &[&str] = &["writer", "coder", "researcher"];

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.agent";

/// `plan` handler id — produce a plan for the given goal.
pub const HANDLER_PLAN: u32 = 1;
/// `run` handler id — plan + execute in one call.
pub const HANDLER_RUN: u32 = 2;
/// `run_plan` handler id — execute a preset plan.
pub const HANDLER_RUN_PLAN: u32 = 3;
/// `execute_step` handler id — execute a single step of a preset
/// plan. Enables per-step approval flows driven by the UI.
pub const HANDLER_EXECUTE_STEP: u32 = 4;
/// `history_list` handler id — enumerate persisted plan histories
/// under `<forge>/.forge/agent/history/`.
pub const HANDLER_HISTORY_LIST: u32 = 5;
/// `history_get` handler id — load one persisted history entry by
/// plan id.
pub const HANDLER_HISTORY_GET: u32 = 6;
/// `history_delete` handler id — remove one persisted history entry.
pub const HANDLER_HISTORY_DELETE: u32 = 7;
/// `list_archetypes` handler id — return the catalogue of archetype
/// ids the agent library knows about (OI-04). Payload: `[]`. Result:
/// `Vec<String>` — fully-qualified archetype ids (e.g.
/// `com.nexus.agent.writer`). The shell uses this to populate the
/// archetype picker without a hardcoded catalogue.
pub const HANDLER_LIST_ARCHETYPES: u32 = 8;
/// `delegate` handler id — orchestrator hands one goal to one
/// archetype, plans + executes, returns an [`crate::Observation`].
/// Args: [`DelegateArgs`]. (BL-027.)
pub const HANDLER_DELEGATE: u32 = 9;
/// `parallel` handler id — fan out a list of `(archetype, goal)`
/// jobs in parallel; results come back in input order. Args:
/// [`ParallelArgs`]. (BL-027.)
pub const HANDLER_PARALLEL: u32 = 10;
/// `pipeline` handler id — run stages sequentially; each stage's
/// summary becomes `{{prev}}` in the next stage's `goal_template`.
/// Stops on first failure and returns partial results. Args:
/// [`PipelineArgs`]. (BL-027.)
pub const HANDLER_PIPELINE: u32 = 11;
/// `trace_get` handler id — return the orchestrator's trace log
/// (one [`crate::TraceEntry`] per delegated stage). Payload: `[]`.
/// (BL-027.)
pub const HANDLER_TRACE_GET: u32 = 12;

/// Default per-tool-call timeout used by the executor when no
/// caller-provided override lands. Matches the bootstrap bridge.
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// Default chat timeout; planner prompts can cost remote-provider
/// latency. Matches the bootstrap bridge.
const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

/// Core plugin instance.
pub struct AgentCorePlugin {
    context: Option<Arc<KernelPluginContext>>,
    orchestrator: std::sync::Mutex<Option<Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>>>>,
}

impl AgentCorePlugin {
    /// Construct an unwired plugin. Bootstrap must call
    /// [`CorePlugin::wire_context`] before the first dispatch; any
    /// handler that fires before then returns a clear error.
    #[must_use]
    pub fn new() -> Self {
        Self {
            context: None,
            orchestrator: std::sync::Mutex::new(None),
        }
    }

    /// Lazily build (or return the cached) orchestrator. State —
    /// scratch + trace — persists for the plugin's lifetime so
    /// `trace_get` can read entries appended by earlier
    /// `delegate` / `parallel` / `pipeline` calls.
    fn orchestrator(
        &self,
        ctx: &Arc<KernelPluginContext>,
    ) -> Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>> {
        let mut guard = self
            .orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = guard.as_ref() {
            return Arc::clone(existing);
        }
        let driver = AiChatBridge {
            ctx: Arc::clone(ctx),
            timeout: DEFAULT_CHAT_TIMEOUT,
        };
        let dispatcher = KernelToolBridge {
            ctx: Arc::clone(ctx),
            timeout: DEFAULT_TOOL_TIMEOUT,
        };
        let built = Arc::new(AgentOrchestrator::new(driver, dispatcher));
        *guard = Some(Arc::clone(&built));
        built
    }
}

impl Default for AgentCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AgentCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // `list_archetypes` is the one sync handler on this plugin — it
        // reads only from compile-time `archetypes.rs` constants so
        // there's no reason to burn an async hop. Every other handler
        // is kernel-context-dependent and lives in `dispatch_async`.
        if handler_id == HANDLER_LIST_ARCHETYPES {
            return Ok(serde_json::json!(ARCHETYPE_NAMES));
        }
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: agent commands are async; caller should use dispatch_async"
            ),
        })
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        // Let the sync path handle `list_archetypes` — the kernel's
        // `ipc_call` prefers `dispatch_async` when Some is returned,
        // and we don't want to hop an unnecessary async frame for a
        // pure compile-time constant read.
        if handler_id == HANDLER_LIST_ARCHETYPES {
            return None;
        }
        let ctx = self.context.clone();
        let orchestrator = ctx.as_ref().map(|c| self.orchestrator(c));
        let args = args.clone();
        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err("agent plugin context not wired (bootstrap incomplete)".into())
            })?;
            match handler_id {
                HANDLER_PLAN => handle_plan(ctx, &args).await,
                HANDLER_RUN => handle_run(ctx, &args).await,
                HANDLER_RUN_PLAN => handle_run_plan(ctx, &args).await,
                HANDLER_EXECUTE_STEP => handle_execute_step(ctx, &args).await,
                HANDLER_HISTORY_LIST => handle_history_list(ctx).await,
                HANDLER_HISTORY_GET => handle_history_get(ctx, &args).await,
                HANDLER_HISTORY_DELETE => handle_history_delete(ctx, &args).await,
                HANDLER_DELEGATE => {
                    handle_delegate(orchestrator.expect("ctx present"), &args).await
                }
                HANDLER_PARALLEL => {
                    handle_parallel(orchestrator.expect("ctx present"), &args).await
                }
                HANDLER_PIPELINE => {
                    handle_pipeline(orchestrator.expect("ctx present"), &args).await
                }
                HANDLER_TRACE_GET => handle_trace_get(orchestrator.expect("ctx present")).await,
                other => Err(exec_err(format!("unknown handler id {other}"))),
            }
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ── Handler impls ───────────────────────────────────────────────────────────

/// Args for `com.nexus.agent::plan` and `::run` (handler ids `1`, `2`).
/// Lifted from inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GoalArgs {
    goal: String,
    #[serde(default)]
    archetype: Option<String>,
}

/// Args for `com.nexus.agent::run_plan` (handler id `7`). Lifted from
/// inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    plan: Plan,
}

async fn handle_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "plan")?;
    let agent = build_planner(Arc::clone(&ctx), &a).await;
    let plan = agent.plan(&a.goal).await.map_err(|e| agent_err(&e))?;
    to_value(&plan, "plan")
}

async fn handle_run(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "run")?;
    let agent = build_planner(Arc::clone(&ctx), &a).await;
    let plan = agent.plan(&a.goal).await.map_err(|e| agent_err(&e))?;
    run_plan_internal(ctx, plan, Some(a.goal)).await
}

async fn build_planner(
    ctx: Arc<KernelPluginContext>,
    args: &GoalArgs,
) -> LlmAgent<AiChatBridge> {
    let skills_prompt = system_prompt_with_skills(&ctx, &args.goal).await;
    // `system_prompt_with_skills` returns DEFAULT_SYSTEM_PROMPT as its
    // baseline when no skills match; strip that prefix so we can layer
    // the archetype's prompt as the new baseline without duplicating
    // the schema block.
    let extra = skills_prompt
        .strip_prefix(DEFAULT_SYSTEM_PROMPT)
        .map(str::trim_start)
        .filter(|s| !s.is_empty());
    let driver = AiChatBridge {
        ctx,
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    build_archetype(args.archetype.as_deref(), driver, extra)
}

async fn handle_run_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanArgs = parse(args, "run_plan")?;
    run_plan_internal(ctx, a.plan, None).await
}

/// Args for `com.nexus.agent::execute_step` (handler id `4`). Lifted
/// from inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ExecuteStepArgs {
    plan: Plan,
    index: usize,
}

async fn handle_execute_step(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: ExecuteStepArgs = parse(args, "execute_step")?;
    let executor = PlanExecutor::new(KernelToolBridge {
        ctx,
        timeout: DEFAULT_TOOL_TIMEOUT,
    });
    let result = executor
        .execute_step_at(&a.plan, a.index)
        .await
        .map_err(|e| agent_err(&e))?;
    to_value(&result, "execute_step")
}

async fn run_plan_internal(
    ctx: Arc<KernelPluginContext>,
    plan: Plan,
    goal: Option<String>,
) -> Result<serde_json::Value, PluginError> {
    let executor = PlanExecutor::new(KernelToolBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_TOOL_TIMEOUT,
    });

    // Drive the plan step-by-step so we can publish kernel-bus events
    // around each dispatch. The UI subscribes via `ctx.kernel.on` in the
    // shell's agent plugin and updates the pending-plan card live instead
    // of blocking until the whole plan completes.
    let _ = ctx.publish(
        EVENT_RUN_START,
        serde_json::json!({
            "plan_id": plan.id,
            "steps": plan.steps.len(),
            "goal": goal,
        }),
    );

    let mut results: Vec<crate::StepResult> = Vec::with_capacity(plan.steps.len());
    let mut abort: Option<AgentError> = None;

    for (idx, step) in plan.steps.iter().enumerate() {
        if abort.is_some() {
            results.push(crate::StepResult {
                step_id: step.id.clone(),
                response: None,
                status: crate::StepStatus::Skipped,
            });
            let _ = ctx.publish(
                EVENT_STEP_DONE,
                serde_json::json!({
                    "plan_id": plan.id,
                    "step_id": step.id,
                    "index": idx,
                    "status": "skipped",
                }),
            );
            continue;
        }
        let _ = ctx.publish(
            EVENT_STEP_START,
            serde_json::json!({
                "plan_id": plan.id,
                "step_id": step.id,
                "index": idx,
                "description": step.description,
            }),
        );
        match executor.execute_step_at(&plan, idx).await {
            Ok(result) => {
                let _ = ctx.publish(
                    EVENT_STEP_DONE,
                    serde_json::json!({
                        "plan_id": plan.id,
                        "step_id": step.id,
                        "index": idx,
                        "status": "ok",
                    }),
                );
                results.push(result);
            }
            Err(err) => {
                results.push(crate::StepResult {
                    step_id: step.id.clone(),
                    response: None,
                    status: crate::StepStatus::Failed,
                });
                let _ = ctx.publish(
                    EVENT_STEP_DONE,
                    serde_json::json!({
                        "plan_id": plan.id,
                        "step_id": step.id,
                        "index": idx,
                        "status": "failed",
                        "error": err.to_string(),
                    }),
                );
                abort = Some(err);
            }
        }
    }

    let success = abort.is_none();
    let observation = crate::Observation {
        plan_id: plan.id.clone(),
        steps: results,
        success,
    };
    let _ = ctx.publish(
        EVENT_RUN_DONE,
        serde_json::json!({
            "plan_id": plan.id,
            "success": success,
        }),
    );
    save_history(&ctx, &plan, &observation, goal.as_deref()).await;
    to_value(&observation, "run")
}

/// Kernel-bus topics emitted while a plan runs. Consumed by the shell's
/// agent plugin via `ctx.kernel.on("com.nexus.agent.")` for live
/// pending-plan updates (historically mirrored as `agent:run_start` /
/// `agent:step_start` / `agent:step_done` / `agent:run_done` Tauri events
/// by the legacy shell, which has since been retired).
pub const EVENT_RUN_START: &str = "com.nexus.agent.run_start";
/// See [`EVENT_RUN_START`].
pub const EVENT_STEP_START: &str = "com.nexus.agent.step_start";
/// See [`EVENT_RUN_START`].
pub const EVENT_STEP_DONE: &str = "com.nexus.agent.step_done";
/// See [`EVENT_RUN_START`].
pub const EVENT_RUN_DONE: &str = "com.nexus.agent.run_done";

// ── History persistence ─────────────────────────────────────────────────────

const HISTORY_DIR: &str = ".forge/agent/history";

fn history_path(plan_id: &str) -> Option<std::path::PathBuf> {
    // Same alphabet as com.nexus.ai session ids — belt-and-braces
    // path-traversal guard since plan ids are model-derived.
    if plan_id.is_empty() || plan_id.len() > 96 {
        return None;
    }
    let safe = plan_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !safe {
        return None;
    }
    Some(std::path::PathBuf::from(HISTORY_DIR).join(format!("{plan_id}.json")))
}

/// Best-effort — history failures are logged but never bubble up as
/// plugin errors. The caller has a good run; persistence is
/// secondary.
async fn save_history(
    ctx: &KernelPluginContext,
    plan: &Plan,
    observation: &crate::Observation,
    goal: Option<&str>,
) {
    let Some(path) = history_path(&plan.id) else {
        tracing::warn!(plan_id = %plan.id, "skipping history save — unsafe plan id");
        return;
    };
    let record = serde_json::json!({
        "plan_id": plan.id,
        "goal": goal,
        "plan": plan,
        "observation": observation,
        "created_at": timestamp(),
    });
    match serde_json::to_vec_pretty(&record) {
        Ok(bytes) => {
            // Route through storage `write_file` so the atomic-write
            // helper's mkdir -p runs. `ctx.write_file` is plain
            // `tokio::fs::write` and would silently fail on a fresh
            // forge where `.forge/agent/history/` doesn't yet exist.
            let Some(path_str) = path.to_str() else {
                tracing::warn!(plan_id = %plan.id, "history path not UTF-8");
                return;
            };
            let call = ctx
                .ipc_call(
                    "com.nexus.storage",
                    "write_file",
                    serde_json::json!({ "path": path_str, "bytes": bytes }),
                    Duration::from_secs(10),
                )
                .await;
            if let Err(err) = call {
                tracing::warn!(plan_id = %plan.id, %err, "history write failed");
            }
        }
        Err(err) => tracing::warn!(plan_id = %plan.id, %err, "history encode failed"),
    }
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("ts-{secs}")
}

async fn handle_history_list(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let dir = std::path::Path::new(HISTORY_DIR);
    let Ok(entries) = ctx.list_files(dir).await else {
        return Ok(serde_json::Value::Array(Vec::new()));
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for path in entries {
        let Some(plan_id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| history_path(s).is_some())
            .map(ToString::to_string)
        else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let goal = record
            .get("goal")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let created_at = record
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let success = record
            .get("observation")
            .and_then(|o| o.get("success"))
            .and_then(serde_json::Value::as_bool);
        let step_count = record
            .get("observation")
            .and_then(|o| o.get("steps"))
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        out.push(serde_json::json!({
            "plan_id": plan_id,
            "goal": goal,
            "created_at": created_at,
            "success": success,
            "steps": step_count,
            "bytes": bytes.len(),
        }));
    }
    Ok(serde_json::Value::Array(out))
}

/// Args for `com.nexus.agent::history_get` and related plan-id-keyed
/// handlers. Lifted from inline by audit-2026-05-01 P1-3 (#113).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PlanIdArgs {
    plan_id: String,
}

async fn handle_history_get(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanIdArgs = parse(args, "history_get")?;
    let path = history_path(&a.plan_id)
        .ok_or_else(|| exec_err(format!("history_get: invalid plan_id '{}'", a.plan_id)))?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("history_get: {e}")))?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| exec_err(format!("history_get: invalid JSON on disk: {e}")))
}

async fn handle_history_delete(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanIdArgs = parse(args, "history_delete")?;
    let path = history_path(&a.plan_id).ok_or_else(|| {
        exec_err(format!("history_delete: invalid plan_id '{}'", a.plan_id))
    })?;
    ctx.delete_file(&path)
        .await
        .map_err(|e| exec_err(format!("history_delete: {e}")))?;
    Ok(serde_json::json!({ "deleted": true, "plan_id": a.plan_id }))
}

// ── Skill-aware system prompt assembly ─────────────────────────────────────

/// Build a planner system prompt that layers in any skill whose
/// triggers match the goal text. Calls `com.nexus.skills::triggered_by`
/// best-effort — failures (plugin not registered, disk errors) fall
/// back silently to [`DEFAULT_SYSTEM_PROMPT`] so the agent still
/// works in forges without a skills directory.
async fn system_prompt_with_skills(
    ctx: &KernelPluginContext,
    goal: &str,
) -> String {
    let mut prompt = String::from(DEFAULT_SYSTEM_PROMPT);
    append_mcp_hint(ctx, &mut prompt).await;

    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "triggered_by",
            serde_json::json!({ "text": goal }),
            Duration::from_secs(5),
        )
        .await;
    let Ok(value) = response else {
        return prompt;
    };
    let skills: Vec<serde_json::Value> = match serde_json::from_value(value) {
        Ok(v) => v,
        Err(_) => return prompt,
    };
    if skills.is_empty() {
        return prompt;
    }

    prompt.push_str(
        "\n\nThe following skills match this goal — apply their guidance \
         when producing the plan. Each skill is delimited by a heading.\n",
    );
    for skill in &skills {
        let name = skill
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(unnamed)");
        let id = skill
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let fallback_body = skill
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        // BL-021 — prefer the composed (depends_on-resolved) body so
        // an inheritance chain like `concise → markdown-style → rust`
        // contributes every layer's instructions in topo order. Fall
        // back to the rendered single-skill body, then to the raw
        // body, when compose isn't available (older registry, cycle
        // / missing-dep, etc.).
        let composed = compose_skill_body(ctx, id).await;
        let body = match composed {
            Some(merged) => merged,
            None => render_skill_body(ctx, id)
                .await
                .unwrap_or_else(|| fallback_body.to_string()),
        };
        let _ = write!(prompt, "\n## Skill: {name} [{id}]\n{body}\n");
    }
    prompt
}

/// BL-021 — call `com.nexus.skills::compose` and return the merged
/// body string. Returns `None` for a missing handler / unknown skill /
/// cycle / missing dependency — every error path falls back to the
/// pre-BL-021 single-skill render so a broken dep graph never blocks
/// planning. Also surfaces conflict warnings (if any) through `tracing`
/// so operators can see them in logs without us having to plumb an
/// event channel through to the UI for the planner.
async fn compose_skill_body(ctx: &KernelPluginContext, id: &str) -> Option<String> {
    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "compose",
            serde_json::json!({ "id": id }),
            Duration::from_secs(5),
        )
        .await
        .ok()?;
    if let Some(arr) = response.get("conflicts").and_then(serde_json::Value::as_array) {
        if !arr.is_empty() {
            tracing::warn!(
                skill_id = id,
                conflict_count = arr.len(),
                "com.nexus.skills::compose returned non-fatal conflicts"
            );
        }
    }
    response
        .get("merged_body")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

/// Query `com.nexus.mcp.host::list_servers` and, for each enabled
/// server, `list_tools`. Append a compact advertisement to the
/// planner prompt so the LLM knows what external MCP tools are
/// reachable and how to call them (`target_plugin_id:
/// "com.nexus.mcp.host"`, `command_id: "call_tool"`, args shape).
///
/// Best-effort: any failure (plugin not registered, server crashed,
/// timeout) logs at debug and the prompt is left unchanged.
async fn append_mcp_hint(ctx: &KernelPluginContext, prompt: &mut String) {
    let Ok(servers_value) = ctx
        .ipc_call(
            "com.nexus.mcp.host",
            "list_servers",
            serde_json::json!({}),
            Duration::from_secs(3),
        )
        .await
    else {
        return;
    };
    let Some(servers) = servers_value.as_array() else {
        return;
    };
    let active: Vec<(&str, &[serde_json::Value])> = servers
        .iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(|v| v.as_str())?;
            let disabled = s
                .get("disabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if disabled {
                return None;
            }
            let args = s
                .get("args")
                .and_then(|v| v.as_array())
                .map_or(&[][..], Vec::as_slice);
            Some((name, args))
        })
        .collect();
    if active.is_empty() {
        return;
    }

    prompt.push_str(
        "\n\nExternal MCP servers are available via \
         `com.nexus.mcp.host::call_tool` with args \
         `{ server, tool, arguments }`. Servers:\n",
    );
    for (name, _args) in &active {
        let _ = write!(prompt, "- {name}");
        // Optional: fetch tool names when the server responds quickly.
        // Keep this light — a slow server shouldn't hold up planning.
        let tools_value = ctx
            .ipc_call(
                "com.nexus.mcp.host",
                "list_tools",
                serde_json::json!({ "server": name }),
                Duration::from_secs(3),
            )
            .await;
        if let Ok(v) = tools_value {
            if let Some(arr) = v.as_array() {
                let names: Vec<_> = arr
                    .iter()
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                    .take(8)
                    .collect();
                if !names.is_empty() {
                                let _ = write!(prompt, " — tools: {}", names.join(", "));
                    if arr.len() > names.len() {
                        let _ = write!(prompt, " (+{} more)", arr.len() - names.len());
                    }
                }
            }
        }
        prompt.push('\n');
    }
}

/// Best-effort call to `com.nexus.skills::render` with no override
/// values — lets frontmatter `default`s substitute into the body.
/// Returns `None` when the handler errors (e.g. required parameter
/// with no default); caller falls back to the raw body.
async fn render_skill_body(ctx: &KernelPluginContext, id: &str) -> Option<String> {
    let response = ctx
        .ipc_call(
            "com.nexus.skills",
            "render",
            serde_json::json!({ "id": id, "values": {} }),
            Duration::from_secs(5),
        )
        .await
        .ok()?;
    response
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

// ── Local adapters mirroring nexus-bootstrap::agent ────────────────────────

#[derive(Clone)]
struct AiChatBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ChatDriver for AiChatBridge {
    async fn propose(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<crate::Proposal, String> {
        propose_via_ai(&self.ctx, self.timeout, system, user_message).await
    }
}

/// Shared `propose_tool_calls` IPC dance used by the in-tree
/// `AiChatBridge` and `nexus_bootstrap::agent::AiChatDriver` (G7-1b,
/// ADR 0023). Decodes [`AiProposeReply`]-shaped JSON into the
/// agent-side [`crate::Proposal`] without taking a dependency on
/// `nexus-ai`'s types.
async fn propose_via_ai(
    ctx: &KernelPluginContext,
    timeout: Duration,
    system: &str,
    user_message: &str,
) -> Result<crate::Proposal, String> {
    #[derive(Deserialize)]
    struct ProposeWire {
        #[serde(default)]
        text: String,
        #[serde(default)]
        tool_calls: Vec<ProposedWire>,
    }
    #[derive(Deserialize)]
    struct ProposedWire {
        id: String,
        name: String,
        target_plugin_id: String,
        command_id: String,
        args: serde_json::Value,
    }

    let args = serde_json::json!({
        "messages": [{ "role": "user", "content": user_message }],
        "system": system,
    });
    let raw = ctx
        .ipc_call("com.nexus.ai", "propose_tool_calls", args, timeout)
        .await
        .map_err(|e| e.to_string())?;
    let parsed: ProposeWire = serde_json::from_value(raw).map_err(|e| e.to_string())?;
    let tool_calls = parsed
        .tool_calls
        .into_iter()
        .map(|t| crate::ProposedToolCall {
            id: t.id,
            name: t.name,
            tool_call: ToolCall {
                target_plugin_id: t.target_plugin_id,
                command_id: t.command_id,
                args: t.args,
            },
        })
        .collect();
    Ok(crate::Proposal {
        text: parsed.text,
        tool_calls,
    })
}

#[derive(Clone)]
struct KernelToolBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ToolDispatcher for KernelToolBridge {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String> {
        self.ctx
            .ipc_call(
                &call.target_plugin_id,
                &call.command_id,
                call.args.clone(),
                self.timeout,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

// ── Error / serde plumbing ──────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn agent_err(e: &AgentError) -> PluginError {
    exec_err(e.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize: {e}")))
}

// ── Orchestrator handlers (BL-027) ──────────────────────────────────────────

/// Args for [`HANDLER_DELEGATE`]: pick one archetype and a goal.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// Archetype short name (`"writer"`, `"coder"`, `"researcher"`).
    pub archetype: String,
    /// Natural-language goal forwarded to the archetype's planner.
    pub goal: String,
}

/// One job for [`HANDLER_PARALLEL`].
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ParallelJob {
    /// Archetype short name.
    pub archetype: String,
    /// Natural-language goal.
    pub goal: String,
}

/// Args for [`HANDLER_PARALLEL`].
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ParallelArgs {
    /// Jobs fanned out concurrently; results returned in input order.
    pub jobs: Vec<ParallelJob>,
}

/// One stage in [`HANDLER_PIPELINE`].
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PipelineStage {
    /// Archetype short name.
    pub archetype: String,
    /// Goal template — `{{prev}}` is substituted with the previous
    /// stage's textual summary before planning.
    pub goal_template: String,
}

/// Args for [`HANDLER_PIPELINE`].
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PipelineArgs {
    /// Stages run sequentially; first failure stops the pipeline and
    /// the partial observation list is returned.
    pub stages: Vec<PipelineStage>,
}

/// Response wrapper for [`HANDLER_TRACE_GET`].
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct TraceResponse {
    /// Trace entries in append order.
    pub entries: Vec<TraceEntry>,
}

async fn handle_delegate(
    orch: Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DelegateArgs = parse(args, "delegate")?;
    let obs = orch
        .delegate(&a.archetype, &a.goal, None)
        .await
        .map_err(|e| agent_err(&e))?;
    to_value(&obs, "delegate")
}

async fn handle_parallel(
    orch: Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: ParallelArgs = parse(args, "parallel")?;
    let jobs: Vec<(String, String)> = a
        .jobs
        .into_iter()
        .map(|j| (j.archetype, j.goal))
        .collect();
    let observations = orch.parallel(&jobs).await;
    to_value(&observations, "parallel")
}

async fn handle_pipeline(
    orch: Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PipelineArgs = parse(args, "pipeline")?;
    let stages: Vec<(String, String)> = a
        .stages
        .into_iter()
        .map(|s| (s.archetype, s.goal_template))
        .collect();
    let observations = orch.pipeline(&stages).await;
    to_value(&observations, "pipeline")
}

async fn handle_trace_get(
    orch: Arc<AgentOrchestrator<AiChatBridge, KernelToolBridge>>,
) -> Result<serde_json::Value, PluginError> {
    let entries = orch.trace().await;
    to_value(&TraceResponse { entries }, "trace_get")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// OI-04 — `list_archetypes` returns the short-name catalogue
    /// (`"writer"`, `"coder"`, `"researcher"`) via the sync dispatch
    /// path without needing a wired kernel context. These are the
    /// strings [`crate::archetypes::resolve_prompt`] accepts back as
    /// the `archetype` arg to `plan` / `run`, so the shell's picker
    /// can round-trip them verbatim.
    #[test]
    fn list_archetypes_returns_short_names() {
        let mut plugin = AgentCorePlugin::new();
        let v = plugin
            .dispatch(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null)
            .expect("list_archetypes dispatch");
        let names: Vec<String> = serde_json::from_value(v).expect("decode");
        assert_eq!(names, vec!["writer", "coder", "researcher"]);
    }

    /// OI-04 — `dispatch_async` returns `None` for
    /// `list_archetypes` so the kernel falls back to the sync path
    /// and avoids burning a tokio frame on a pure constant read.
    #[test]
    fn dispatch_async_yields_to_sync_for_list_archetypes() {
        let mut plugin = AgentCorePlugin::new();
        let fut = plugin.dispatch_async(HANDLER_LIST_ARCHETYPES, &serde_json::Value::Null);
        assert!(fut.is_none(), "list_archetypes must not return an async future");
    }
}
