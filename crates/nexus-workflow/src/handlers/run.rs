//! `com.nexus.workflow::run` async handler — load a workflow, evaluate
//! its optional gate condition, run the steps through the
//! `KernelActionDispatcher`, persist a run-history row, and emit
//! activity-timeline events on both ends. Lifted out of
//! `core_plugin.rs` by the BL-137 oversized-file decomposition.
//!
//! Public surface area is `prepare`, which is what `dispatch_async`
//! calls. Everything below it — `KernelActionDispatcher`, the per-step
//! parsers, the variable flattener — lives here because nothing else
//! in the crate consumes them.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePluginFuture, PluginError};

use crate::ai_steps;
use crate::core_plugin::RunWorkflowArgs;
use crate::{
    condition_skipped_run, evaluate_condition, run_workflow_with_variables, ActionDispatcher,
    EvaluationContext, Step, VariableMap, Workflow, WorkflowRegistry,
};

use super::shared::{
    exec_err, parse_args, poisoned, publish_workflow_activity, to_value, DEFAULT_STEP_TIMEOUT,
};

pub(crate) fn prepare(
    registry: &Mutex<WorkflowRegistry>,
    ctx: Option<Arc<KernelPluginContext>>,
    root: &std::path::Path,
    run_history: &Arc<crate::run_history::RunHistoryStore>,
    args: &serde_json::Value,
) -> Option<CorePluginFuture> {
    let args = args.clone();
    let workflow = match lookup_by_args(registry, &args) {
        Ok(wf) => wf,
        Err(err) => return Some(Box::pin(async move { Err(err) })),
    };
    let variables = match extract_variables(&args) {
        Ok(v) => v,
        Err(err) => return Some(Box::pin(async move { Err(err) })),
    };
    let run_history = Arc::clone(run_history);

    // Evaluate [condition] up front — gate closed means no step
    // dispatches. Errors propagate as plugin failures (if we can't
    // evaluate the gate, we can't safely open it).
    if let Some(cond) = &workflow.condition {
        let forge_root = root.parent().map(std::path::Path::to_path_buf);
        let eval_ctx = EvaluationContext {
            forge_root,
            variables: variables.clone(),
        };
        match evaluate_condition(cond, &eval_ctx) {
            Ok(false) => {
                let run = condition_skipped_run(&workflow);
                // BL-054 Phase 4 follow-up — persist condition-skipped
                // runs too so the automation tab's "last run" reflects
                // them.
                let now = chrono::Utc::now().to_rfc3339();
                run_history.append(crate::run_history::RunHistoryEntry {
                    workflow_name: workflow.workflow.name.clone(),
                    started_at: now.clone(),
                    finished_at: now,
                    success: true,
                    condition_skipped: true,
                    step_count: 0,
                    error: None,
                });
                let value = to_value(&run, "run");
                return Some(Box::pin(async move { value }));
            }
            Ok(true) => {}
            Err(e) => {
                let err = exec_err(format!("run: condition: {e}"));
                return Some(Box::pin(async move { Err(err) }));
            }
        }
    }

    Some(Box::pin(async move {
        let ctx = ctx.ok_or_else(|| {
            exec_err("workflow plugin context not wired (bootstrap incomplete)".into())
        })?;
        // BL-052 — emit activity start before dispatching steps.
        let workflow_name = workflow.workflow.name.clone();
        let started_at = chrono::Utc::now().to_rfc3339();
        publish_workflow_activity(&ctx, &workflow_name, true, None).await;
        let dispatcher = KernelActionDispatcher { ctx: Arc::clone(&ctx) };
        let result = run_workflow_with_variables(&workflow, &dispatcher, &variables).await;
        // BL-052 — emit activity end (success or failure).
        let err_msg = result.as_ref().err().map(std::string::ToString::to_string);
        publish_workflow_activity(&ctx, &workflow_name, false, err_msg.clone()).await;
        // BL-054 Phase 4 follow-up — persist a run-history row in both
        // branches. `step_count` is taken off the executor result when
        // present; on `EmptyPlan` we record zero.
        let finished_at = chrono::Utc::now().to_rfc3339();
        let (success, step_count, history_err) = match &result {
            Ok(run) => (run.success, u32::try_from(run.steps.len()).unwrap_or(u32::MAX), None),
            Err(_) => (false, 0u32, err_msg.clone()),
        };
        run_history.append(crate::run_history::RunHistoryEntry {
            workflow_name,
            started_at,
            finished_at,
            success,
            condition_skipped: false,
            step_count,
            error: history_err,
        });
        let run = result.map_err(|e| exec_err(format!("run: {e}")))?;
        to_value(&run, "run")
    }))
}

