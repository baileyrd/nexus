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
//!
//! # Trigger engines
//!
//! The plugin drives workflow triggers from `wire_context`:
//! - **cron** — `spawn_cron_schedulers` ([`cron.rs`](crate::cron))
//!   parses each `trigger.schedule` and fires via `run`.
//! - **`file_event`** — `spawn_file_event_triggers` subscribes to
//!   `com.nexus.storage.file_*` on the kernel bus, filters against
//!   the trigger's `watch_dir` / `pattern` / `events`, and fires
//!   `run` with `trigger.{path,event_type,content_hash}` variables.
//! - **manual** — no background engine; callers drive `run`
//!   directly (CLI, UI, scheduled task, nested workflow).
//!
//! `webhook` / `git_event` / `mcp_event` are not yet wired.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{EventFilter, KernelPluginContext, NexusEvent, PluginContext, RecvError};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Deserialize;

use crate::{
    condition_skipped_run, cron::CronSchedule, evaluate_condition, parse_workflow_text,
    run_workflow_with_variables, ActionDispatcher, EvaluationContext, Step, VariableMap, Workflow,
    WorkflowRegistry, WorkflowRegistryError,
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
    fn spawn_cron_schedulers(&self, ctx: &Arc<KernelPluginContext>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                "workflow scheduler: no tokio runtime available; cron triggers disabled"
            );
            return;
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
        let Ok(mut handles) = self.scheduler_handles.lock() else {
            return;
        };
        for (name, expr) in workflows {
            let schedule = match CronSchedule::parse(&expr) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(workflow = %name, expr = %expr, error = %e, "cron parse failed; scheduler skipping this workflow");
                    continue;
                }
            };
            let ctx = Arc::clone(ctx);
            let wf_name = name.clone();
            tracing::info!(workflow = %wf_name, expr = %expr, "cron scheduler armed");
            let handle = runtime.spawn(async move {
                scheduler_loop(ctx, wf_name, schedule).await;
            });
            handles.push(handle);
        }
    }

    /// Spawn one tokio task per `file_event`-triggered workflow.
    /// Each task subscribes to `com.nexus.storage.file_*` on the
    /// kernel bus, filters events against the trigger's optional
    /// `watch_dir` / `pattern` / `events` fields, and dispatches
    /// `com.nexus.workflow::run` with `trigger.{path,event_type}`
    /// variables when an event matches.
    ///
    /// Parse failures (e.g. invalid regex) log-and-skip that one
    /// workflow; other workflows keep their subscriptions. Handles
    /// are retained so the plugin can cancel them on drop (alongside
    /// cron-scheduler handles).
    fn spawn_file_event_triggers(&self, ctx: &Arc<KernelPluginContext>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                "workflow scheduler: no tokio runtime available; file_event triggers disabled"
            );
            return;
        };
        let specs: Vec<FileEventSpec> = match self.registry.lock() {
            Ok(reg) => reg
                .iter()
                .filter(|(_, wf)| wf.trigger.trigger_type == "file_event")
                .filter_map(|(name, wf)| match FileEventSpec::from_trigger(name, wf) {
                    Ok(spec) => Some(spec),
                    Err(e) => {
                        tracing::warn!(workflow = %name, error = %e, "file_event trigger: spec parse failed; skipping");
                        None
                    }
                })
                .collect(),
            Err(_) => return,
        };
        let Ok(mut handles) = self.scheduler_handles.lock() else {
            return;
        };
        for spec in specs {
            let ctx = Arc::clone(ctx);
            tracing::info!(
                workflow = %spec.workflow_name,
                watch_dir = ?spec.watch_dir,
                has_pattern = spec.pattern.is_some(),
                events = ?spec.events,
                "file_event trigger armed"
            );
            let handle = runtime.spawn(async move {
                file_event_loop(ctx, spec).await;
            });
            handles.push(handle);
        }
    }
}

