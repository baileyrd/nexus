//! Thin `#[tauri::command]` wrappers around the stateful [`ThemeEngine`].
//!
//! Each command locks the engine Mutex, forwards to the underlying method,
//! and maps [`nexus_theme::ThemeError`] to a string so Tauri can send it
//! over IPC.
//!
//! Tauri's extractor pattern requires `State<'_, T>` by value and invokes
//! these functions via the `invoke_handler!` macro rather than direct call
//! sites, so two pedantic clippy lints are suppressed here.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc
)]

use std::sync::Mutex;

use nexus_theme::api::{AppliedTheme, SnippetMetadata, ThemeConfig, ThemeEngine};
use nexus_theme::theme::ThemeMetadata;
use nexus_theme::VariableMap;
use tauri::State;

/// Tauri-managed engine handle shared across commands.
pub struct EngineState(pub Mutex<ThemeEngine>);

fn lock<'a>(state: &'a State<'_, EngineState>) -> std::sync::MutexGuard<'a, ThemeEngine> {
    // Panics only if a previous invocation panicked while holding the lock —
    // at which point the app is in a broken state and a crash is honest.
    state.0.lock().expect("theme engine mutex poisoned")
}

/// List every theme available to the engine (built-ins + discovered).
#[tauri::command]
pub fn get_available_themes(state: State<'_, EngineState>) -> Vec<ThemeMetadata> {
    lock(&state).get_available_themes()
}

/// Switch the active theme; returns the resolved variable map.
#[tauri::command]
pub fn apply_theme(id: String, state: State<'_, EngineState>) -> Result<AppliedTheme, String> {
    lock(&state).apply_theme(&id).map_err(|e| e.to_string())
}

/// Stateless cascade: compute the final variable map for the given theme
/// plus enabled snippets without mutating the engine's current selection.
#[tauri::command]
pub fn compute_variables(
    theme_id: String,
    enabled_snippets: Vec<String>,
    state: State<'_, EngineState>,
) -> Result<VariableMap, String> {
    lock(&state)
        .compute_variables(&theme_id, &enabled_snippets)
        .map_err(|e| e.to_string())
}

/// List every discovered CSS snippet with its enabled flag.
#[tauri::command]
pub fn get_available_snippets(state: State<'_, EngineState>) -> Vec<SnippetMetadata> {
    lock(&state).get_available_snippets()
}

/// Toggle a snippet on/off; returns the new enabled list.
#[tauri::command]
pub fn toggle_snippet(id: String, state: State<'_, EngineState>) -> Result<Vec<String>, String> {
    lock(&state).toggle_snippet(&id).map_err(|e| e.to_string())
}

/// Replace the ordered list of enabled snippet ids.
#[tauri::command]
pub fn reorder_snippets(
    ids: Vec<String>,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    lock(&state).reorder_snippets(ids).map_err(|e| e.to_string())
}

/// Current theme selection + mode + snippet order.
#[tauri::command]
pub fn get_theme_config(state: State<'_, EngineState>) -> ThemeConfig {
    lock(&state).config()
}