fn lookup_by_args(
    registry: &Mutex<WorkflowRegistry>,
    args: &serde_json::Value,
) -> Result<Workflow, PluginError> {
    let a: RunWorkflowArgs = parse_args(args, "run")?;
    let reg = registry.lock().map_err(poisoned)?;
    reg.get(&a.name)
        .cloned()
        .ok_or_else(|| exec_err(format!("no workflow named '{}'", a.name)))
}

/// Pull the optional `variables` object out of the run args and
/// flatten it to the dotted-path map the executor consumes.
///
/// The caller sends `variables` as a nested JSON object, typically
/// `{ "trigger": { "path": "…" }, "inputs": { "dir": "…" } }`. We
/// flatten nested objects into dotted keys (`trigger.path`,
/// `inputs.dir`) and convert scalar JSON values to TOML values so
/// [`crate::interpolate::substitute_string`] can stringify them.
/// Array values are preserved as TOML arrays and render via their
/// TOML string form.
///
/// Missing `variables` → empty map (no interpolation).
fn extract_variables(args: &serde_json::Value) -> Result<VariableMap, PluginError> {
    let Some(raw) = args.get("variables") else {
        return Ok(VariableMap::new());
    };
    let Some(obj) = raw.as_object() else {
        return Err(exec_err("run: `variables` must be an object".into()));
    };
    let mut out = VariableMap::new();
    for (k, v) in obj {
        flatten_into(k, v, &mut out);
    }
    Ok(out)
}

fn flatten_into(prefix: &str, value: &serde_json::Value, out: &mut VariableMap) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten_into(&format!("{prefix}.{k}"), v, out);
            }
        }
        other => {
            if let Some(tv) = json_to_toml(other) {
                out.insert(prefix.to_string(), tv);
            }
        }
    }
}

fn json_to_toml(v: &serde_json::Value) -> Option<toml::Value> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml::Value::Integer(i))
            } else {
                n.as_f64().map(toml::Value::Float)
            }
        }
        serde_json::Value::String(s) => Some(toml::Value::String(s.clone())),
        serde_json::Value::Array(items) => Some(toml::Value::Array(
            items.iter().filter_map(json_to_toml).collect(),
        )),
        serde_json::Value::Object(_) => {
            // Flattened above; a nested object reaching here would be
            // a leaf in an array, which we don't currently support.
            None
        }
    }
}

/// BL-056 — parsed shape of a `type = "terminal"` workflow step.
///
/// `slug` is required so every action has a saved-command profile to
/// reference (shell / cwd / env). `action` defaults to `start`. The
/// BL-133 follow-up — typed view of the `[[steps]] type = "notify"`
/// fields. Parsed once in [`ActionDispatcher::run`] before dispatch so
/// the per-channel rejection message points at the failing step rather
/// than the downstream serde error from the notifications plugin.
///
/// BL-135 reshape: `channel` becomes optional (override path) and
/// `source` is the canonical knob. A step that supplies neither
/// defaults to `source = "workflow"` so the notifications plugin's
/// router picks the channels from `notifications.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NotifyStepArgs {
    channel: Option<String>,
    source: Option<String>,
    severity: Option<String>,
    message: String,
    title: Option<String>,
}

impl NotifyStepArgs {
    /// Parse the step's `extra` table into the validated view. The
    /// channel name itself isn't matched against `Channel`'s variants
    /// here — the notifications plugin's `serde(deny_unknown_fields)`
    /// does that and returns a clear "invalid args" error if the
    /// workflow author typos the channel. Keeping the parse local-only
    /// avoids a circular dep on `nexus-notifications` from
    /// `nexus-workflow`.
    fn from_step(step: &Step) -> Result<Self, String> {
        let channel = step
            .extra
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let source = step
            .extra
            .get("source")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let severity = step
            .extra
            .get("severity")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let message = step
            .extra
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "notify step missing `message`".to_string())?
            .to_string();
        let title = step
            .extra
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(Self {
            channel,
            source,
            severity,
            message,
            title,
        })
    }
}

