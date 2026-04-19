//! Shell-side persisted state.
//!
//! Single JSON file at `<app_config_dir>/shell-state.json`, read once at
//! startup and atomically rewritten (tmp → rename). Mirrors the pattern
//! in `crates/nexus-app/src/persistence.rs` so when `nexus-app` is
//! retired the file format and helpers carry over cleanly.
//!
//! Grows over time: today it just tracks the most-recently-opened forge
//! paths so the launcher can show a recents list; per-forge UI state
//! (expanded tree paths, open tabs) will be added as the corresponding
//! plugins migrate off localStorage.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE_NAME: &str = "shell-state.json";
const CURRENT_VERSION: u32 = 1;
/// Cap on the recents list. Older entries drop off the end.
const MAX_RECENT_FORGES: usize = 8;

/// Root of the persisted state. `#[serde(default)]` on every field so
/// older files that predate later additions still load.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    pub version: u32,
    /// Absolute path of the last forge the user opened. Restored on next
    /// boot if the directory still exists.
    #[serde(default)]
    pub last_forge_path: Option<String>,
    /// Newest-first list of recently-opened forge paths, capped to
    /// `MAX_RECENT_FORGES`. Updated alongside `last_forge_path`.
    #[serde(default)]
    pub recent_forge_paths: Vec<String>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            last_forge_path: None,
            recent_forge_paths: Vec::new(),
        }
    }
}

fn resolve_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir.join(FILE_NAME))
}

/// Load from disk. Any read or parse error returns `default()` so a
/// fresh install or a corrupted file never blocks startup.
fn load_from(path: &Path) -> ShellState {
    let Ok(bytes) = fs::read(path) else {
        return ShellState::default();
    };
    match serde_json::from_slice::<ShellState>(&bytes) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("[persistence] {} unreadable — using defaults: {err}", path.display());
            ShellState::default()
        }
    }
}

/// Atomic write: serialize to a sibling `.tmp` then rename over the
/// target so a crash mid-write can't produce a half-written file.
fn save_to(path: &Path, state: &ShellState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Return the full persisted state, or a default if none exists yet.
#[tauri::command]
pub fn get_shell_state(app: AppHandle) -> Result<ShellState, String> {
    let path = resolve_path(&app)?;
    Ok(load_from(&path))
}

/// Overwrite the persisted state. The frontend owns the full shape and
/// sends the whole object; we don't merge. That keeps the serialization
/// single-threaded and avoids races between frontend and backend-driven
/// updates (backend writes go through the dedicated helpers below).
#[tauri::command]
pub fn save_shell_state(app: AppHandle, state: ShellState) -> Result<(), String> {
    let path = resolve_path(&app)?;
    save_to(&path, &state)
}

/// Record `forge_path` as the new `last_forge_path` and promote it to
/// the front of `recent_forge_paths` (dedupe + cap). All other fields
/// preserved. Called from backend flows that open a forge without the
/// frontend holding the full state in memory.
#[tauri::command]
pub fn write_last_forge_path(app: AppHandle, forge_path: String) -> Result<ShellState, String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    state.last_forge_path = Some(forge_path.clone());
    state.recent_forge_paths.retain(|p| p != &forge_path);
    state.recent_forge_paths.insert(0, forge_path);
    state.recent_forge_paths.truncate(MAX_RECENT_FORGES);
    save_to(&path, &state)?;
    Ok(state)
}

/// Remove `forge_path` from the recents list and clear `last_forge_path`
/// if it matches. Used by the launcher's "Remove from recents" menu.
#[tauri::command]
pub fn forget_forge_path(app: AppHandle, forge_path: String) -> Result<ShellState, String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    state.recent_forge_paths.retain(|p| p != &forge_path);
    if state.last_forge_path.as_deref() == Some(forge_path.as_str()) {
        state.last_forge_path = None;
    }
    save_to(&path, &state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_state() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = ShellState::default();
        state.last_forge_path = Some("/forge/one".into());
        state.recent_forge_paths = vec!["/forge/one".into(), "/forge/two".into()];
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.last_forge_path.as_deref(), Some("/forge/one"));
        assert_eq!(loaded.recent_forge_paths, vec!["/forge/one", "/forge/two"]);
    }

    #[test]
    fn missing_file_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("no-such.json");
        let loaded = load_from(&path);
        assert_eq!(loaded.version, CURRENT_VERSION);
        assert!(loaded.recent_forge_paths.is_empty());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"{ not json").unwrap();
        let loaded = load_from(tmp.path());
        assert!(loaded.recent_forge_paths.is_empty());
    }
}
