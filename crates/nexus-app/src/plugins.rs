//! Plugin integration: Tauri managed state + IPC commands.
//!
//! Holds a [`PluginManager`] behind a mutex so the frontend can list
//! plugin-contributed command palette entries and invoke them by id.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use nexus_plugins::{
    PluginManager, PluginManagerConfig, PluginStatus, TrustLevel, UiContribution,
    UiPanelContribution, UiRibbonItemContribution, UiSettingsTabContribution,
    UiStatusItemContribution,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// Tauri event emitted when one or more community plugins have been
/// hot-reloaded. Payload: `{ "plugin_ids": ["com.nexus.hello", …] }`.
pub const PLUGINS_RELOADED_EVENT: &str = "plugins:reloaded";

/// Tauri event emitted once per plugin-side event. Payload:
/// `{ "plugin_id": "...", "topic": "...", "payload": <any> }`. Plugins
/// surface events by returning an `events` array in their handler
/// response; `invoke_plugin_command` extracts the array and fires one
/// of these per entry.
pub const PLUGIN_EVENT_EVENT: &str = "plugin:event";

/// How often the background watcher thread drains pending hot-reload
/// events. The underlying `HotReloader` already debounces filesystem
/// events, so this just needs to be short enough to feel live.
const RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Tauri-managed [`PluginManager`] wrapped in a mutex for interior mutability.
pub struct PluginState(pub Mutex<PluginManager>);

/// Frontend-facing projection of [`nexus_kernel::PluginInfo`].
///
/// Kept separate from `PluginInfo` so we can serialize without forcing
/// `Serialize` onto kernel types (their `CapabilitySet` in particular).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginSummary {
    /// Plugin identifier (reverse-DNS).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Version string from the manifest.
    pub version: String,
    /// Trust level — `"core"` or `"community"`.
    pub trust_level: String,
    /// Current runtime status — `"loaded"`, `"initialized"`, `"running"`,
    /// `"stopped"`, or `"crashed"`.
    pub status: String,
}

fn trust_level_str(level: TrustLevel) -> &'static str {
    match level {
        TrustLevel::Core => "core",
        TrustLevel::Community => "community",
    }
}

fn status_str(status: PluginStatus) -> &'static str {
    match status {
        PluginStatus::Loaded => "loaded",
        PluginStatus::Initialized => "initialized",
        PluginStatus::Running => "running",
        PluginStatus::Stopped => "stopped",
        PluginStatus::Crashed => "crashed",
    }
}

/// Resolve the plugins directory.
///
/// Order of precedence:
/// 1. `NEXUS_PLUGINS_DIR` environment variable (absolute path).
/// 2. The repository's `plugins/` directory when running in dev (detected by
///    walking up from `CARGO_MANIFEST_DIR`).
/// 3. `$CWD/plugins`.
fn resolve_plugins_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("NEXUS_PLUGINS_DIR") {
        return PathBuf::from(explicit);
    }
    // crates/nexus-app -> repo root is two levels up.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo_root) = manifest_dir.parent().and_then(|p| p.parent()) {
        let candidate = repo_root.join("plugins");
        if candidate.exists() {
            return candidate;
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("plugins")
}

/// Build the [`PluginManager`], scan the plugins directory, and return the
/// managed state. Missing plugin directories are created silently so the
/// hot-reload watcher can attach.
pub fn bootstrap() -> PluginState {
    let dir = resolve_plugins_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%err, path = %dir.display(), "failed to ensure plugins dir");
    }
    let config = PluginManagerConfig::default();
    let mut manager = match PluginManager::new(&dir, &config) {
        Ok(m) => m,
        Err(err) => {
            tracing::warn!(%err, "plugin manager init failed; plugins disabled");
            // Fall back to a no-op manager rooted at a scratch dir so the
            // managed-state shape is preserved.
            let scratch = std::env::temp_dir().join("nexus-plugins-empty");
            let _ = std::fs::create_dir_all(&scratch);
            PluginManager::new(
                &scratch,
                &PluginManagerConfig {
                    hot_reload: false,
                    ..PluginManagerConfig::default()
                },
            )
            .expect("scratch plugin manager")
        }
    };
    match manager.load_all() {
        Ok(infos) => {
            tracing::info!(count = infos.len(), "loaded plugins");
        }
        Err(err) => {
            tracing::warn!(%err, "plugin scan failed");
        }
    }
    PluginState(Mutex::new(manager))
}