/// `command` field is only meaningful for `run_adhoc`; for the other
/// actions it's ignored. `working_dir` overrides the saved profile
/// when present.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalStepArgs {
    slug: String,
    action: TerminalAction,
    command: Option<String>,
    working_dir: Option<String>,
}

/// One of the four BL-056 actions. Defaults to `Start` when the
/// workflow author omits the field — the most common case is a
/// foundation-class workflow that just brings a service up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalAction {
    Start,
    Stop,
    Restart,
    RunAdhoc,
}

impl TerminalAction {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            "run_adhoc" => Ok(Self::RunAdhoc),
            other => Err(format!(
                "terminal step: unknown action '{other}'; expected start|stop|restart|run_adhoc"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::RunAdhoc => "run_adhoc",
        }
    }
}

impl TerminalStepArgs {
    /// Parse the step's `extra` table into a structured view. Returns
    /// the same string-shaped errors as the rest of the dispatcher so
    /// the executor surfaces them through `StepOutcome.error`.
    fn from_step(step: &Step) -> Result<Self, String> {
        let slug = step
            .extra
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "terminal step missing `slug`".to_string())?
            .trim()
            .to_string();
        if slug.is_empty() {
            return Err("terminal step: `slug` cannot be empty".into());
        }
        let action_str = step
            .extra
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("start");
        let action = TerminalAction::parse(action_str)?;
        let command = step
            .extra
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if matches!(action, TerminalAction::RunAdhoc) && command.as_deref().unwrap_or("").trim().is_empty() {
            return Err("terminal step: action 'run_adhoc' requires a non-empty `command`".into());
        }
        let working_dir = step
            .extra
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(Self {
            slug,
            action,
            command,
            working_dir,
        })
    }
}

/// Dispatches `step.step_type` by routing known action types through
/// kernel IPC. Unknown types fall through as informational no-ops so
/// the executor still produces a stable outcome shape.
struct KernelActionDispatcher {
    ctx: Arc<KernelPluginContext>,
}