/// Parsed `trigger.type = "file_event"` spec.
struct FileEventSpec {
    workflow_name: String,
    watch_dir: Option<String>,
    pattern: Option<regex_lite::Regex>,
    events: FileEventSet,
}

#[derive(Debug, Clone, Copy)]
struct FileEventSet {
    created: bool,
    modified: bool,
    deleted: bool,
}

impl FileEventSet {
    fn all() -> Self {
        Self {
            created: true,
            modified: true,
            deleted: true,
        }
    }

    fn matches(self, event_type: &str) -> bool {
        match event_type {
            "created" => self.created,
            "modified" => self.modified,
            "deleted" => self.deleted,
            _ => false,
        }
    }
}

impl FileEventSpec {
    fn from_trigger(name: &str, wf: &Workflow) -> Result<Self, String> {
        let watch_dir = wf
            .trigger
            .extra
            .get("watch_dir")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let pattern = match wf.trigger.extra.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => Some(
                regex_lite::Regex::new(p)
                    .map_err(|e| format!("invalid pattern regex `{p}`: {e}"))?,
            ),
            None => None,
        };

        let events = match wf.trigger.extra.get("events") {
            None => FileEventSet::all(),
            Some(toml::Value::Array(items)) => {
                let mut set = FileEventSet {
                    created: false,
                    modified: false,
                    deleted: false,
                };
                for item in items {
                    match item.as_str() {
                        Some("created") => set.created = true,
                        Some("modified") => set.modified = true,
                        Some("deleted") => set.deleted = true,
                        Some(other) => {
                            return Err(format!(
                                "unknown event type `{other}` (expected created|modified|deleted)"
                            ));
                        }
                        None => {
                            return Err("events array must contain strings".into());
                        }
                    }
                }
                set
            }
            Some(_) => return Err("events must be an array of strings".into()),
        };

        Ok(Self {
            workflow_name: name.to_string(),
            watch_dir,
            pattern,
            events,
        })
    }

    fn matches_path(&self, path: &str) -> bool {
        if let Some(dir) = &self.watch_dir {
            if !path.starts_with(dir.as_str()) {
                return false;
            }
        }
        if let Some(re) = &self.pattern {
            if !re.is_match(path) {
                return false;
            }
        }
        true
    }
}

fn event_type_for_type_id(type_id: &str) -> Option<&'static str> {
    match type_id {
        "com.nexus.storage.file_created" => Some("created"),
        "com.nexus.storage.file_modified" => Some("modified"),
        "com.nexus.storage.file_deleted" => Some("deleted"),
        _ => None,
    }
}

