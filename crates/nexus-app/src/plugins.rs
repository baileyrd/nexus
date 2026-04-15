//! Plugin integration: Tauri managed state + IPC commands.
//!
//! Holds a [`PluginManager`] behind a mutex so the frontend can list
//! plugin-contributed command palette entries and invoke them by id.

use std::path::PathBuf;
use std::sync::Mutex;

use nexus_plugins::{PluginManager, PluginManagerConfig, PluginStatus, TrustLevel, UiContribution};
use tauri::State;

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
/// # Errors
/// Returns the dispatch error as a string for the frontend.
#[tauri::command]
pub fn invoke_plugin_command(
    state: State<'_, PluginState>,
    plugin_id: String,
    command_id: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut mgr = state
        .0
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.dispatch_ipc(&plugin_id, &command_id, &args)
        .map_err(|e| e.to_string())
}