impl KernelActionDispatcher {
    /// BL-056 — execute one parsed terminal step. Splits per
    /// [`TerminalAction`] so the test surface can pin behaviour per
    /// action without coupling to TOML parsing. Each branch returns a
    /// JSON document the executor records as the step's response.
    async fn dispatch_terminal(
        &self,
        args: &TerminalStepArgs,
    ) -> Result<serde_json::Value, String> {
        match args.action {
            TerminalAction::Start => self.terminal_start(args, None).await,
            TerminalAction::RunAdhoc => {
                let cmd = args
                    .command
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        "terminal step: action 'run_adhoc' requires a non-empty `command`"
                            .to_string()
                    })?;
                self.terminal_start(args, Some(cmd)).await
            }
            TerminalAction::Stop => self.terminal_stop(&args.slug).await,
            TerminalAction::Restart => {
                let stop = self.terminal_stop(&args.slug).await?;
                let start = self.terminal_start(args, None).await?;
                Ok(serde_json::json!({
                    "action": "restart",
                    "slug": args.slug,
                    "stop": stop,
                    "start": start,
                }))
            }
        }
    }

    /// Wrap `com.nexus.terminal::run_saved`. `command_override` is
    /// `Some` for `run_adhoc` and `None` for `start` / `restart`.
    async fn terminal_start(
        &self,
        args: &TerminalStepArgs,
        command_override: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let mut payload = serde_json::json!({ "slug": args.slug });
        if let Some(wd) = args.working_dir.clone() {
            payload["working_dir"] = serde_json::Value::String(wd);
        }
        if let Some(cmd) = command_override {
            payload["command"] = serde_json::Value::String(cmd);
        }
        let resp = self
            .ctx
            .ipc_call("com.nexus.terminal", "run_saved", payload, DEFAULT_STEP_TIMEOUT)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": args.action.as_str(),
            "slug": args.slug,
            "session": resp,
        }))
    }

    /// Find every live session whose `name == "saved:<slug>"` (the
    /// label `run_saved` writes) and close each one. Returns the count
    /// closed so a workflow run records "stop did something" vs.
    /// "stop was a no-op" without inspecting the response shape.
    async fn terminal_stop(&self, slug: &str) -> Result<serde_json::Value, String> {
        let list = self
            .ctx
            .ipc_call(
                "com.nexus.terminal",
                "list_sessions",
                serde_json::json!({}),
                DEFAULT_STEP_TIMEOUT,
            )
            .await
            .map_err(|e| e.to_string())?;
        let target_name = format!("saved:{slug}");
        let ids: Vec<String> = list
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter(|row| row.get("name").and_then(|n| n.as_str()) == Some(target_name.as_str()))
                    .filter_map(|row| {
                        row.get("id").and_then(|i| i.as_str()).map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut closed: Vec<String> = Vec::with_capacity(ids.len());
        for id in &ids {
            self.ctx
                .ipc_call(
                    "com.nexus.terminal",
                    "close_session",
                    serde_json::json!({ "id": id }),
                    DEFAULT_STEP_TIMEOUT,
                )
                .await
                .map_err(|e| e.to_string())?;
            closed.push(id.clone());
        }
        Ok(serde_json::json!({
            "action": "stop",
            "slug": slug,
            "closed_sessions": closed,
        }))
    }
}

#[async_trait]
impl ActionDispatcher for KernelActionDispatcher {
    async fn run(&self, step: &Step) -> Result<serde_json::Value, String> {
        match step.step_type.as_str() {
            // Direct IPC dispatch: requires `target` + `command`; optional `args` object.
            "ipc" | "ipc_call" => {
                let target = step
                    .extra
                    .get("target")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "ipc step missing `target`".to_string())?;
                let command = step
                    .extra
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "ipc step missing `command`".to_string())?;
                let call_args = step
                    .extra
                    .get("args")
                    .cloned()
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::default()));
                self.ctx
                    .ipc_call(target, command, call_args, DEFAULT_STEP_TIMEOUT)
                    .await
                    .map_err(|e| e.to_string())
            }
            "noop" => Ok(serde_json::json!({ "noop": true })),
            // BL-028d — AI prompt: route through `com.nexus.ai::ask`
            // and return the full RagResponse JSON. The handler's
            // `answer` field is the primary text output.
            "ai_prompt" => {
                let args = ai_steps::AiPromptArgs::from_step(step)?;
                let mut ipc_args = serde_json::json!({ "question": args.prompt });
                if let Some(limit) = args.limit {
                    ipc_args["limit"] = serde_json::Value::Number(limit.into());
                }
                if step.async_submit {
                    return submit_async_step(
                        &self.ctx,
                        step,
                        "com.nexus.ai",
                        "ask",
                        ipc_args,
                    )
                    .await;
                }
                self.ctx
                    .ipc_call("com.nexus.ai", "ask", ipc_args, DEFAULT_STEP_TIMEOUT)
                    .await
                    .map_err(|e| e.to_string())
            }
            // BL-028d — AI decision: ask the AI to pick one of a fixed
            // set of labels. Routes through `ask` with `limit = 0` so
            // we don't pull RAG context for a classifier call. Returns
            // `{ choice, raw, model }`. `choice == None` means the AI
            // response did not match any label — surfaced as Err so
            // the step's retry/`on_error` policy applies.
            "ai_decision" => {
                let args = ai_steps::AiDecisionArgs::from_step(step)?;
                let composed = ai_steps::build_decision_prompt(&args.prompt, &args.choices);
                if step.async_submit {
                    // Async ai_decision drops the choice-matching
                    // post-processing — the workflow author who opts
                    // in is responsible for parsing the returned
                    // `answer` themselves (or chaining a follow-up
                    // sync step that reads `${ThisStep.task_id}` and
                    // calls `runtime::wait_for`).
                    return submit_async_step(
                        &self.ctx,
                        step,
                        "com.nexus.ai",
                        "ask",
                        serde_json::json!({ "question": composed, "limit": 0 }),
                    )
                    .await;
                }
                let resp = self
                    .ctx
                    .ipc_call(
                        "com.nexus.ai",
                        "ask",
                        serde_json::json!({ "question": composed, "limit": 0 }),
                        DEFAULT_STEP_TIMEOUT,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let answer = resp
                    .get("answer")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                match ai_steps::pick_choice(answer, &args.choices) {
                    Some(choice) => Ok(serde_json::json!({
                        "choice": choice,
                        "raw": answer,
                        "model": resp.get("model").cloned().unwrap_or(serde_json::Value::Null),
                    })),
                    None => Err(format!(
                        "ai_decision: AI response did not match any choice: {answer:?}"
                    )),
                }
            }
            // BL-056 — terminal step: start / stop / restart / run_adhoc
            // a saved command. `slug` is required for every action and
            // identifies which `SavedCommand` provides the shell + cwd
            // + env_vars profile. `start` and `run_adhoc` spawn a fresh
            // session via `com.nexus.terminal::run_saved` (BL-055);
            // `stop` closes every live session whose name matches
            // `saved:<slug>` (the convention `run_saved` writes); and
            // `restart` is `stop` followed by `start`. `run_adhoc`
            // forwards `command` as `command` so the saved profile
            // runs an alternate command line for this workflow run.
            "terminal" => {
                let parsed = TerminalStepArgs::from_step(step)?;
                self.dispatch_terminal(&parsed).await
            }
            // BL-133 follow-up — `notify` step: route through
            // `com.nexus.notifications::send`. Lets a workflow surface
            // step / run completion through any configured channel
            // (Desktop / Discord / Telegram) without the author having
            // to spell out an `ipc` step. Fields:
            //   - `channel` (required): "desktop" | "discord" |
            //     "telegram"
            //   - `message` (required): string
            //   - `title` (optional): string
            // Unknown channel → the notifications plugin's server-side
            // serde rejects with `invalid args`, which surfaces here as
            // a workflow step error — the step's `on_error` policy then
            // decides the workflow's fate.
            "notify" => {
                let parsed = NotifyStepArgs::from_step(step)?;
                let mut ipc_args = serde_json::json!({
                    "message": parsed.message,
                });
                let obj = ipc_args.as_object_mut().expect("json object");
                if let Some(channel) = parsed.channel {
                    obj.insert("channel".into(), serde_json::Value::String(channel));
                } else {
                    // Default to source-tag routing through the BL-135
                    // router. Authors override with `source = "..."`;
                    // bare `notify` steps land under the canonical
                    // `workflow` source.
                    obj.insert(
                        "source".into(),
                        serde_json::Value::String(
                            parsed.source.unwrap_or_else(|| "workflow".to_string()),
                        ),
                    );
                }
                if let Some(s) = parsed.severity {
                    obj.insert("severity".into(), serde_json::Value::String(s));
                }
                if let Some(t) = parsed.title {
                    obj.insert("title".into(), serde_json::Value::String(t));
                }
                if step.async_submit {
                    return submit_async_step(
                        &self.ctx,
                        step,
                        "com.nexus.notifications",
                        "send",
                        ipc_args,
                    )
                    .await;
                }
                self.ctx
                    .ipc_call(
                        "com.nexus.notifications",
                        "send",
                        ipc_args,
                        DEFAULT_STEP_TIMEOUT,
                    )
                    .await
                    .map_err(|e| e.to_string())
            }
            other => {
                // Unknown action types still get a stable success so
                // workflow authors can iterate without executor churn.
                tracing::warn!(
                    step_type = other,
                    "unknown workflow action type; treating as noop"
                );
                Ok(serde_json::json!({
                    "unsupported": true,
                    "step_type": other,
                }))
            }
        }
    }
}

