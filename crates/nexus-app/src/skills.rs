//! Tauri command bridge into the skills core plugin.
//!
//! Thin adapter: serializes args to JSON and calls
//! [`nexus_kernel::PluginContext::ipc_call`] on `com.nexus.skills`.
//! Skill lookups are read-mostly; 30s timeout mirrors the CLI.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

const SKILLS_PLUGIN_ID: &str = "com.nexus.skills";
const SKILLS_CALL_TIMEOUT: Duration = Duration::from_secs(30);

async fn call(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(SKILLS_PLUGIN_ID, command, args, SKILLS_CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Every loaded skill.
#[tauri::command]
pub async fn skills_list(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "list", serde_json::json!({})).await
}

/// One skill by id. Errors when the id isn't registered.
#[tauri::command]
pub async fn skills_get(
    id: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "get", serde_json::json!({ "id": id })).await
}

/// Render a skill's body with parameter substitution.
#[tauri::command]
pub async fn skills_render(
    id: String,
    values: Option<serde_json::Map<String, serde_json::Value>>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(
        runtime,
        "render",
        serde_json::json!({ "id": id, "values": values.unwrap_or_default() }),
    )
    .await
}

/// Re-scan the `.forge/skills/` directory.
#[tauri::command]
pub async fn skills_reload(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call(runtime, "reload", serde_json::json!({})).await
}
