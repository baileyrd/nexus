//! Core plugin wrapping [`WorkflowRegistry`].
//!
//! Exposes the registry over kernel IPC so CLI / UI / future trigger
//! engine can consult the list of declared workflows without linking
//! `nexus-workflow` directly. Plugin is *load-only* — mutations
//! happen by editing `.workflow.toml` files and calling `reload`.
//!
//! # Handlers
//!
//! | Id | Command    | Args             | Purpose                              |
//! |---:|------------|------------------|--------------------------------------|
//! | 1  | `list`     | `{}`             | Every loaded workflow (metadata + triggers) |
//! | 2  | `get`      | `{ name }`       | One workflow by name (404 if missing) |
//! | 3  | `reload`   | `{}`             | Re-scan `<forge>/.workflows`          |
//! | 4  | `validate` | `{ text }`       | Parse a TOML string; return validated JSON |
//! | 5  | `run`      | `{ name, variables? }` | Execute a loaded workflow; `variables` is an optional nested object flattened to dotted keys (`{"trigger": {"path": "a.md"}}` → `trigger.path`) for `${…}` interpolation in step fields. |
//!
//! Ids are append-only.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Deserialize;

use crate::{
    cron::CronSchedule, parse_workflow_text, run_workflow_with_variables, ActionDispatcher, Step,
    VariableMap, Workflow, WorkflowRegistry, WorkflowRegistryError,
};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.workflow";

/// `list` handler id.
pub const HANDLER_LIST: u32 = 1;
/// `get` handler id.
pub const HANDLER_GET: u32 = 2;
/// `reload` handler id.
pub const HANDLER_RELOAD: u32 = 3;
/// `validate` handler id.
pub const HANDLER_VALIDATE: u32 = 4;
/// `run` handler id.
pub const HANDLER_RUN: u32 = 5;

/// Default per-step tool-call timeout. Workflow steps often span
/// multiple plugins; give them enough headroom.
const DEFAULT_STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Core plugin — holds the workflow root path + an in-memory registry
/// behind a mutex so dispatches stay `Send + Sync`.
pub struct WorkflowCorePlugin {
    root: PathBuf,
    registry: Mutex<WorkflowRegistry>,
    context: Option<Arc<KernelPluginContext>>,
    /// Spawned cron-trigger tasks, one per cron-scheduled workflow.
    /// Cancelled on plugin drop so the scheduler doesn't outlive the
    /// process.
    scheduler_handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl WorkflowCorePlugin {
    /// Construct with the forge's `.workflows/` directory. Eagerly
    /// loads the registry; partial parse failures are logged at
    /// `warn` and the registry starts with whatever parsed cleanly.
    #[must_use]
    pub fn open(workflows_dir: PathBuf) -> Self {
        let registry = match WorkflowRegistry::load(&workflows_dir) {
            Ok(reg) => reg,
            Err(WorkflowRegistryError::PartialParseFailure { count, first }) => {
                tracing::warn!(
                    path = %workflows_dir.display(),
                    count,
                    first = %first,
                    "com.nexus.workflow: {count} workflow file(s) failed to parse during load"
                );
                // Re-run with the known-good subset by discarding
                // failures — load inserts good entries before it
                // returns the error, but the error path drops the
                // registry. A clean reload rebuilds from scratch.
                WorkflowRegistry::load(&workflows_dir).unwrap_or_else(|_| WorkflowRegistry::empty())
            }
            Err(err) => {
                tracing::warn!(
                    path = %workflows_dir.display(),
                    err = %err,
                    "com.nexus.workflow: load failed; registry starts empty"
                );
                WorkflowRegistry::empty()
            }
        };
        Self {
            root: workflows_dir,
            registry: Mutex::new(registry),
            context: None,
            scheduler_handles: Mutex::new(Vec::new()),
        }
    }

    /// Spawn one tokio task per cron-triggered workflow. Each task
    /// parses the `[trigger].schedule` expression, sleeps until the
    /// next fire time, dispatches `run_workflow`, and loops. Parse
    /// failures log-and-skip; executor failures log-and-continue so
    /// one broken workflow doesn't stall the rest.
    ///
    /// Called from `wire_context` once the kernel context is
    /// available. Handles are retained so the plugin can cancel them
    /// on drop.
    fn spawn_cron_schedulers(&self, ctx: Arc<KernelPluginContext>) {
        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => {
                tracing::warn!(
                    "workflow scheduler: no tokio runtime available; cron triggers disabled"
                );
                return;
            }
        };
        let workflows: Vec<(String, String)> = match self.registry.lock() {
            Ok(reg) => reg
                .iter()
                .filter_map(|(name, wf)| {
                    if wf.trigger.trigger_type != "cron" {
                        return None;
                    }
                    let schedule = wf
                        .trigger
                        .extra
                        .get("schedule")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)?;
                    Some((name.to_string(), schedule))
                })
                .collect(),
            Err(_) => return,
        };
        let mut handles = match self.scheduler_handles.lock() {
            Ok(h) => h,
            Err(_) => return,
        };
        for (name, expr) in workflows {
            let schedule = match CronSchedule::parse(&expr) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(workflow = %name, expr = %expr, error = %e, "cron parse failed; scheduler skipping this workflow");
                    continue;
                }
            };
            let ctx = Arc::clone(&ctx);
            let wf_name = name.clone();
            tracing::info!(workflow = %wf_name, expr = %expr, "cron scheduler armed");
            let handle = runtime.spawn(async move {
                scheduler_loop(ctx, wf_name, schedule).await;
            });
            handles.push(handle);
        }
    }
}