/// BL-134 Phase 3 — submit a workflow step to the AI runtime instead
/// of awaiting its underlying IPC inline.
///
/// Packs the target plugin + command + args into an
/// `AgentTaskKind::WorkflowAiStep`, calls
/// `com.nexus.ai.runtime::submit`, and returns the runtime's
/// `{ task_id }` reply as the step's output. The workflow's run
/// record stores `task_id` so a follow-up step can `wait_for` it.
///
/// Failures (e.g. runtime not registered, caps denied) surface as
/// step errors with `submit_async:` prefix so the on_error policy
/// path is the same as for any other IPC failure.
/// Pure builder for the `com.nexus.ai.runtime::submit` envelope a
/// workflow async step emits. Factored out so the wire shape can be
/// tested without spinning up a kernel context.
fn build_async_submit_args(
    step: &crate::Step,
    target_plugin: &str,
    command: &str,
    ipc_args: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "task": {
            "kind": "workflow_ai_step",
            "target_plugin": target_plugin,
            "command": command,
            "args": ipc_args,
            "workflow": step.name.clone().unwrap_or_default(),
            "step": 0,
        },
        "priority": "background",
    })
}

async fn submit_async_step(
    ctx: &std::sync::Arc<nexus_kernel::KernelPluginContext>,
    step: &crate::Step,
    target_plugin: &str,
    command: &str,
    ipc_args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use nexus_kernel::PluginContext;
    let submit_args = build_async_submit_args(step, target_plugin, command, ipc_args);
    ctx.ipc_call(
        "com.nexus.ai.runtime",
        "submit",
        submit_args,
        DEFAULT_STEP_TIMEOUT,
    )
    .await
    .map_err(|e| format!("submit_async ({target_plugin}::{command}): {e}"))
}

