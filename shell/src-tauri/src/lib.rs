// src-tauri/src/lib.rs

use std::fs;
use serde::{Deserialize, Serialize};

// ── Community plugin manifest ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPluginManifest {
    pub id:          String,
    pub name:        String,
    pub version:     String,
    pub main:        String,
    #[serde(default = "default_true")]
    pub enabled:     bool,
    pub description: Option<String>,
    pub author:      Option<String>,
    // Injected by scan — not present in plugin.json on disk
    #[serde(skip_deserializing, default)]
    pub dir:         String,
    #[serde(skip_deserializing, default)]
    pub manifest_path: String,
}

fn default_true() -> bool { true }

// ── Commands ──────────────────────────────────────────────────────────────────

/// Scan ~/.nexus-shell/plugins/ for community plugin bundles.
/// Each bundle is a sub-directory containing plugin.json + a JS entry point.
/// Creates the directory on first run so users know where to drop plugins.
/// Returns both enabled and disabled manifests — the frontend filters.
#[tauri::command]
fn scan_plugin_directory() -> Vec<CommunityPluginManifest> {
    let plugins_dir = match dirs::home_dir() {
        Some(h) => h.join(".nexus-shell").join("plugins"),
        None    => {
            eprintln!("[scan_plugin_directory] Cannot resolve home dir");
            return vec![];
        }
    };

    if !plugins_dir.exists() {
        if let Err(e) = fs::create_dir_all(&plugins_dir) {
            eprintln!("[scan_plugin_directory] Cannot create plugins dir: {e}");
            return vec![];
        }
    }

    let entries = match fs::read_dir(&plugins_dir) {
        Ok(e)  => e,
        Err(e) => {
            eprintln!("[scan_plugin_directory] Cannot read plugins dir: {e}");
            return vec![];
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir_path      = e.path();
            let manifest_path = dir_path.join("plugin.json");

            let content = fs::read_to_string(&manifest_path)
                .map_err(|_| eprintln!("[scan_plugin_directory] No plugin.json in {}", dir_path.display()))
                .ok()?;

            let mut manifest: CommunityPluginManifest = serde_json::from_str(&content)
                .map_err(|err| eprintln!(
                    "[scan_plugin_directory] Bad plugin.json in {}: {err}",
                    dir_path.display()
                ))
                .ok()?;

            // Verify main entry point exists before advertising the plugin
            if !dir_path.join(&manifest.main).exists() {
                eprintln!(
                    "[scan_plugin_directory] main '{}' not found in {}",
                    manifest.main, dir_path.display()
                );
                return None;
            }

            manifest.dir           = dir_path.to_string_lossy().into_owned();
            manifest.manifest_path = manifest_path.to_string_lossy().into_owned();
            Some(manifest)
        })
        .collect()
}

/// Scan an explicit directory path for community plugins.
/// Used in dev mode to load plugins straight from the repo without copying them.
#[tauri::command]
fn scan_plugin_directory_at(dir: String) -> Vec<CommunityPluginManifest> {
    let plugins_dir = std::path::Path::new(&dir);

    if !plugins_dir.exists() {
        return vec![];
    }

    let entries = match fs::read_dir(plugins_dir) {
        Ok(e)  => e,
        Err(e) => {
            eprintln!("[scan_plugin_directory_at] Cannot read {dir}: {e}");
            return vec![];
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir_path      = e.path();
            let manifest_path = dir_path.join("plugin.json");

            let content = fs::read_to_string(&manifest_path).ok()?;
            let mut manifest: CommunityPluginManifest = serde_json::from_str(&content)
                .map_err(|err| eprintln!(
                    "[scan_plugin_directory_at] Bad plugin.json in {}: {err}",
                    dir_path.display()
                ))
                .ok()?;

            if !dir_path.join(&manifest.main).exists() {
                eprintln!(
                    "[scan_plugin_directory_at] main '{}' not found in {}",
                    manifest.main, dir_path.display()
                );
                return None;
            }

            manifest.dir           = dir_path.to_string_lossy().into_owned();
            manifest.manifest_path = manifest_path.to_string_lossy().into_owned();
            Some(manifest)
        })
        .collect()
}

/// Persist the enabled/disabled state to a plugin's plugin.json.
#[tauri::command]
fn set_plugin_enabled(plugin_id: String, enabled: bool) -> Result<(), String> {
    let plugins_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot resolve home dir".to_string())?
        .join(".nexus-shell")
        .join("plugins");

    let entries = fs::read_dir(&plugins_dir)
        .map_err(|e| format!("Cannot read plugins dir: {e}"))?;

    for entry in entries.filter_map(|e| e.ok()) {
        let manifest_path = entry.path().join("plugin.json");
        let Ok(content) = fs::read_to_string(&manifest_path) else { continue };
        let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) else { continue };

        if json.get("id").and_then(|v| v.as_str()) == Some(plugin_id.as_str()) {
            json["enabled"] = serde_json::Value::Bool(enabled);
            let updated = serde_json::to_string_pretty(&json)
                .map_err(|e| format!("Serialize error: {e}"))?;
            fs::write(&manifest_path, updated)
                .map_err(|e| format!("Write error: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("Plugin '{plugin_id}' not found"))
}

/// Unscoped path existence check. tauri-plugin-fs scopes paths to a
/// configured allowlist, which rejects arbitrary user-picked folders
/// before we ever see them. This bypass uses std::path directly so the
/// workspace plugin can verify a persisted root on boot without having
/// to preconfigure every possible folder the user might open.
#[tauri::command]
fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_plugin_directory,
            scan_plugin_directory_at,
            set_plugin_enabled,
            path_exists,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