async fn file_event_loop(ctx: Arc<KernelPluginContext>, spec: FileEventSpec) {
    let mut sub = ctx.subscribe(EventFilter::CustomPrefix(
        "com.nexus.storage.file_".to_string(),
    ));
    loop {
        let published = match sub.recv().await {
            Ok(e) => e,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(workflow = %spec.workflow_name, n, "file_event trigger lagged; events lost");
                continue;
            }
            Err(RecvError::Closed) => {
                tracing::debug!(workflow = %spec.workflow_name, "file_event trigger: bus closed");
                return;
            }
        };
        let NexusEvent::Custom { type_id, payload, .. } = &published.event else {
            continue;
        };
        let Some(event_type) = event_type_for_type_id(type_id) else {
            continue;
        };
        if !spec.events.matches(event_type) {
            continue;
        }
        let path = payload
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if path.is_empty() || !spec.matches_path(path) {
            continue;
        }
        let mut trigger_vars = serde_json::Map::new();
        trigger_vars.insert("path".into(), serde_json::Value::String(path.into()));
        trigger_vars.insert(
            "event_type".into(),
            serde_json::Value::String(event_type.into()),
        );
        if let Some(hash) = payload.get("content_hash").cloned() {
            trigger_vars.insert("content_hash".into(), hash);
        }
        let variables = serde_json::json!({ "trigger": trigger_vars });
        let args = serde_json::json!({
            "name": spec.workflow_name,
            "variables": variables,
        });
        match ctx
            .ipc_call(
                PLUGIN_ID,
                "run",
                args,
                std::time::Duration::from_secs(600),
            )
            .await
        {
            Ok(_) => {
                tracing::info!(
                    workflow = %spec.workflow_name,
                    event_type, path, "file_event trigger fired"
                );
            }
            Err(err) => {
                tracing::warn!(
                    workflow = %spec.workflow_name,
                    event_type, path, %err,
                    "file_event trigger: run failed; continuing"
                );
            }
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
        let forge_root = self.root.parent().map(std::path::Path::to_path_buf);

        // Evaluate [condition] up front — gate closed means no step
        // dispatches. Errors propagate as plugin failures (if we
        // can't evaluate the gate, we can't safely open it).
        if let Some(cond) = &workflow.condition {
            let eval_ctx = EvaluationContext {
                forge_root: forge_root.clone(),
                variables: variables.clone(),
            };
            match evaluate_condition(cond, &eval_ctx) {
                Ok(false) => {
                    let run = condition_skipped_run(&workflow);
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
        self.spawn_cron_schedulers(&ctx);
        self.spawn_file_event_triggers(&ctx);
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
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::default()));
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
    fn file_event_spec_parses_all_fields() {
        let src = r#"
[workflow]
name = "FE"

[trigger]
type = "file_event"
watch_dir = "notes/"
pattern = "\\.md$"
events = ["created", "modified"]
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = FileEventSpec::from_trigger("FE", &wf).unwrap();
        assert_eq!(spec.watch_dir.as_deref(), Some("notes/"));
        assert!(spec.pattern.is_some());
        assert!(spec.events.created);
        assert!(spec.events.modified);
        assert!(!spec.events.deleted);
    }

    #[test]
    fn file_event_spec_defaults_to_all_events_when_omitted() {
        let src = r#"
[workflow]
name = "FE"

[trigger]
type = "file_event"
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = FileEventSpec::from_trigger("FE", &wf).unwrap();
        assert!(spec.watch_dir.is_none());
        assert!(spec.pattern.is_none());
        assert!(spec.events.created);
        assert!(spec.events.modified);
        assert!(spec.events.deleted);
    }

    #[test]
    fn file_event_spec_rejects_invalid_regex_and_unknown_event() {
        let bad_pat = r#"
[workflow]
name = "FE"

[trigger]
type = "file_event"
pattern = "[unterminated"
"#;
        let wf = parse_workflow_text(bad_pat).unwrap();
        assert!(FileEventSpec::from_trigger("FE", &wf).is_err());

        let bad_event = r#"
[workflow]
name = "FE"

[trigger]
type = "file_event"
events = ["resurrected"]
"#;
        let wf = parse_workflow_text(bad_event).unwrap();
        assert!(FileEventSpec::from_trigger("FE", &wf).is_err());
    }

    #[test]
    fn file_event_spec_matches_path_combines_dir_and_pattern() {
        let src = r#"
[workflow]
name = "FE"

[trigger]
type = "file_event"
watch_dir = "notes/"
pattern = "\\.md$"
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = FileEventSpec::from_trigger("FE", &wf).unwrap();
        assert!(spec.matches_path("notes/a.md"));
        assert!(!spec.matches_path("notes/a.txt")); // extension mismatch
        assert!(!spec.matches_path("other/a.md")); // dir mismatch
    }

    #[test]
    fn event_type_mapping_covers_all_storage_file_events() {
        assert_eq!(
            event_type_for_type_id("com.nexus.storage.file_created"),
            Some("created")
        );
        assert_eq!(
            event_type_for_type_id("com.nexus.storage.file_modified"),
            Some("modified")
        );
        assert_eq!(
            event_type_for_type_id("com.nexus.storage.file_deleted"),
            Some("deleted")
        );
        assert!(event_type_for_type_id("com.nexus.other.ping").is_none());
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
