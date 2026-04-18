//! Tauri command bridge into the workflow core plugin.
//!
//! Thin adapter: serializes args to JSON and calls
//! [`nexus_kernel::PluginContext::ipc_call`] on `com.nexus.workflow`.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

const WORKFLOW_PLUGIN_ID: &str = "com.nexus.workflow";
const WORKFLOW_CALL_TIMEOUT: Duration = Duration::from_secs(30);

async fn call(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(WORKFLOW_PLUGIN_ID, command, args, WORKFLOW_CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Every loaded workflow.
#[tauri::command]
pub async fn workflow_list(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "list", serde_json::json!({})).await
}

/// One workflow by `[workflow].name`.
#[tauri::command]
pub async fn workflow_get(
    name: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "get", serde_json::json!({ "name": name })).await
}

/// Re-scan the `.workflows/` directory.
#[tauri::command]
pub async fn workflow_reload(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "reload", serde_json::json!({})).await
}

/// Parse + validate raw TOML text without loading it into the registry.
#[tauri::command]
pub async fn workflow_validate(
    text: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "validate", serde_json::json!({ "text": text })).await
}