// ── BL-056 — terminal-step parsing tests ────────────────────────────
//
// The dispatcher itself needs an `Arc<KernelPluginContext>` to land
// IPC calls, which would mean spinning up the whole plugin context
// machinery for these tests. The structural parser
// (`TerminalStepArgs::from_step`) is the load-bearing piece — once
// that is right, the IPC payload shape is mechanical. We pin the
// parser exhaustively here and rely on the IPC integration covered
// by the saved-store tests in `nexus-terminal::core_plugin::tests`
// for the round-trip. The async validate path is exercised via
// `validate_async` directly with a `None` context.
#[cfg(test)]
mod terminal_step_parse_tests {
    use super::*;
    use crate::parse_workflow_text;
    use crate::Step;

    fn step_from_toml(src: &str) -> Step {
        let wf = parse_workflow_text(src).expect("parse");
        wf.steps.into_iter().next().expect("one step")
    }

    fn build_with_terminal_step(action: &str, extra: &str) -> Step {
        let src = format!(
            r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "terminal"
slug = "dev-server"
action = "{action}"
{extra}
"#
        );
        step_from_toml(&src)
    }

    #[test]
    fn parses_default_action_as_start_when_omitted() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "terminal"
slug = "dev-server"
"#;
        let step = step_from_toml(src);
        let parsed = TerminalStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.slug, "dev-server");
        assert_eq!(parsed.action, TerminalAction::Start);
        assert!(parsed.command.is_none());
        assert!(parsed.working_dir.is_none());
    }

    #[test]
    fn parses_each_known_action() {
        for (name, expected) in [
            ("start", TerminalAction::Start),
            ("stop", TerminalAction::Stop),
            ("restart", TerminalAction::Restart),
        ] {
            let step = build_with_terminal_step(name, "");
            let parsed = TerminalStepArgs::from_step(&step).unwrap();
            assert_eq!(parsed.action, expected, "for {name}");
        }
        // run_adhoc requires a non-empty command
        let step = build_with_terminal_step("run_adhoc", "command = \"npm run lint\"");
        let parsed = TerminalStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.action, TerminalAction::RunAdhoc);
        assert_eq!(parsed.command.as_deref(), Some("npm run lint"));
    }

    #[test]
    fn rejects_unknown_action() {
        let step = build_with_terminal_step("blow-up", "");
        let err = TerminalStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("unknown action 'blow-up'"), "got: {err}");
    }

    #[test]
    fn rejects_missing_slug() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "terminal"
"#;
        let step = step_from_toml(src);
        let err = TerminalStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("missing `slug`"), "got: {err}");
    }

    #[test]
    fn rejects_empty_slug() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "terminal"
slug = "   "
"#;
        let step = step_from_toml(src);
        let err = TerminalStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("cannot be empty"), "got: {err}");
    }

    #[test]
    fn rejects_run_adhoc_without_command() {
        let step = build_with_terminal_step("run_adhoc", "");
        let err = TerminalStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("requires a non-empty `command`"), "got: {err}");
    }

    #[test]
    fn rejects_run_adhoc_with_blank_command() {
        let step = build_with_terminal_step("run_adhoc", "command = \"   \"");
        let err = TerminalStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("requires a non-empty `command`"), "got: {err}");
    }

    #[test]
    fn working_dir_override_round_trips() {
        let step = build_with_terminal_step("start", "working_dir = \"/srv/app\"");
        let parsed = TerminalStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.working_dir.as_deref(), Some("/srv/app"));
    }

    #[test]
    fn terminal_action_as_str_round_trips() {
        for action in [
            TerminalAction::Start,
            TerminalAction::Stop,
            TerminalAction::Restart,
            TerminalAction::RunAdhoc,
        ] {
            let parsed = TerminalAction::parse(action.as_str()).unwrap();
            assert_eq!(parsed, action);
        }
    }
}

