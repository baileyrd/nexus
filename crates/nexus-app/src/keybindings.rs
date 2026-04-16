//! User keybinding overrides.
//!
//! Stored in Tauri's `app_config_dir()` alongside the layout persistence
//! file. Each entry maps a command id (plugin-contributed or built-in)
//! to the user's overriding chord. Absent entries inherit the
//! manifest-declared default (or have no binding).
//!
//! Errors during load are non-fatal — a missing or corrupt file yields
//! an empty override map so a fresh install always boots.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE_NAME: &str = "keybindings.json";
const CURRENT_VERSION: u32 = 1;

/// Root of the persisted override state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingOverrides {
    /// Schema version for future migrations.
    pub version: u32,
    /// `commandId` → overriding chord (e.g. `"Mod+Shift+H"`).
    /// An empty string means the user explicitly unbound a default —
    /// reserved for a later slice; today we only write non-empty chords
    /// and `clear_keybinding_override` to remove an entry entirely.
    pub overrides: BTreeMap<String, String>,
}

impl Default for KeybindingOverrides {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            overrides: BTreeMap::new(),
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

/// Read overrides from disk, returning default on any error. Logs a
/// trace warning on corrupt files so bad data doesn't silently wipe
/// user state.
pub fn load_from(path: &Path) -> KeybindingOverrides {
    let Ok(bytes) = fs::read(path) else {
        return KeybindingOverrides::default();
    };
    match serde_json::from_slice::<KeybindingOverrides>(&bytes) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!(?path, %err, "keybindings.json unreadable — using defaults");
            KeybindingOverrides::default()
        }
    }
}

/// Atomically replace the persisted overrides file via a temp-file
/// rename, so a crash mid-write can't leave a partial JSON blob.
pub fn save_to(path: &Path, state: &KeybindingOverrides) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

/// Return the persisted override map, or an empty map if none exists yet.
#[tauri::command]
pub fn get_keybinding_overrides(app: AppHandle) -> Result<KeybindingOverrides, String> {
    let path = resolve_path(&app)?;
    Ok(load_from(&path))
}

/// Set a single override for `command_id`. The `binding` is accepted
/// verbatim — validation (parseability, modifier sanity) lives on the
/// frontend parser, so any string here is persisted as-is.
#[tauri::command]
pub fn set_keybinding_override(
    app: AppHandle,
    command_id: String,
    binding: String,
) -> Result<(), String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    state.overrides.insert(command_id, binding);
    save_to(&path, &state)
}

/// Remove a single override for `command_id`, reverting to whatever
/// the manifest (or builtin) declared. No-op if the key is absent.
#[tauri::command]
pub fn clear_keybinding_override(app: AppHandle, command_id: String) -> Result<(), String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    state.overrides.remove(&command_id);
    save_to(&path, &state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_overrides() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = KeybindingOverrides::default();
        state
            .overrides
            .insert("workspace.settings".into(), "Mod+,".into());
        state.overrides.insert("hello.sayHi".into(), "Alt+H".into());
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.overrides.len(), 2);
        assert_eq!(loaded.overrides["workspace.settings"], "Mod+,");
        assert_eq!(loaded.overrides["hello.sayHi"], "Alt+H");
    }

    #[test]
    fn missing_file_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("no-such-file.json");
        let loaded = load_from(&path);
        assert_eq!(loaded.version, CURRENT_VERSION);
        assert!(loaded.overrides.is_empty());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"{ not json").unwrap();
        let loaded = load_from(tmp.path());
        assert!(loaded.overrides.is_empty());
    }
}