/// List all plugin-contributed palette commands across every loaded plugin.
#[tauri::command]
pub fn list_plugin_contributions(state: State<'_, PluginState>) -> Vec<UiContribution> {
    state
        .0
        .lock()
        .map(|mgr| mgr.ui_contributions())
        .unwrap_or_default()
}

/// List all plugin-contributed side panels across every loaded plugin.
/// The frontend merges these into the active layout's left/right side
/// panel arrays at render time.
#[tauri::command]
pub fn list_plugin_panels(state: State<'_, PluginState>) -> Vec<UiPanelContribution> {
    state
        .0
        .lock()
        .map(|mgr| mgr.ui_panels())
        .unwrap_or_default()
}

/// List all plugin-contributed Settings-modal tabs. The frontend
/// renders one row per tab under the Settings modal's "Plugins" rail
/// group.
#[tauri::command]
pub fn list_plugin_settings_tabs(
    state: State<'_, PluginState>,
) -> Vec<UiSettingsTabContribution> {
    state
        .0
        .lock()
        .map(|mgr| mgr.ui_settings_tabs())
        .unwrap_or_default()
}

/// List all plugin-contributed workspace-ribbon icons. The frontend
/// merges these into the active layout's `ribbon` array at render
/// time.
#[tauri::command]
pub fn list_plugin_ribbon_items(
    state: State<'_, PluginState>,
) -> Vec<UiRibbonItemContribution> {
    state
        .0
        .lock()
        .map(|mgr| mgr.ui_ribbon_items())
        .unwrap_or_default()
}

/// List all plugin-contributed status-bar entries. The frontend
/// merges these into the active layout's `statusBar` array.
#[tauri::command]
pub fn list_plugin_status_items(
    state: State<'_, PluginState>,
) -> Vec<UiStatusItemContribution> {
    state
        .0
        .lock()
        .map(|mgr| mgr.ui_status_items())
        .unwrap_or_default()
}

/// Return the JSON Schema declared by `plugin_id`, or `null` if the
/// plugin isn't loaded or didn't declare a `[settings]` block.
#[tauri::command]
pub fn get_plugin_settings_schema(
    state: State<'_, PluginState>,
    plugin_id: String,
) -> Option<serde_json::Value> {
    state
        .0
        .lock()
        .ok()
        .and_then(|mgr| mgr.get_settings_schema(&plugin_id))
}

/// Load the currently persisted settings for `plugin_id`. Empty
/// object when no settings file exists yet.
///
/// # Errors
/// Returns the load error as a string for the frontend.
#[tauri::command]
pub fn get_plugin_settings(
    state: State<'_, PluginState>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let mgr = state
        .0
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.get_settings(&plugin_id).map_err(|e| e.to_string())
}

/// Validate `settings` against the registered schema and, if valid,
/// persist them to `<plugin_dir>/settings.json`. Fires the plugin's
/// `on_settings_changed` lifecycle hook if declared.
///
/// # Errors
/// Returns validation / I/O errors as a string for the frontend.
#[tauri::command]
pub fn save_plugin_settings(
    state: State<'_, PluginState>,
    plugin_id: String,
    settings: serde_json::Value,
) -> Result<(), String> {
    let mut mgr = state
        .0
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.set_settings(&plugin_id, &settings).map_err(|e| e.to_string())
}

