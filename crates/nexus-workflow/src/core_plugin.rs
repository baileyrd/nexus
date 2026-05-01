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
//! - **`git_event`** — `spawn_git_event_triggers` subscribes to
//!   `com.nexus.git.*` on the kernel bus, filters against the
//!   trigger's `events` / `branch` / `branch_pattern` fields, and
//!   fires `run` with `trigger.{event_type,branch,head,...}`
//!   variables.
//! - **`mcp_event`** — `spawn_mcp_event_triggers` subscribes to
//!   `com.nexus.mcp.*` on the kernel bus, filters against the
//!   trigger's `events` field (subset of currently-known topics),
//!   and fires `run` with `trigger.{event_type,...}` plus any payload
//!   fields carried on the event. Available topics today:
//!   `host_started` (one-shot snapshot at plugin boot — opt in by
//!   listing it in `events`). More land here when `nexus-mcp` grows
//!   them; the trigger needs no executor changes.
//! - **manual** — no background engine; callers drive `run`
//!   directly (CLI, UI, scheduled task, nested workflow).
//!
//! `webhook` is not yet wired.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{EventFilter, KernelPluginContext, NexusEvent, PluginContext, RecvError};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{
    ai_steps, condition_skipped_run, cron::CronSchedule, digests, evaluate_condition,
    parse_workflow_text, run_workflow_with_variables, templates, webhook, ActionDispatcher,
    DigestConfig, DigestKind, EvaluationContext, Step, VariableMap, Workflow, WorkflowRegistry,
    WorkflowRegistryError,
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
/// `run_digest` handler id (BL-047).
pub const HANDLER_RUN_DIGEST: u32 = 6;
/// FU-7 — `set_digest_config` handler id. Replaces the in-memory
/// [`DigestConfig`] under the shared lock so the scheduler loop
/// picks up enabled / cron / output-dir changes without a restart.
/// Args: a [`DigestConfig`] JSON object.
pub const HANDLER_SET_DIGEST_CONFIG: u32 = 7;
/// BL-028f — `templates_list`: enumerate built-in templates.
/// Args: `{}`. Returns `[{ slug, description, tags, filename }]`.
pub const HANDLER_TEMPLATES_LIST: u32 = 8;
/// BL-028f — `templates_get`: fetch one template's TOML body.
/// Args: `{ slug }`. Returns `{ slug, description, tags, filename, body }`.
pub const HANDLER_TEMPLATES_GET: u32 = 9;
/// BL-028f — `templates_init`: write a template into the forge's
/// `.workflows/` directory so it's loaded on the next reload.
/// Args: `{ slug, filename?, overwrite? }`. Returns
/// `{ written: true, path }`. Refuses to clobber an existing file
/// unless `overwrite = true`.
pub const HANDLER_TEMPLATES_INIT: u32 = 10;

// ── IPC arg types (audit P1-3 #113 — lifted from inline) ─────────────────────

/// Args for `com.nexus.workflow::run` (handler id `5`). Lifted from
/// an inline `struct Args` inside [`lookup_by_args`] by audit-2026-05-01
/// P1-3 (#113) so the schema generator can see the shape.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct RunWorkflowArgs {
    /// Workflow name (matches `[workflow].name`).
    pub name: String,
    /// Optional flattened-variables map consumed by `extract_variables`
    /// off the raw JSON; declared here so strict deserialization
    /// accepts callers that pass it.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    pub variables: Option<serde_json::Value>,
}

/// Args for `com.nexus.workflow::get` (handler id `2`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GetWorkflowArgs {
    /// Workflow name to fetch.
    pub name: String,
}

/// Args for `com.nexus.workflow::templates_get` (handler id `9`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GetTemplateArgs {
    /// Template slug (e.g. `daily-digest`, `pr-checklist`).
    pub slug: String,
}

/// Args for `com.nexus.workflow::templates_init` (handler id `10`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InitTemplateArgs {
    /// Template slug to instantiate.
    pub slug: String,
    /// Override filename (default: `<slug>.workflow.toml`).
    #[serde(default)]
    pub filename: Option<String>,
    /// Allow overwriting an existing file. Default `false` —
    /// callers must opt in to clobber.
    #[serde(default)]
    pub overwrite: bool,
}

/// Args for `com.nexus.workflow::validate` (handler id `4`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ValidateWorkflowArgs {
    /// Raw `.workflow.toml` source text.
    pub text: String,
}

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
    /// BL-047 digest configuration. Loaded from
    /// `<forge>/.forge/config.toml` `[digests]` table when present;
    /// falls back to [`DigestConfig::default`] (disabled) otherwise.
    /// FU-7 — wrapped in `Arc<RwLock<>>` so `set_digest_config`
    /// pushes are visible to the long-running scheduler loop without
    /// a restart.
    digest_config: Arc<RwLock<DigestConfig>>,
    /// BL-028g — webhook listener configuration. The accept loop
    /// only spawns when `enabled = true` and at least one workflow
    /// declares a `webhook` trigger.
    webhook_config: webhook::WebhookConfig,
}

impl WorkflowCorePlugin {
    /// Construct with the forge's `.workflows/` directory. Eagerly
    /// loads the registry; partial parse failures are logged at
    /// `warn` and the registry starts with whatever parsed cleanly.
    #[must_use]
    pub fn open(workflows_dir: PathBuf) -> Self {
        Self::open_with_digest_config(workflows_dir, DigestConfig::default())
    }

