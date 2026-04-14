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

pub mod commands;

/// Entry point for the desktop app. Called from `main.rs` (and from the
/// mobile entry points on those targets).
///
/// # Panics
/// Panics if Tauri itself fails to start (e.g. windowing stack unavailable).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = ThemeEngine::new();

    tauri::Builder::default()
        .manage(commands::EngineState(Mutex::new(engine)))
        .invoke_handler(tauri::generate_handler![
            commands::get_available_themes,
            commands::apply_theme,
            commands::compute_variables,
            commands::get_available_snippets,
            commands::toggle_snippet,
            commands::reorder_snippets,
            commands::get_theme_config,
            commands::set_mode,
        ])
        .run(tauri::generate_context!())
        .expect("failed to launch nexus-app");
}