// ── BL-133 follow-up — workflow `notify` step parser tests ───────
//
// Same testing scope as the terminal-step parser above: pin the
// structural parser exhaustively here, rely on the
// `nexus-notifications::core_plugin::tests` IPC tests for the
// downstream serde-validation behaviour. Adding the dispatcher path
// to the integration tests would need a live `com.nexus.notifications`
// in the test runtime, which the `validate_async`-style fallback
// doesn't exercise.
#[cfg(test)]
mod notify_step_parse_tests {
    use super::*;
    use crate::parse_workflow_text;
    use crate::Step;

    fn step_from_toml(src: &str) -> Step {
        let wf = parse_workflow_text(src).expect("parse");
        wf.steps.into_iter().next().expect("one step")
    }

    fn build_notify_step(extra: &str) -> Step {
        let src = format!(
            r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "notify"
{extra}
"#
        );
        step_from_toml(&src)
    }

    #[test]
    fn parses_minimal_notify_step_with_explicit_channel() {
        let step = build_notify_step(
            r#"channel = "desktop"
message = "hello""#,
        );
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.channel.as_deref(), Some("desktop"));
        assert!(parsed.source.is_none());
        assert_eq!(parsed.message, "hello");
        assert!(parsed.title.is_none());
    }

    #[test]
    fn parses_notify_step_with_title() {
        let step = build_notify_step(
            r#"channel = "discord"
message = "deploy complete"
title = "Workflow nightly""#,
        );
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.channel.as_deref(), Some("discord"));
        assert_eq!(parsed.message, "deploy complete");
        assert_eq!(parsed.title.as_deref(), Some("Workflow nightly"));
    }

    #[test]
    fn parses_notify_step_with_source_only() {
        let step = build_notify_step(
            r#"source = "ai_runtime"
severity = "warn"
message = "task failed""#,
        );
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert!(parsed.channel.is_none());
        assert_eq!(parsed.source.as_deref(), Some("ai_runtime"));
        assert_eq!(parsed.severity.as_deref(), Some("warn"));
        assert_eq!(parsed.message, "task failed");
    }

    #[test]
    fn accepts_step_with_neither_channel_nor_source() {
        // BL-135 — the dispatcher fills in `source = "workflow"`
        // when neither field is supplied. The parser accepts this
        // shape; downstream code handles the defaulting.
        let step = build_notify_step(r#"message = "x""#);
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert!(parsed.channel.is_none());
        assert!(parsed.source.is_none());
        assert_eq!(parsed.message, "x");
    }

    #[test]
    fn rejects_missing_message() {
        let step = build_notify_step(r#"channel = "desktop""#);
        let err = NotifyStepArgs::from_step(&step).unwrap_err();
        assert!(err.contains("missing `message`"), "got: {err}");
    }

    #[test]
    fn empty_channel_string_is_dropped_to_none() {
        // Producers that emit `channel = ""` (deserialisers that
        // surface absent fields as empty strings, the legacy workflow
        // author who clears the field) are treated as if the field
        // were absent — the dispatcher then routes by source / default
        // tag.
        let step = build_notify_step(
            r#"channel = "   "
message = "x""#,
        );
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert!(parsed.channel.is_none());
    }

    /// The parser deliberately does NOT validate `channel` against
    /// `nexus_notifications::Channel`'s variants. Doing so would
    /// require a circular dep (`nexus-workflow` → `nexus-notifications`).
    /// Server-side `serde(deny_unknown_fields)` on `SendArgs.channel`
    /// rejects unknown values at dispatch time with a clear "invalid
    /// args" error. This test pins that contract — parsing accepts
    /// unknown channel names so the dispatcher's error path is the
    /// one that fires.
    #[test]
    fn accepts_unknown_channel_at_parse_time() {
        let step = build_notify_step(
            r#"channel = "carrier-pigeon"
message = "x""#,
        );
        let parsed = NotifyStepArgs::from_step(&step).unwrap();
        assert_eq!(parsed.channel.as_deref(), Some("carrier-pigeon"));
    }
}

#[cfg(test)]
mod async_step_tests {
    //! BL-134 Phase 3 — `async = true` workflow steps.

    use super::*;
    use crate::{parse_workflow_text, Step};

    fn step_from_toml(src: &str) -> Step {
        let wf = parse_workflow_text(src).expect("parse");
        wf.steps.into_iter().next().expect("one step")
    }

    #[test]
    fn step_parses_async_true_from_toml() {
        let step = step_from_toml(
            r#"
[workflow]
name = "W"

[trigger]
type = "manual"

[[steps]]
name = "AskNow"
type = "ai_prompt"
async = true
prompt = "What time is it?"
"#,
        );
        assert!(step.async_submit, "async = true must set async_submit");
    }

    #[test]
    fn step_defaults_async_to_false_when_field_omitted() {
        let step = step_from_toml(
            r#"
[workflow]
name = "W"

[trigger]
type = "manual"

[[steps]]
name = "AskInline"
type = "ai_prompt"
prompt = "What time is it?"
"#,
        );
        assert!(!step.async_submit);
    }

    #[test]
    fn build_async_submit_args_packs_workflow_ai_step_envelope() {
        let step = Step {
            name: Some("AskNow".into()),
            step_type: "ai_prompt".into(),
            parallel: false,
            async_submit: true,
            on_error: None,
            max_retries: None,
            retry_backoff: None,
            retry_initial_delay_ms: None,
            retry_max_delay_ms: None,
            retry_jitter: None,
            extra: Default::default(),
        };
        let ipc_args = serde_json::json!({ "question": "What time is it?" });
        let submit = build_async_submit_args(&step, "com.nexus.ai", "ask", ipc_args.clone());
        // Top-level shape.
        assert_eq!(
            submit.get("priority").and_then(serde_json::Value::as_str),
            Some("background"),
            "async steps default to background priority so they don't \
             preempt interactive agent runs"
        );
        // Nested task envelope.
        let task = submit.get("task").expect("task object");
        assert_eq!(
            task.get("kind").and_then(serde_json::Value::as_str),
            Some("workflow_ai_step")
        );
        assert_eq!(
            task.get("target_plugin").and_then(serde_json::Value::as_str),
            Some("com.nexus.ai")
        );
        assert_eq!(
            task.get("command").and_then(serde_json::Value::as_str),
            Some("ask")
        );
        assert_eq!(task.get("args"), Some(&ipc_args));
        assert_eq!(
            task.get("workflow").and_then(serde_json::Value::as_str),
            Some("AskNow"),
            "step name surfaces as the runtime's `workflow` field"
        );
    }

    #[test]
    fn build_async_submit_args_omits_workflow_field_for_unnamed_steps() {
        let step = Step {
            name: None,
            step_type: "notify".into(),
            parallel: false,
            async_submit: true,
            on_error: None,
            max_retries: None,
            retry_backoff: None,
            retry_initial_delay_ms: None,
            retry_max_delay_ms: None,
            retry_jitter: None,
            extra: Default::default(),
        };
        let submit = build_async_submit_args(
            &step,
            "com.nexus.notifications",
            "send",
            serde_json::json!({ "channel": "desktop", "message": "x" }),
        );
        assert_eq!(
            submit
                .get("task")
                .and_then(|t| t.get("workflow"))
                .and_then(serde_json::Value::as_str),
            Some(""),
            "unnamed step yields an empty workflow tag; runtime treats as opaque"
        );
    }
}

#[cfg(test)]
mod variable_tests {
    use super::*;

    #[test]
    fn extract_variables_flattens_nested_objects() {
        let args = serde_json::json!({
            "name": "Foo",
            "variables": {
                "trigger": { "path": "a.md", "lines": 42 },
                "inputs": { "enabled": true }
            }
        });
        let vars = extract_variables(&args).unwrap();
        assert_eq!(
            vars.get("trigger.path").and_then(|v| v.as_str()),
            Some("a.md")
        );
        assert_eq!(
            vars.get("trigger.lines").and_then(toml::Value::as_integer),
            Some(42)
        );
        assert_eq!(
            vars.get("inputs.enabled").and_then(toml::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn extract_variables_missing_returns_empty() {
        let args = serde_json::json!({ "name": "Foo" });
        let vars = extract_variables(&args).unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_variables_rejects_non_object() {
        let args = serde_json::json!({ "name": "Foo", "variables": "nope" });
        let err = extract_variables(&args).unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("must be an object"));
            }
            _ => panic!("unexpected"),
        }
    }
}