    /// Like [`open`](Self::open) but with a caller-supplied
    /// [`DigestConfig`]. Bootstrap loads the config from
    /// `<forge>/.forge/config.toml` and passes it here.
    #[must_use]
    pub fn open_with_digest_config(
        workflows_dir: PathBuf,
        digest_config: DigestConfig,
    ) -> Self {
        Self::open_full(workflows_dir, digest_config, webhook::WebhookConfig::default())
    }

    /// Construct with both the digest and webhook config blocks set.
    /// BL-028g — bootstrap calls this so the webhook listener picks
    /// up `[webhooks].enabled` / `[webhooks].bind` from
    /// `<forge>/.forge/config.toml` without further plumbing.
    #[must_use]
    pub fn open_full(
        workflows_dir: PathBuf,
        digest_config: DigestConfig,
        webhook_config: webhook::WebhookConfig,
    ) -> Self {
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
            digest_config: Arc::new(RwLock::new(digest_config)),
            webhook_config,
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

impl WorkflowCorePlugin {
    /// Spawn the BL-047 digest scheduler. Wakes every 60s, computes
    /// the next fire across daily / weekly schedules, and dispatches
    /// `run_digest` via the plugin's own IPC handler when it falls
    /// due. Disabled when [`DigestConfig::enabled`] is `false`.
    ///
    /// The task is held in `scheduler_handles` alongside the cron and
    /// file-event triggers so plugin drop aborts everything together.
    fn spawn_digest_scheduler(&self, ctx: &Arc<KernelPluginContext>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("digest scheduler: no tokio runtime available; disabled");
            return;
        };
        let cfg_handle = Arc::clone(&self.digest_config);
        let ctx = Arc::clone(ctx);
        let forge_root = self.root.parent().map(std::path::Path::to_path_buf);
        let Ok(mut handles) = self.scheduler_handles.lock() else {
            return;
        };
        // FU-7 — always spawn the loop, even when initially disabled,
        // so a later `set_digest_config` toggle takes effect without
        // restarting the plugin. The loop short-circuits each tick
        // when the live config still says disabled.
        let initial = cfg_handle.read().ok().map(|g| g.clone()).unwrap_or_default();
        tracing::info!(
            enabled = initial.enabled,
            daily = ?initial.daily_cron,
            weekly = ?initial.weekly_cron,
            "digest scheduler armed"
        );
        let handle = runtime.spawn(async move {
            digest_scheduler_loop(ctx, cfg_handle, forge_root).await;
        });
        handles.push(handle);
    }
}

async fn digest_scheduler_loop(
    ctx: Arc<KernelPluginContext>,
    cfg_handle: Arc<RwLock<DigestConfig>>,
    forge_root: Option<std::path::PathBuf>,
) {
    use std::time::Duration;
    loop {
        let cfg = cfg_handle.read().ok().map(|g| g.clone()).unwrap_or_default();
        if !cfg.enabled {
            // Park briefly so a `set_digest_config` toggle is picked up
            // within ~60s of being pushed.
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let now = chrono::Utc::now();
        let Some((kind, next)) = digests::next_fire(&cfg, now) else {
            tracing::warn!("digest scheduler: no schedules; parking");
            tokio::time::sleep(Duration::from_secs(86_400)).await;
            continue;
        };
        let wait = (next - now).to_std().unwrap_or(Duration::ZERO);
        // Cap to 60s so we re-evaluate (config may change, clock skew).
        let nap = wait.min(Duration::from_secs(60));
        tokio::time::sleep(nap).await;
        if chrono::Utc::now() < next {
            continue;
        }
        let kind_str = match kind {
            DigestKind::Daily => "daily",
            DigestKind::Weekly => "weekly",
        };
        // FU-6 — suppression watermark. A backwards clock jump (NTP
        // correction, suspend/resume) could otherwise re-fire the
        // same minute boundary. Skip when a recent fire is recorded
        // for this kind.
        let now = chrono::Utc::now();
        if let Some(root) = forge_root.as_deref() {
            let last = digests::read_last_fired(root);
            if digests::within_suppression_window(&last, kind, now) {
                tracing::debug!(
                    ?kind,
                    "digest scheduler: within suppression window; skipping"
                );
                tokio::time::sleep(Duration::from_secs(61)).await;
                continue;
            }
        }
        let call = ctx
            .ipc_call(
                PLUGIN_ID,
                "run_digest",
                serde_json::json!({ "kind": kind_str }),
                Duration::from_secs(600),
            )
            .await;
        match call {
            Ok(_) => {
                tracing::info!(?kind, "digest scheduler fired");
                if let Some(root) = forge_root.as_deref() {
                    let mut last = digests::read_last_fired(root);
                    last.set(kind, now);
                    digests::write_last_fired(root, &last);
                }
            }
            Err(err) => {
                tracing::warn!(?kind, %err, "digest scheduler: run failed; continuing");
            }
        }
        // Sleep a minute past the fire time so we don't re-fire the
        // same minute boundary repeatedly.
        tokio::time::sleep(Duration::from_secs(61)).await;
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

/// Parsed `trigger.type = "git_event"` spec.
///
/// The default event set is `["commit", "branch_changed",
/// "dirty_changed"]` — the `state` topic is **excluded** by default
/// because `nexus-git` publishes it once on plugin start as a
/// snapshot (not a delta), and most workflows want to react to
/// changes only. Opt in by listing `"state"` in `events`.
struct GitEventSpec {
    workflow_name: String,
    events: GitEventSet,
    branch: Option<String>,
    branch_pattern: Option<regex_lite::Regex>,
}

#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct GitEventSet {
    state: bool,
    commit: bool,
    branch_changed: bool,
    dirty_changed: bool,
}

impl GitEventSet {
    #[cfg(test)]
    fn all() -> Self {
        Self {
            state: true,
            commit: true,
            branch_changed: true,
            dirty_changed: true,
        }
    }

    /// Default set: every delta topic, but **not** `state`. See
    /// [`GitEventSpec`] for rationale.
    fn defaults() -> Self {
        Self {
            state: false,
            commit: true,
            branch_changed: true,
            dirty_changed: true,
        }
    }

    fn matches(self, event_type: &str) -> bool {
        match event_type {
            "state" => self.state,
            "commit" => self.commit,
            "branch_changed" => self.branch_changed,
            "dirty_changed" => self.dirty_changed,
            _ => false,
        }
    }
}

impl GitEventSpec {
    fn from_trigger(name: &str, wf: &Workflow) -> Result<Self, String> {
        let events = match wf.trigger.extra.get("events") {
            None => GitEventSet::defaults(),
            Some(toml::Value::Array(items)) => {
                let mut set = GitEventSet {
                    state: false,
                    commit: false,
                    branch_changed: false,
                    dirty_changed: false,
                };
                for item in items {
                    match item.as_str() {
                        Some("state") => set.state = true,
                        Some("commit") => set.commit = true,
                        Some("branch_changed") => set.branch_changed = true,
                        Some("dirty_changed") => set.dirty_changed = true,
                        Some(other) => {
                            return Err(format!(
                                "unknown event type `{other}` (expected state|commit|branch_changed|dirty_changed)"
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

        let branch = wf
            .trigger
            .extra
            .get("branch")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let branch_pattern = match wf.trigger.extra.get("branch_pattern").and_then(|v| v.as_str()) {
            Some(p) => Some(
                regex_lite::Regex::new(p)
                    .map_err(|e| format!("invalid branch_pattern regex `{p}`: {e}"))?,
            ),
            None => None,
        };

        Ok(Self {
            workflow_name: name.to_string(),
            events,
            branch,
            branch_pattern,
        })
    }

    fn matches_branch(&self, branch: &str) -> bool {
        if let Some(b) = &self.branch {
            if b != branch {
                return false;
            }
        }
        if let Some(re) = &self.branch_pattern {
            if !re.is_match(branch) {
                return false;
            }
        }
        true
    }
}

fn git_event_type_for_type_id(type_id: &str) -> Option<&'static str> {
    match type_id {
        "com.nexus.git.state" => Some("state"),
        "com.nexus.git.commit" => Some("commit"),
        "com.nexus.git.branch_changed" => Some("branch_changed"),
        "com.nexus.git.dirty_changed" => Some("dirty_changed"),
        _ => None,
    }
}

impl WorkflowCorePlugin {
    /// Spawn one tokio task per `git_event`-triggered workflow. Each
    /// task subscribes to `com.nexus.git.*` on the kernel bus,
    /// filters events against the trigger's optional `events` /
    /// `branch` / `branch_pattern` fields, and dispatches
    /// `com.nexus.workflow::run` with `trigger.{event_type,branch,
    /// head,...}` variables when an event matches.
    ///
    /// Parse failures (e.g. invalid regex) log-and-skip that one
    /// workflow; other workflows keep their subscriptions. Handles
    /// are retained so the plugin can cancel them on drop alongside
    /// the cron / `file_event` schedulers.
    fn spawn_git_event_triggers(&self, ctx: &Arc<KernelPluginContext>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                "workflow scheduler: no tokio runtime available; git_event triggers disabled"
            );
            return;
        };
        let specs: Vec<GitEventSpec> = match self.registry.lock() {
            Ok(reg) => reg
                .iter()
                .filter(|(_, wf)| wf.trigger.trigger_type == "git_event")
                .filter_map(|(name, wf)| match GitEventSpec::from_trigger(name, wf) {
                    Ok(spec) => Some(spec),
                    Err(e) => {
                        tracing::warn!(workflow = %name, error = %e, "git_event trigger: spec parse failed; skipping");
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
                branch = ?spec.branch,
                has_branch_pattern = spec.branch_pattern.is_some(),
                events = ?spec.events,
                "git_event trigger armed"
            );
            let handle = runtime.spawn(async move {
                git_event_loop(ctx, spec).await;
            });
            handles.push(handle);
        }
    }
}

async fn git_event_loop(ctx: Arc<KernelPluginContext>, spec: GitEventSpec) {
    let mut sub = ctx.subscribe(EventFilter::CustomPrefix(
        "com.nexus.git.".to_string(),
    ));
    loop {
        let published = match sub.recv().await {
            Ok(e) => e,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(workflow = %spec.workflow_name, n, "git_event trigger lagged; events lost");
                continue;
            }
            Err(RecvError::Closed) => {
                tracing::debug!(workflow = %spec.workflow_name, "git_event trigger: bus closed");
                return;
            }
        };
        let NexusEvent::Custom { type_id, payload, .. } = &published.event else {
            continue;
        };
        let Some(event_type) = git_event_type_for_type_id(type_id) else {
            continue;
        };
        if !spec.events.matches(event_type) {
            continue;
        }
        // Branch field depends on topic: `branch_changed` carries the
        // new branch under `to`, every other topic carries it under
        // `branch`.
        let branch = if event_type == "branch_changed" {
            payload.get("to").and_then(|v| v.as_str()).unwrap_or_default()
        } else {
            payload.get("branch").and_then(|v| v.as_str()).unwrap_or_default()
        };
        if !spec.matches_branch(branch) {
            continue;
        }

        let mut trigger_vars = serde_json::Map::new();
        trigger_vars.insert(
            "event_type".into(),
            serde_json::Value::String(event_type.into()),
        );
        trigger_vars.insert("branch".into(), serde_json::Value::String(branch.into()));
        if let Some(head) = payload.get("head").cloned() {
            trigger_vars.insert("head".into(), head);
        }
        match event_type {
            "commit" => {
                if let Some(prev) = payload.get("prev_head").cloned() {
                    trigger_vars.insert("prev_head".into(), prev);
                }
            }
            "branch_changed" => {
                if let Some(from) = payload.get("from").cloned() {
                    trigger_vars.insert("from".into(), from);
                }
            }
            "state" | "dirty_changed" => {
                if let Some(d) = payload.get("is_dirty").cloned() {
                    trigger_vars.insert("is_dirty".into(), d);
                }
            }
            _ => {}
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
                    event_type, branch, "git_event trigger fired"
                );
            }
            Err(err) => {
                tracing::warn!(
                    workflow = %spec.workflow_name,
                    event_type, branch, %err,
                    "git_event trigger: run failed; continuing"
                );
            }
        }
    }
}

/// BL-028e — parsed `trigger.type = "mcp_event"` spec.
///
/// Subscribes to `com.nexus.mcp.*` on the kernel bus, filters by an
/// optional `events: [String]` allow-list. `host_started` is a
/// one-shot snapshot fired at MCP plugin boot — most workflows want
/// deltas, not snapshots, so `host_started` is **excluded** by default
/// (mirrors the git `state` topic). Opt in by listing it in `events`.
struct McpEventSpec {
    workflow_name: String,
    events: McpEventSet,
}

#[derive(Debug, Clone, Copy)]
struct McpEventSet {
    host_started: bool,
}

impl McpEventSet {
    /// Default set: empty for now. `host_started` is excluded as a
    /// snapshot. As more topics are added in `nexus-mcp`, default
    /// inclusions land here (e.g. delta events) — opt-out behaviour
    /// can be implemented per-topic if it matters.
    fn defaults() -> Self {
        Self {
            host_started: false,
        }
    }

    fn matches(self, event_type: &str) -> bool {
        match event_type {
            "host_started" => self.host_started,
            _ => false,
        }
    }
}

impl McpEventSpec {
    fn from_trigger(name: &str, wf: &Workflow) -> Result<Self, String> {
        let events = match wf.trigger.extra.get("events") {
            None => McpEventSet::defaults(),
            Some(toml::Value::Array(items)) => {
                let mut set = McpEventSet { host_started: false };
                for item in items {
                    match item.as_str() {
                        Some("host_started") => set.host_started = true,
                        Some(other) => {
                            return Err(format!(
                                "unknown event type `{other}` (expected host_started)"
                            ));
                        }
                        None => return Err("events array must contain strings".into()),
                    }
                }
                set
            }
            Some(_) => return Err("events must be an array of strings".into()),
        };
        Ok(Self {
            workflow_name: name.to_string(),
            events,
        })
    }
}

fn mcp_event_type_for_type_id(type_id: &str) -> Option<&'static str> {
    match type_id {
        "com.nexus.mcp.host.started" => Some("host_started"),
        _ => None,
    }
}

impl WorkflowCorePlugin {
    /// Spawn one tokio task per `mcp_event`-triggered workflow. Each
    /// task subscribes to `com.nexus.mcp.*` on the kernel bus,
    /// filters events against the trigger's optional `events` field,
    /// and dispatches `com.nexus.workflow::run` with `trigger.event_type`
    /// (plus any payload keys carried on the event) when an event
    /// matches.
    ///
    /// Parse failures log-and-skip that one workflow; other workflows
    /// keep their subscriptions. Handles are retained so the plugin
    /// can cancel them on drop alongside the cron / `file_event` /
    /// `git_event` schedulers.
    fn spawn_mcp_event_triggers(&self, ctx: &Arc<KernelPluginContext>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                "workflow scheduler: no tokio runtime available; mcp_event triggers disabled"
            );
            return;
        };
        let specs: Vec<McpEventSpec> = match self.registry.lock() {
            Ok(reg) => reg
                .iter()
                .filter(|(_, wf)| wf.trigger.trigger_type == "mcp_event")
                .filter_map(|(name, wf)| match McpEventSpec::from_trigger(name, wf) {
                    Ok(spec) => Some(spec),
                    Err(e) => {
                        tracing::warn!(workflow = %name, error = %e, "mcp_event trigger: spec parse failed; skipping");
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
                events = ?spec.events,
                "mcp_event trigger armed"
            );
            let handle = runtime.spawn(async move {
                mcp_event_loop(ctx, spec).await;
            });
            handles.push(handle);
        }
    }
}

async fn mcp_event_loop(ctx: Arc<KernelPluginContext>, spec: McpEventSpec) {
    let mut sub = ctx.subscribe(EventFilter::CustomPrefix(
        "com.nexus.mcp.".to_string(),
    ));
    loop {
        let published = match sub.recv().await {
            Ok(e) => e,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(workflow = %spec.workflow_name, n, "mcp_event trigger lagged; events lost");
                continue;
            }
            Err(RecvError::Closed) => {
                tracing::debug!(workflow = %spec.workflow_name, "mcp_event trigger: bus closed");
                return;
            }
        };
        let NexusEvent::Custom { type_id, payload, .. } = &published.event else {
            continue;
        };
        let Some(event_type) = mcp_event_type_for_type_id(type_id) else {
            continue;
        };
        if !spec.events.matches(event_type) {
            continue;
        }

        let mut trigger_vars = serde_json::Map::new();
        trigger_vars.insert(
            "event_type".into(),
            serde_json::Value::String(event_type.into()),
        );
        // Pass through every top-level payload key as `trigger.<key>`
        // so workflows can read whatever the event carries (e.g.
        // `configured_servers` on `host_started`).
        if let Some(obj) = payload.as_object() {
            for (k, v) in obj {
                trigger_vars.entry(k.clone()).or_insert_with(|| v.clone());
            }
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
                    event_type, "mcp_event trigger fired"
                );
            }
            Err(err) => {
                tracing::warn!(
                    workflow = %spec.workflow_name,
                    event_type, %err,
                    "mcp_event trigger: run failed; continuing"
                );
            }
        }
    }
}

impl WorkflowCorePlugin {
    /// BL-028g — spawn the webhook accept loop, when configured.
    ///
    /// Bails (with a `tracing::debug!`) if `[webhooks].enabled = false`
    /// or if no workflow declares a `webhook` trigger — both states
    /// mean there's no listener worth running. Spec parse failures
    /// log-and-skip per workflow.
    fn spawn_webhook_listener(&self, ctx: &Arc<KernelPluginContext>) {
        if !self.webhook_config.enabled {
            tracing::debug!("webhook listener: disabled in [webhooks].enabled");
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("webhook listener: no tokio runtime available; disabled");
            return;
        };
        let specs: Vec<webhook::WebhookSpec> = match self.registry.lock() {
            Ok(reg) => reg
                .iter()
                .filter(|(_, wf)| wf.trigger.trigger_type == "webhook")
                .filter_map(|(name, wf)| match webhook::WebhookSpec::from_trigger(name, wf) {
                    Ok(spec) => Some(spec),
                    Err(e) => {
                        tracing::warn!(workflow = %name, error = %e, "webhook trigger: spec parse failed; skipping");
                        None
                    }
                })
                .collect(),
            Err(_) => return,
        };
        if specs.is_empty() {
            tracing::debug!("webhook listener: no webhook-trigger workflows; not binding");
            return;
        }
        let bind = self.webhook_config.bind.clone();
        let ctx = Arc::clone(ctx);
        let specs = Arc::new(specs);
        let Ok(mut handles) = self.scheduler_handles.lock() else {
            return;
        };
        let handle = runtime.spawn(async move {
            webhook_accept_loop(ctx, bind, specs).await;
        });
        handles.push(handle);
    }
}

/// True iff `bind` is a loopback address-port pair. Parses out the
/// host and checks against `127.0.0.1` and `::1`. Used by the
/// webhook accept loop to decide whether to emit a "bound to a
/// non-loopback address" warn at arm time (issue #85).
fn is_loopback_bind(bind: &str) -> bool {
    // A bind string looks like `host:port` or `[v6]:port`. Strip the
    // port suffix and parse the host.
    let host = match bind.rsplit_once(':') {
        // `[::1]:18080` → `[::1]`
        Some((host, _port)) => host.trim_start_matches('[').trim_end_matches(']'),
        None => bind,
    };
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        // Unparseable host — be conservative and treat as non-loopback
        // so the warn fires (alerting the operator to a typo).
        Err(_) => host == "localhost",
    }
}

async fn webhook_accept_loop(
    ctx: Arc<KernelPluginContext>,
    bind: String,
    specs: Arc<Vec<webhook::WebhookSpec>>,
) {
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(%bind, error = %e, "webhook listener: bind failed; aborting");
            return;
        }
    };
    // Issue #85. Default config binds to `127.0.0.1`; an operator
    // who flips this to `0.0.0.0` (or another non-loopback address)
    // takes the documented responsibility for exposing the webhook
    // to the local network. Surface the choice loudly at bind time
    // so it's not silent — the workflow trigger arms even if the
    // operator forgot they edited `[webhooks].bind`.
    if !is_loopback_bind(&bind) {
        tracing::warn!(
            audit = true,
            %bind,
            "webhook listener bound to a non-loopback address; the listener \
             accepts requests from any host that can reach this address. \
             Ensure the bind config is intentional and your shared-secret \
             headers are set on every workflow."
        );
    }
    tracing::info!(%bind, count = specs.len(), "webhook listener armed");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "webhook listener: accept failed; continuing");
                continue;
            }
        };
        let ctx = Arc::clone(&ctx);
        let specs = Arc::clone(&specs);
        tokio::spawn(async move {
            handle_webhook_connection(ctx, sock, peer.to_string(), &specs).await;
        });
    }
}