/// List every loaded plugin as a serializable summary — used by the
/// Settings modal's plugins tab.
#[tauri::command]
pub fn list_plugins(state: State<'_, PluginState>) -> Vec<PluginSummary> {
    let Ok(mgr) = state.0.lock() else {
        return Vec::new();
    };
    mgr.list()
        .into_iter()
        .map(|info| PluginSummary {
            id: info.id,
            name: info.name,
            version: info.version,
            trust_level: trust_level_str(info.trust_level).to_string(),
            status: status_str(info.status).to_string(),
        })
        .collect()
}

/// Invoke a plugin command by `plugin_id` and `command_id`, forwarding
/// arbitrary JSON `args`.
///
/// Side-effect: if the plugin's response is a JSON object containing
/// an `events: [{ topic, payload }, …]` array, each entry is emitted
/// as a [`PLUGIN_EVENT_EVENT`] Tauri event with
/// `{ plugin_id, topic, payload }`. The `events` key is left in the
/// returned value; the frontend can either ignore it or route it
/// through the dedicated event bus.
///
/// # Errors
/// Returns the dispatch error as a string for the frontend.
#[tauri::command]
pub fn invoke_plugin_command(
    app: AppHandle,
    state: State<'_, PluginState>,
    plugin_id: String,
    command_id: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = {
        let mut mgr = state
            .0
            .lock()
            .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
        mgr.dispatch_ipc(&plugin_id, &command_id, &args)
            .map_err(|e| e.to_string())?
    };
    emit_plugin_events(&app, &plugin_id, &result);
    Ok(result)
}

/// Pull an optional `events` array off a plugin's response and emit
/// each entry as a [`PLUGIN_EVENT_EVENT`] Tauri event. Malformed
/// entries (missing `topic`, non-object, etc.) are logged and skipped
/// so one bad event can't take out the rest.
fn emit_plugin_events(app: &AppHandle, plugin_id: &str, result: &serde_json::Value) {
    let Some(events) = result.get("events").and_then(|v| v.as_array()) else {
        return;
    };
    for event in events {
        let Some(topic) = event.get("topic").and_then(|v| v.as_str()) else {
            tracing::warn!(plugin = plugin_id, "plugin event missing 'topic'; skipping");
            continue;
        };
        let payload = event
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let envelope = serde_json::json!({
            "plugin_id": plugin_id,
            "topic": topic,
            "payload": payload,
        });
        if let Err(err) = app.emit(PLUGIN_EVENT_EVENT, envelope) {
            tracing::warn!(%err, plugin = plugin_id, topic, "failed to emit plugin event");
        }
    }
}

/// Spawn a background thread that drains [`PluginManager::poll_reloads`]
/// and emits [`PLUGINS_RELOADED_EVENT`] to the frontend whenever one or
/// more plugins have been hot-reloaded.
///
/// The thread lives for the app process lifetime. It is cheap: it sleeps
/// between polls and only briefly locks the [`PluginState`] mutex to
/// drain pending events.
pub fn start_reload_watcher(handle: AppHandle) {
    std::thread::Builder::new()
        .name("nexus-plugin-reload-watcher".to_string())
        .spawn(move || loop {
            std::thread::sleep(RELOAD_POLL_INTERVAL);
            let Some(state) = handle.try_state::<PluginState>() else {
                // Managed state disappeared — app is shutting down.
                return;
            };
            let reloaded = {
                let Ok(mut mgr) = state.0.lock() else {
                    continue;
                };
                match mgr.poll_reloads() {
                    Ok(ids) => ids,
                    Err(err) => {
                        tracing::warn!(%err, "poll_reloads failed");
                        continue;
                    }
                }
            };
            if reloaded.is_empty() {
                continue;
            }
            tracing::info!(plugins = ?reloaded, "hot-reloaded plugins");
            if let Err(err) = handle.emit(
                PLUGINS_RELOADED_EVENT,
                serde_json::json!({ "plugin_ids": reloaded }),
            ) {
                tracing::warn!(%err, "failed to emit plugins:reloaded");
            }
        })
        .expect("spawn plugin reload watcher");
}

