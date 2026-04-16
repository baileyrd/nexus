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
use std::time::Duration;

use nexus_kernel::{EventFilter, RecvError};
use nexus_theme::api::ThemeEngine;
use tauri::{Emitter, Manager};

/// Tauri event emitted when any file under the active forge root changes.
/// Frontend listens via `@tauri-apps/api/event`.
///
/// Previously emitted by a `notify_debouncer_mini` watcher in `forge.rs`;
/// now forwarded by the storage-plugin kernel-bus subscriber started in
/// [`run`] so the frontend always sees notifications *after* the storage
/// index has been updated.
const FS_CHANGED_EVENT: &str = "forge:fs-changed";

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
                    match nexus_bootstrap::build_cli_runtime(info.root.clone()) {
                        Ok(runtime) => {
                            let runtime = std::sync::Arc::new(runtime);
                            // Subscribe to storage file-change events on the
                            // kernel bus and forward them to the frontend as
                            // `forge:fs-changed`. The storage plugin's watcher
                            // updates the index before publishing, so the
                            // frontend always sees notifications in order.
                            start_storage_event_forwarder(handle.clone(), &runtime);
                            if let Some(state) = app.try_state::<editor::KernelRuntime>() {
                                state.set(runtime);
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

/// Spawn a background thread that subscribes to `com.nexus.storage.file_*`
/// events on the kernel bus and forwards each one as a `forge:fs-changed`
/// Tauri event. This replaces the old `notify_debouncer_mini` watcher that
/// ran directly in `nexus-app`, which raced against the storage index update.
///
/// With this approach the frontend only sees a notification *after*
/// `StorageCorePlugin`'s bridge thread has already processed the raw OS event
/// and published the typed kernel event — so the index is always current
/// when the file tree re-fetches.
fn start_storage_event_forwarder(
    handle: tauri::AppHandle,
    runtime: &nexus_bootstrap::Runtime,
) {
    let bus = runtime.kernel.event_bus();
    let mut sub = bus.subscribe(EventFilter::CustomPrefix(
        "com.nexus.storage.file_".to_string(),
    ));

    std::thread::Builder::new()
        .name("nexus-storage-event-forwarder".to_string())
        .spawn(move || loop {
            // Poll at 100 ms intervals — fast enough to feel live, cheap
            // enough not to busy-spin.
            std::thread::sleep(Duration::from_millis(100));
            loop {
                match sub.try_recv() {
                    Ok(Some(_)) => {
                        if let Err(e) = handle.emit(FS_CHANGED_EVENT, ()) {
                            tracing::warn!(%e, "storage event forwarder: emit failed");
                        }
                    }
                    // No more events buffered — back to sleep.
                    Ok(None) => break,
                    // Fell behind; skip lost events and keep going.
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(n, "storage event forwarder: lagged, {n} events lost");
                    }
                    // Bus shut down — exit thread.
                    Err(RecvError::Closed) => return,
                }
            }
        })
        .expect("spawn storage event forwarder");
}