async fn handle_webhook_connection(
    ctx: Arc<KernelPluginContext>,
    mut sock: tokio::net::TcpStream,
    peer: String,
    specs: &[webhook::WebhookSpec],
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let max = webhook::MAX_HEADER_BYTES + webhook::MAX_BODY_BYTES;
    let mut buf = Vec::with_capacity(2_048);
    let mut tmp = [0u8; 2_048];
    let read_deadline = tokio::time::Instant::now()
        + std::time::Duration::from_millis(webhook::READ_TIMEOUT_MS);
    let parsed = loop {
        if buf.len() >= max {
            let _ = write_status(&mut sock, 413, "Payload Too Large").await;
            return;
        }
        let remaining = read_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let _ = write_status(&mut sock, 408, "Request Timeout").await;
            return;
        }
        match tokio::time::timeout(remaining, sock.read(&mut tmp)).await {
            Ok(Ok(0)) => break webhook::parse_request(&buf),
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                match webhook::parse_request(&buf) {
                    Ok(req) => break Ok(req),
                    // need more bytes; loop will read again
                    Err(webhook::RequestError::Malformed) => {}
                    Err(other) => break Err(other),
                }
            }
            Ok(Err(_)) | Err(_) => {
                let _ = write_status(&mut sock, 408, "Request Timeout").await;
                return;
            }
        }
    };
    let req = match parsed {
        Ok(r) => r,
        Err(webhook::RequestError::BodyTooLarge) => {
            let _ = write_status(&mut sock, 413, "Payload Too Large").await;
            return;
        }
        Err(_) => {
            let _ = write_status(&mut sock, 400, "Bad Request").await;
            return;
        }
    };

    match webhook::route_request(specs, &req) {
        webhook::Route::NotFound => {
            let _ = write_status(&mut sock, 404, "Not Found").await;
        }
        webhook::Route::MethodNotAllowed => {
            let _ = write_status(&mut sock, 405, "Method Not Allowed").await;
        }
        webhook::Route::Unauthorized => {
            let _ = write_status(&mut sock, 401, "Unauthorized").await;
        }
        webhook::Route::Dispatch(spec) => {
            let variables = webhook::build_trigger_vars(&req, &peer);
            let args = serde_json::json!({
                "name": spec.workflow_name,
                "variables": variables,
            });
            let dispatch = ctx
                .ipc_call(
                    PLUGIN_ID,
                    "run",
                    args,
                    std::time::Duration::from_secs(600),
                )
                .await;
            match dispatch {
                Ok(_) => {
                    tracing::info!(workflow = %spec.workflow_name, path = %spec.path, peer = %peer, "webhook fired");
                    let _ = write_status_with_body(&mut sock, 200, "OK", b"{\"ok\":true}").await;
                }
                Err(err) => {
                    tracing::warn!(workflow = %spec.workflow_name, %err, "webhook dispatch failed");
                    let _ = write_status(&mut sock, 500, "Internal Server Error").await;
                }
            }
        }
    }
    let _ = sock.shutdown().await;
}

