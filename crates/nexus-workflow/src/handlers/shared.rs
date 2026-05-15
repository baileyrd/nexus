//! Cross-cutting helpers shared by every handler module under
//! `handlers/`. Moved out of `core_plugin.rs` by the BL-137 oversized-
//! file decomposition.
//!
//! Stays `pub(crate)` — these aren't part of the plugin's public
//! surface; they're just the tiny error / serde plumbing layer plus
//! the `publish_workflow_activity` helper used by `run`.

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;

use crate::core_plugin::PLUGIN_ID;

pub(crate) fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

pub(crate) fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("workflow registry mutex poisoned — prior handler panicked".into())
}

pub(crate) fn parse<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

pub(crate) fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize: {e}")))
}

/// BL-052 — emit a workflow start / end activity entry. `started=true`
/// labels the prompt as "started <name>"; `started=false` produces
/// "completed <name>" or, when `error` is set, "failed <name>".
pub(crate) async fn publish_workflow_activity(
    ctx: &KernelPluginContext,
    workflow_name: &str,
    started: bool,
    error: Option<String>,
) {
    use nexus_types::activity::{
        ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface,
        ACTIVITY_APPENDED_TOPIC,
    };
    let mut entry = ActivityEntry::now(
        workflow_name.to_string(),
        ActivitySurface::Workflow,
        ActivityOrigin::Workflow(workflow_name.to_string()),
    );
    if started {
        entry.outcome = ActivityOutcome::Ok;
        entry.prompt = format!("started {workflow_name}");
    } else if let Some(err) = error.as_ref() {
        entry.outcome = ActivityOutcome::Error;
        entry.prompt = format!("failed {workflow_name}");
        entry.error = Some(err.clone());
    } else {
        entry.outcome = ActivityOutcome::Ok;
        entry.prompt = format!("completed {workflow_name}");
    }
    if let Ok(payload) = serde_json::to_value(&entry) {
        let _ = ctx.publish(ACTIVITY_APPENDED_TOPIC, payload);
    }
}

/// Default per-step tool-call timeout for IPC dispatches initiated by
/// the workflow plugin. Workflow steps often span multiple plugins;
/// give them enough headroom.
pub(crate) const DEFAULT_STEP_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(60);
