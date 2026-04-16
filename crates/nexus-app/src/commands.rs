//! Tauri command bridge into the theme core plugin.
//!
//! Every command here is a thin adapter that serializes its args into
//! JSON and calls [`nexus_kernel::PluginContext::ipc_call`] on
//! `com.nexus.theme` via the kernel runtime held in Tauri state.
//!
//! State ownership lives in
//! [`nexus_theme::core_plugin::ThemeCorePlugin`] — registered by
//! [`nexus_bootstrap`] — so other plugins can reach the same engine via
//! `ipc_call` and subscribe to `com.nexus.theme.changed` events on the
//! kernel bus. See PRD-07 and `crates/nexus-theme/src/core_plugin.rs`
//! for the command-id → handler-id mapping.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use nexus_theme::api::{AppliedTheme, SnippetMetadata, ThemeConfig};
use nexus_theme::theme::ThemeMetadata;
use nexus_theme::{PresetInfo, PresetRegistry, ThemeMode, VariableMap, WorkspaceLayout};
use tauri::State;

use crate::editor::KernelRuntime;

/// Reverse-DNS id of the theme core plugin. Duplicated from
/// `nexus-theme` to avoid pulling the full crate through nexus-app's
/// public surface.
const THEME_PLUGIN_ID: &str = "com.nexus.theme";

/// Default per-call timeout. Theme ops are pure in-memory work —
/// anything longer than a second indicates a lock-contention bug.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

async fn call_theme(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(THEME_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

async fn call_theme_typed<T: serde::de::DeserializeOwned>(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<T, String> {
    let value = call_theme(runtime, command, args).await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// List every theme available to the engine (built-ins + discovered).
#[tauri::command]
pub async fn get_available_themes(
    runtime: State<'_, KernelRuntime>,
) -> Result<Vec<ThemeMetadata>, String> {
    call_theme_typed(runtime, "get_available_themes", serde_json::json!({})).await
}

/// Switch the active theme; returns the resolved variable map.
#[tauri::command]
pub async fn apply_theme(
    id: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<AppliedTheme, String> {
    call_theme_typed(runtime, "apply_theme", serde_json::json!({ "id": id })).await
}

/// Stateless cascade: compute the final variable map for the given theme
/// plus enabled snippets without mutating the engine's current selection.
#[tauri::command]
pub async fn compute_variables(
    theme_id: String,
    enabled_snippets: Vec<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<VariableMap, String> {
    call_theme_typed(
        runtime,
        "compute_variables",
        serde_json::json!({
            "theme_id": theme_id,
            "enabled_snippets": enabled_snippets,
        }),
    )
    .await
}

/// List every discovered CSS snippet with its enabled flag.
#[tauri::command]
pub async fn get_available_snippets(
    runtime: State<'_, KernelRuntime>,
) -> Result<Vec<SnippetMetadata>, String> {
    call_theme_typed(runtime, "get_available_snippets", serde_json::json!({})).await
}

/// Toggle a snippet on/off; returns the new enabled list.
#[tauri::command]
pub async fn toggle_snippet(
    id: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<Vec<String>, String> {
    call_theme_typed(runtime, "toggle_snippet", serde_json::json!({ "id": id })).await
}

/// Replace the ordered list of enabled snippet ids.
#[tauri::command]
pub async fn reorder_snippets(
    ids: Vec<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    call_theme(runtime, "reorder_snippets", serde_json::json!({ "ids": ids })).await?;
    Ok(())
}

/// Current theme selection + mode + snippet order.
#[tauri::command]
pub async fn get_theme_config(
    runtime: State<'_, KernelRuntime>,
) -> Result<ThemeConfig, String> {
    call_theme_typed(runtime, "get_theme_config", serde_json::json!({})).await
}

/// Switch the light/dark/system mode; returns the recomputed applied theme.
#[tauri::command]
pub async fn set_mode(
    mode: ThemeMode,
    runtime: State<'_, KernelRuntime>,
) -> Result<AppliedTheme, String> {
    call_theme_typed(runtime, "set_mode", serde_json::json!({ "mode": mode })).await
}

/// Return the default workspace layout shown on first launch.
///
/// Today this is the Obsidian preset — ribbon + panel sidebars on both
/// sides, single editor pane — which exercises the most layout surfaces.
#[tauri::command]
pub fn get_default_layout() -> WorkspaceLayout {
    PresetRegistry::with_core_presets()
        .get("obsidian")
        .expect("obsidian preset must be embedded")
}

/// Return a named layout preset hydrated into a fresh [`WorkspaceLayout`].
///
/// `name` is the preset id from [`list_layout_presets`]. Returns an error
/// string if the preset is unknown or fails to parse.
#[tauri::command]
pub fn get_layout_preset(name: String) -> Result<WorkspaceLayout, String> {
    PresetRegistry::with_core_presets()
        .get(&name)
        .map_err(|e| e.to_string())
}

/// List every available layout preset (embedded / user / plugin), sorted by
/// id. Used by the frontend picker to render entries dynamically rather than
/// hardcoding a union type.
#[tauri::command]
pub fn list_layout_presets() -> Vec<PresetInfo> {
    PresetRegistry::with_core_presets().list()
}
