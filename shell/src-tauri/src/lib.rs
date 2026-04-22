// src-tauri/src/lib.rs

mod bridge;
mod persistence;

use std::fs;
use serde::{Deserialize, Serialize};
use tauri::Manager;

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

// Git status now routes through the kernel's git plugin via
// `api.kernel.invoke('com.nexus.git', 'status', {})`. The standalone
// `get_git_status` Tauri command (and the direct `git2` dependency it
// pulled in) was retired in Phase 1 of the shell ↔ kernel bridge
// migration (see docs/shell-kernel-bridge-plan.md).

// Directory listing now routes through the kernel's storage plugin via
// `api.kernel.invoke('com.nexus.storage', 'list_dir', { relpath })`. The
// standalone `read_dir` Tauri command was retired in Phase 1 of the
// shell ↔ kernel bridge migration (see docs/shell-kernel-bridge-plan.md).

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(bridge::KernelRuntime::new())
        // E2E-only: if NEXUS_E2E_VAULT is set, init + boot the kernel here
        // directly (bypassing the webview IPC path). Webdriver-injected
        // `invoke()` calls fail with "Origin header is not a valid URL" on
        // Tauri v2 + tauri-driver BiDi, so we pre-seed the runtime from
        // Rust. We also write the vault into shell-state's last_forge_path
        // so the launcher's recents / frontend restore paths see it too.
        .setup(|app| {
            let vault = match std::env::var("NEXUS_E2E_VAULT") {
                Ok(v) if !v.is_empty() => v,
                _ => return Ok(()),
            };
            eprintln!("[e2e-setup] NEXUS_E2E_VAULT={vault} — seeding kernel");

            let vault_path = std::path::PathBuf::from(&vault);
            let runtime_state = app.state::<bridge::KernelRuntime>();

            // Cap the block so a hung init/boot can't freeze app startup.
            let boot_result = tauri::async_runtime::block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    async {
                        bridge::init_forge(vault.clone()).await?;
                        runtime_state.boot_at(&vault_path).await
                    },
                )
                .await
                .map_err(|_| "timed out waiting for init_forge/boot_kernel".to_string())
                .and_then(|r| r)
            });

            match boot_result {
                Ok(()) => {
                    eprintln!("[e2e-setup] kernel booted at {vault}");
                    // Write to shell-state so the launcher's recents and any
                    // "restore last forge" path reflects the e2e vault.
                    if let Err(e) = persistence::write_last_forge_path(
                        app.handle().clone(),
                        vault.clone(),
                    ) {
                        eprintln!("[e2e-setup] write_last_forge_path failed: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("[e2e-setup] kernel boot failed: {e} (continuing)");
                }
            }

            Ok(())
        })
        // Fire the kernel shutdown when the user closes a window. Fire-and-
        // forget for now — Tauri 2's `CloseRequested` has an `api` handle we
        // could use to delay the actual close until shutdown completes, but
        // that adds complexity we don't need until something demonstrates a
        // race. A warning is logged if shutdown fails so it at least shows up
        // in the dev console.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app_handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = app_handle.state::<bridge::KernelRuntime>();
                    if let Err(e) = runtime.shutdown().await {
                        eprintln!("[shutdown] kernel shutdown failed: {e}");
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan_plugin_directory,
            scan_plugin_directory_at,
            set_plugin_enabled,
            path_exists,
            persistence::get_shell_state,
            persistence::save_shell_state,
            persistence::write_last_forge_path,
            persistence::forget_forge_path,
            bridge::init_forge,
            bridge::boot_kernel,
            bridge::shutdown_kernel,
            bridge::kernel_invoke,
            bridge::kernel_subscribe,
            bridge::kernel_unsubscribe,
            bridge::kernel_is_booted,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