async fn write_status(sock: &mut tokio::net::TcpStream, code: u16, reason: &str) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let body = format!("{code} {reason}");
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(resp.as_bytes()).await
}

async fn write_status_with_body(
    sock: &mut tokio::net::TcpStream,
    code: u16,
    reason: &str,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    sock.write_all(header.as_bytes()).await?;
    sock.write_all(body).await
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
            HANDLER_RUN_DIGEST => Err(exec_err(format!(
                "handler {HANDLER_RUN_DIGEST}: run_digest is async; caller should use dispatch_async"
            ))),
            HANDLER_SET_DIGEST_CONFIG => Err(exec_err(format!(
                "handler {HANDLER_SET_DIGEST_CONFIG}: set_digest_config is async; caller should use dispatch_async"
            ))),
            HANDLER_TEMPLATES_LIST => Self::dispatch_templates_list(),
            HANDLER_TEMPLATES_GET => Self::dispatch_templates_get(args),
            HANDLER_TEMPLATES_INIT => self.dispatch_templates_init(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        if handler_id == HANDLER_RUN_DIGEST {
            let ctx = self.context.clone();
            let cfg_handle = Arc::clone(&self.digest_config);
            let args = args.clone();
            return Some(Box::pin(async move {
                let ctx = ctx.ok_or_else(|| {
                    exec_err(
                        "workflow plugin context not wired (bootstrap incomplete)".into(),
                    )
                })?;
                let kind_str = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| exec_err("run_digest: missing 'kind'".into()))?;
                let kind = DigestKind::from_str(kind_str).map_err(exec_err)?;
                let cfg = cfg_handle
                    .read()
                    .map(|g| g.clone())
                    .map_err(|_| exec_err("run_digest: digest config lock poisoned".to_string()))?;
                let report = digests::run_digest(&ctx, &cfg, kind, chrono::Utc::now())
                    .await
                    .map_err(|e| exec_err(format!("run_digest: {e}")))?;
                to_value(&report, "run_digest")
            }));
        }
        // FU-7 — `set_digest_config`: replace the live config under
        // the shared lock. The scheduler loop snapshots on every
        // tick, so an enabled-flip is picked up within 60 s.
        if handler_id == HANDLER_SET_DIGEST_CONFIG {
            let cfg_handle = Arc::clone(&self.digest_config);
            let args = args.clone();
            return Some(Box::pin(async move {
                let new_cfg: DigestConfig = serde_json::from_value(args).map_err(|e| {
                    exec_err(format!("set_digest_config: decode: {e}"))
                })?;
                {
                    let mut g = cfg_handle.write().map_err(|_| {
                        exec_err("set_digest_config: digest config lock poisoned".to_string())
                    })?;
                    *g = new_cfg;
                }
                Ok(serde_json::json!({ "applied": true }))
            }));
        }
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
        self.spawn_git_event_triggers(&ctx);
        self.spawn_mcp_event_triggers(&ctx);
        self.spawn_webhook_listener(&ctx);
        self.spawn_digest_scheduler(&ctx);
    }
}

