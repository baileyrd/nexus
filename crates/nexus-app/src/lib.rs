//! Nexus desktop shell.
//!
//! Boots a Tauri 2 application that hosts the React/Vite frontend and
//! bridges it to the Rust subsystems (currently `nexus-theme`; more will
//! join as later PRDs land).
//!
//! The split between `lib.rs` and `main.rs` follows the Tauri 2 mobile
//! convention — `run()` is callable from iOS/Android entry points, even
//! though only desktop targets are active today.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::sync::Mutex;

use nexus_theme::api::ThemeEngine;
use tauri::Manager;

pub mod commands;
pub mod editor;
pub mod forge;
pub mod keybindings;
pub mod persistence;
pub mod plugins;

/// Entry point for the desktop app. Called from `main.rs` (and from the
/// mobile entry points on those targets).
///
/// # Panics
/// Panics if Tauri itself fails to start (e.g. windowing stack unavailable).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::too_many_lines)]
pub fn run() {
    let engine = ThemeEngine::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::EngineState(Mutex::new(engine)))
        .manage(forge::ForgeState(Mutex::new(None)))
        .manage(forge::WatcherHandle(Mutex::new(None)))
        .manage(editor::KernelRuntime::empty())
        .manage(plugins::bootstrap())
        .setup(|app| {
            let handle = app.handle().clone();
            plugins::inject_event_forwarder(handle.clone());
            plugins::start_reload_watcher(handle.clone());
            plugins::start_host_event_watcher(handle.clone());
            match forge::bootstrap(&handle) {
                Ok(info) => {
                    tracing::info!(root = %info.root.display(), name = %info.name, "opened forge");
                    match forge::start_watcher(handle.clone(), &info.root) {
                        Ok(debouncer) => {
                            if let Some(state) = app.try_state::<forge::WatcherHandle>() {
                                if let Ok(mut guard) = state.0.lock() {
                                    *guard = Some(debouncer);
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(%err, "forge watcher failed to start; live tree refresh disabled");
                        }
                    }
                    match nexus_bootstrap::build_cli_runtime(info.root.clone()) {
                        Ok(runtime) => {
                            if let Some(state) = app.try_state::<editor::KernelRuntime>() {
                                state.set(std::sync::Arc::new(runtime));
                            }
                            tracing::info!("kernel runtime built for editor IPC");
                        }
                        Err(err) => {
                            tracing::warn!(%err, "kernel runtime build failed; editor IPC disabled");
                        }
                    }
                    if let Some(state) = app.try_state::<forge::ForgeState>() {
                        if let Ok(mut guard) = state.0.lock() {
                            *guard = Some(info);
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(%err, "forge bootstrap failed; UI will show no forge open");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_available_themes,
            commands::apply_theme,
            commands::compute_variables,
            commands::get_available_snippets,
            commands::toggle_snippet,
            commands::reorder_snippets,
            commands::get_theme_config,
            commands::set_mode,
            commands::get_default_layout,
            commands::get_layout_preset,
            commands::list_layout_presets,
            persistence::get_layout_persistence,
            persistence::save_layout_persistence,
            forge::current_forge,
            forge::open_forge,
            forge::list_forge_dir,
            forge::read_forge_file,
            forge::write_forge_file,
            forge::create_forge_file,
            forge::create_forge_dir,
            forge::rename_forge_entry,
            forge::delete_forge_entry,
            plugins::list_plugin_contributions,
            plugins::list_plugin_panels,
            plugins::list_plugin_settings_tabs,
            plugins::list_plugin_ribbon_items,
            plugins::list_plugin_status_items,
            plugins::list_plugin_slash_commands,
            plugins::get_plugin_settings_schema,
            plugins::get_plugin_settings,
            plugins::save_plugin_settings,
            plugins::invoke_plugin_command,
            plugins::invoke_plugin_ipc,
            plugins::read_plugin_script,
            plugins::toggle_plugin_subscription,
            plugins::publish_host_event,
            plugins::list_plugins,
            keybindings::get_keybinding_overrides,
            keybindings::set_keybinding_override,
            keybindings::clear_keybinding_override,
            editor::editor_open,
            editor::editor_close,
            editor::editor_get_tree,
            editor::editor_save,
            editor::editor_apply_transaction,
            editor::editor_undo,
            editor::editor_redo,
            editor::editor_list_open,
        ])
        .run(tauri::generate_context!())
        .expect("failed to launch nexus-app");
}