impl Drop for WorkflowCorePlugin {
    fn drop(&mut self) {
        if let Ok(handles) = self.scheduler_handles.lock() {
            for h in handles.iter() {
                h.abort();
            }
        }
    }
}

async fn scheduler_loop(
    ctx: Arc<KernelPluginContext>,
    workflow_name: String,
    schedule: CronSchedule,
) {
    loop {
        let now = chrono::Utc::now();
        let Some(next) = schedule.next_after(now) else {
            tracing::warn!(workflow = %workflow_name, "cron schedule has no future fire time; parking forever");
            // Park on a very long sleep — the task stays alive so
            // drop-abort works, but does nothing.
            tokio::time::sleep(std::time::Duration::from_secs(86_400 * 365)).await;
            continue;
        };
        let wait = (next - now).to_std().unwrap_or(std::time::Duration::ZERO);
        tracing::debug!(workflow = %workflow_name, next = %next, "cron sleep");
        tokio::time::sleep(wait).await;
        // Dispatch through the plugin's own run handler so semantics
        // match the CLI / UI code paths (history persistence,
        // streaming events, etc. all flow through one spot).
        let call = ctx
            .ipc_call(
                PLUGIN_ID,
                "run",
                serde_json::json!({ "name": workflow_name }),
                std::time::Duration::from_secs(600),
            )
            .await;
        match call {
            Ok(_) => tracing::info!(workflow = %workflow_name, "cron fired"),
            Err(err) => tracing::warn!(workflow = %workflow_name, %err, "cron run failed; scheduler continues"),
        }
    }
}

impl CorePlugin for WorkflowCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST => self.dispatch_list(),
            HANDLER_GET => self.dispatch_get(args),
            HANDLER_RELOAD => self.dispatch_reload(),
            HANDLER_VALIDATE => Self::dispatch_validate(args),
            HANDLER_RUN => Err(exec_err(
                format!(
                    "handler {HANDLER_RUN}: run is async; caller should use dispatch_async"
                ),
            )),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        if handler_id != HANDLER_RUN {
            return None;
        }
        let ctx = self.context.clone();
        let args = args.clone();
        let workflow = match lookup_by_args(&self.registry, &args) {
            Ok(wf) => wf,
            Err(err) => return Some(Box::pin(async move { Err(err) })),
        };
        let variables = match extract_variables(&args) {
            Ok(v) => v,
            Err(err) => return Some(Box::pin(async move { Err(err) })),
        };
        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err(
                    "workflow plugin context not wired (bootstrap incomplete)".into(),
                )
            })?;
            let dispatcher = KernelActionDispatcher { ctx };
            let run = run_workflow_with_variables(&workflow, &dispatcher, &variables)
                .await
                .map_err(|e| exec_err(format!("run: {e}")))?;
            to_value(&run, "run")
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(Arc::clone(&ctx));
        self.spawn_cron_schedulers(ctx);
    }
}