fn lookup_by_args(
    registry: &Mutex<WorkflowRegistry>,
    args: &serde_json::Value,
) -> Result<Workflow, PluginError> {
    let a: RunWorkflowArgs = parse(args, "run")?;
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
            // BL-028d — AI prompt: route through `com.nexus.ai::ask`
            // and return the full RagResponse JSON. The handler's
            // `answer` field is the primary text output.
            "ai_prompt" => {
                let args = ai_steps::AiPromptArgs::from_step(step)?;
                let mut ipc_args = serde_json::json!({ "question": args.prompt });
                if let Some(limit) = args.limit {
                    ipc_args["limit"] = serde_json::Value::Number(limit.into());
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
        let a: GetWorkflowArgs = parse(args, "get")?;
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

    #[allow(clippy::unnecessary_wraps)] // dispatcher contract returns Result
    fn dispatch_templates_list() -> Result<serde_json::Value, PluginError> {
        let entries: Vec<_> = templates::CATALOG
            .iter()
            .map(|t| {
                serde_json::json!({
                    "slug": t.slug,
                    "description": t.description,
                    "tags": t.tags,
                    "filename": t.filename,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(entries))
    }

    fn dispatch_templates_get(
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: GetTemplateArgs = parse(args, "templates_get")?;
        let t = templates::find(&a.slug)
            .ok_or_else(|| exec_err(format!("no template named '{}'", a.slug)))?;
        Ok(serde_json::json!({
            "slug": t.slug,
            "description": t.description,
            "tags": t.tags,
            "filename": t.filename,
            "body": t.body,
        }))
    }

    fn dispatch_templates_init(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: InitTemplateArgs = parse(args, "templates_init")?;
        let t = templates::find(&a.slug)
            .ok_or_else(|| exec_err(format!("no template named '{}'", a.slug)))?;
        let filename = a
            .filename
            .as_deref()
            .map(sanitize_filename)
            .transpose()
            .map_err(exec_err)?
            .unwrap_or_else(|| t.filename.to_string());
        let target = self.root.join(&filename);
        if target.exists() && !a.overwrite {
            return Err(exec_err(format!(
                "templates_init: '{}' already exists (pass overwrite=true to replace)",
                target.display()
            )));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| exec_err(format!("templates_init: create_dir_all: {e}")))?;
        }
        std::fs::write(&target, t.body)
            .map_err(|e| exec_err(format!("templates_init: write: {e}")))?;
        Ok(serde_json::json!({
            "written": true,
            "path": target.to_string_lossy(),
            "slug": t.slug,
        }))
    }

    fn dispatch_validate(
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ValidateWorkflowArgs = parse(args, "validate")?;
        match parse_workflow_text(&a.text) {
            Ok(w) => to_value(&w, "validate"),
            Err(err) => Err(exec_err(format!("invalid workflow: {err}"))),
        }
    }
}

/// Defensive filename check for `templates_init`. Rejects path
/// separators and parent-dir hops so a malicious caller can't write
/// outside `<forge>/.workflows/`. Empty / whitespace-only names also
/// fail. Allowed: `<basename>` or `<basename>.workflow.toml`.
fn sanitize_filename(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("templates_init: filename cannot be empty".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err(format!(
            "templates_init: filename '{trimmed}' must be a bare basename (no path separators)"
        ));
    }
    Ok(trimmed.to_string())
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
    fn git_event_spec_parses_all_fields() {
        let src = r#"
[workflow]
name = "GE"

[trigger]
type = "git_event"
events = ["commit", "branch_changed"]
branch = "main"
branch_pattern = "^feat/.*"
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = GitEventSpec::from_trigger("GE", &wf).unwrap();
        assert_eq!(spec.branch.as_deref(), Some("main"));
        assert!(spec.branch_pattern.is_some());
        assert!(!spec.events.state);
        assert!(spec.events.commit);
        assert!(spec.events.branch_changed);
        assert!(!spec.events.dirty_changed);
    }

    #[test]
    fn git_event_spec_defaults_omit_state() {
        let src = r#"
[workflow]
name = "GE"

[trigger]
type = "git_event"
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = GitEventSpec::from_trigger("GE", &wf).unwrap();
        assert!(spec.branch.is_none());
        assert!(spec.branch_pattern.is_none());
        assert!(!spec.events.state, "state must be excluded by default");
        assert!(spec.events.commit);
        assert!(spec.events.branch_changed);
        assert!(spec.events.dirty_changed);
    }

    #[test]
    fn git_event_spec_rejects_invalid_event_name() {
        let src = r#"
[workflow]
name = "GE"

[trigger]
type = "git_event"
events = ["pushed"]
"#;
        let wf = parse_workflow_text(src).unwrap();
        assert!(GitEventSpec::from_trigger("GE", &wf).is_err());
    }

    #[test]
    fn git_event_spec_rejects_invalid_branch_regex() {
        let src = r#"
[workflow]
name = "GE"

[trigger]
type = "git_event"
branch_pattern = "[unterminated"
"#;
        let wf = parse_workflow_text(src).unwrap();
        assert!(GitEventSpec::from_trigger("GE", &wf).is_err());
    }

    #[test]
    fn git_event_type_mapping_covers_all_four_topics() {
        assert_eq!(git_event_type_for_type_id("com.nexus.git.state"), Some("state"));
        assert_eq!(git_event_type_for_type_id("com.nexus.git.commit"), Some("commit"));
        assert_eq!(
            git_event_type_for_type_id("com.nexus.git.branch_changed"),
            Some("branch_changed")
        );
        assert_eq!(
            git_event_type_for_type_id("com.nexus.git.dirty_changed"),
            Some("dirty_changed")
        );
        assert!(git_event_type_for_type_id("com.nexus.git.other").is_none());
        assert!(git_event_type_for_type_id("com.nexus.storage.file_created").is_none());
        // Sanity check: GitEventSet::all matches every short name we map.
        let all = GitEventSet::all();
        for short in ["state", "commit", "branch_changed", "dirty_changed"] {
            assert!(all.matches(short), "all() should include `{short}`");
        }
    }

    #[test]
    fn mcp_event_spec_defaults_exclude_host_started() {
        let src = r#"
[workflow]
name = "M"

[trigger]
type = "mcp_event"
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = McpEventSpec::from_trigger("M", &wf).unwrap();
        assert!(
            !spec.events.host_started,
            "host_started must be excluded by default (snapshot, not delta)"
        );
    }

    #[test]
    fn mcp_event_spec_opts_in_via_events_array() {
        let src = r#"
[workflow]
name = "M"

[trigger]
type = "mcp_event"
events = ["host_started"]
"#;
        let wf = parse_workflow_text(src).unwrap();
        let spec = McpEventSpec::from_trigger("M", &wf).unwrap();
        assert!(spec.events.host_started);
    }

    #[test]
    fn mcp_event_spec_rejects_unknown_event_name() {
        let src = r#"
[workflow]
name = "M"

[trigger]
type = "mcp_event"
events = ["nope"]
"#;
        let wf = parse_workflow_text(src).unwrap();
        assert!(McpEventSpec::from_trigger("M", &wf).is_err());
    }

    #[test]
    fn mcp_event_type_mapping_covers_known_topics() {
        assert_eq!(
            mcp_event_type_for_type_id("com.nexus.mcp.host.started"),
            Some("host_started")
        );
        assert!(mcp_event_type_for_type_id("com.nexus.mcp.other").is_none());
        assert!(mcp_event_type_for_type_id("com.nexus.git.state").is_none());
    }

    #[test]
    fn templates_list_returns_catalog() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(HANDLER_TEMPLATES_LIST, &serde_json::json!({}))
            .unwrap();
        let arr = v.as_array().unwrap();
        assert!(arr.len() >= 5);
        assert!(arr.iter().any(|e| e["slug"] == "daily-journal"));
    }

    #[test]
    fn templates_get_returns_body_for_known_slug() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(
                HANDLER_TEMPLATES_GET,
                &serde_json::json!({ "slug": "daily-journal" }),
            )
            .unwrap();
        assert_eq!(v["slug"], "daily-journal");
        assert!(v["body"].as_str().unwrap().contains("Daily Journal"));
    }

    #[test]
    fn templates_get_errors_for_unknown_slug() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(
                HANDLER_TEMPLATES_GET,
                &serde_json::json!({ "slug": "nope" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("no template"));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn templates_init_writes_file_and_refuses_to_clobber() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(
                HANDLER_TEMPLATES_INIT,
                &serde_json::json!({ "slug": "daily-journal" }),
            )
            .unwrap();
        assert_eq!(v["written"], true);
        let path_str = v["path"].as_str().unwrap().to_string();
        let written = std::fs::read_to_string(&path_str).unwrap();
        assert!(written.contains("Daily Journal"));

        // Second init without overwrite must fail.
        let err = plugin
            .dispatch(
                HANDLER_TEMPLATES_INIT,
                &serde_json::json!({ "slug": "daily-journal" }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("already exists"));
            }
            _ => panic!("unexpected"),
        }

        // With overwrite=true it succeeds.
        let v = plugin
            .dispatch(
                HANDLER_TEMPLATES_INIT,
                &serde_json::json!({ "slug": "daily-journal", "overwrite": true }),
            )
            .unwrap();
        assert_eq!(v["written"], true);
    }

    #[test]
    fn templates_init_rejects_filename_with_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(
                HANDLER_TEMPLATES_INIT,
                &serde_json::json!({
                    "slug": "daily-journal",
                    "filename": "../escape.toml"
                }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("bare basename"));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn templates_init_then_reload_picks_up_new_workflow() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        plugin
            .dispatch(
                HANDLER_TEMPLATES_INIT,
                &serde_json::json!({ "slug": "research-prompt" }),
            )
            .unwrap();
        let v = plugin.dispatch(HANDLER_RELOAD, &serde_json::json!({})).unwrap();
        assert_eq!(v["loaded"], 1);
        let list = plugin
            .dispatch(HANDLER_LIST, &serde_json::json!({}))
            .unwrap();
        assert_eq!(list[0]["workflow"]["name"], "Research Prompt");
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