fn lookup_by_args(
    registry: &Mutex<WorkflowRegistry>,
    args: &serde_json::Value,
) -> Result<Workflow, PluginError> {
    #[derive(Deserialize)]
    struct Args {
        name: String,
    }
    let a: Args = parse(args, "run")?;
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

/// Dispatches `step.step_type` by routing known action types through
/// kernel IPC. Unknown types fall through as informational no-ops so
/// the executor still produces a stable outcome shape.
struct KernelActionDispatcher {
    ctx: Arc<KernelPluginContext>,
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
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                self.ctx
                    .ipc_call(target, command, call_args, DEFAULT_STEP_TIMEOUT)
                    .await
                    .map_err(|e| e.to_string())
            }
            "noop" => Ok(serde_json::json!({ "noop": true })),
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

impl WorkflowCorePlugin {
    fn dispatch_list(&self) -> Result<serde_json::Value, PluginError> {
        let reg = self.registry.lock().map_err(poisoned)?;
        let workflows: Vec<_> = reg.iter().map(|(_, w)| w.clone()).collect();
        to_value(&workflows, "list")
    }

    fn dispatch_get(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
        }
        let a: Args = parse(args, "get")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        match reg.get(&a.name) {
            Some(w) => to_value(w, "get"),
            None => Err(exec_err(format!("no workflow named '{}'", a.name))),
        }
    }

    fn dispatch_reload(&self) -> Result<serde_json::Value, PluginError> {
        let reloaded = WorkflowRegistry::load(&self.root)
            .unwrap_or_else(|_| WorkflowRegistry::empty());
        let len = reloaded.len();
        *self.registry.lock().map_err(poisoned)? = reloaded;
        Ok(serde_json::json!({ "loaded": len }))
    }

    fn dispatch_validate(
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            text: String,
        }
        let a: Args = parse(args, "validate")?;
        match parse_workflow_text(&a.text) {
            Ok(w) => to_value(&w, "validate"),
            Err(err) => Err(exec_err(format!("invalid workflow: {err}"))),
        }
    }
}

// ── Error / serde plumbing ──────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("workflow registry mutex poisoned — prior handler panicked".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const WF: &str = r#"
[workflow]
name = "Daily"
description = "daily journal"

[trigger]
type = "cron"
schedule = "0 9 * * *"

[[steps]]
type = "file_create"
path = "journal/today.md"
"#;

    fn write(dir: &std::path::Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn list_round_trips_through_dispatch() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "daily.workflow.toml", WF);
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin.dispatch(HANDLER_LIST, &serde_json::json!({})).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["workflow"]["name"], "Daily");
        assert_eq!(arr[0]["trigger"]["type"], "cron");
    }

    #[test]
    fn get_returns_error_for_unknown_name() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "name": "nope" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("no workflow"));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn reload_picks_up_new_files() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        assert_eq!(
            plugin
                .dispatch(HANDLER_LIST, &serde_json::json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        write(tmp.path(), "daily.workflow.toml", WF);
        let v = plugin.dispatch(HANDLER_RELOAD, &serde_json::json!({})).unwrap();
        assert_eq!(v["loaded"], 1);
    }

    #[test]
    fn validate_accepts_good_toml() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(HANDLER_VALIDATE, &serde_json::json!({ "text": WF }))
            .unwrap();
        assert_eq!(v["workflow"]["name"], "Daily");
    }

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
            vars.get("trigger.lines").and_then(|v| v.as_integer()),
            Some(42)
        );
        assert_eq!(
            vars.get("inputs.enabled").and_then(|v| v.as_bool()),
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

    #[test]
    fn validate_rejects_bad_toml() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_VALIDATE, &serde_json::json!({ "text": "not-toml {{" }))
            .unwrap_err();
        assert!(matches!(err, PluginError::ExecutionFailed { .. }));
    }
}
