//! Machine-global layout persistence.
//!
//! Keeps per-preset side-panel state (collapsed + active-panel ids) and
//! the most recently loaded preset id in a single JSON file under Tauri's
//! `app_config_dir()`. This is intentionally un-scoped to a forge —
//! `nexus-app` doesn't open one today. When forge opening lands, the
//! file format will gain a forge-id key layer (bump [`LayoutPersistence::version`]
//! and migrate).
//!
//! Errors during load are non-fatal: a missing or corrupt file yields
//! [`LayoutPersistence::default()`] so a fresh install never fails to boot.

// Tauri command extractors require owned values; documentation is on the
// module-level doc comment rather than per-command.
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

const FILE_NAME: &str = "layout-state.json";
const CURRENT_VERSION: u32 = 1;

/// Root of the persisted state. Keyed by preset id so switching between
/// presets preserves each one's side-panel state independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPersistence {
    /// Schema version for future migrations.
    pub version: u32,
    /// Last preset the user loaded, restored on next boot if still
    /// available. `None` → frontend falls back to the default preset.
    pub last_preset_id: Option<String>,
    /// Per-preset state keyed by preset id.
    pub layouts: std::collections::BTreeMap<String, PersistedLayoutState>,
}

impl Default for LayoutPersistence {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            last_preset_id: None,
            layouts: std::collections::BTreeMap::new(),
        }
    }
}

/// Per-preset side-panel state. Only fields the UI actually mutates
/// today are persisted; preset-declared defaults cover everything else.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLayoutState {
    /// `true` if the user has collapsed the left side panel.
    pub left_side_panel_collapsed: bool,
    /// `true` if the user has collapsed the right side panel.
    pub right_side_panel_collapsed: bool,
    /// Id of the visible panel on the left side, or `None` to fall back
    /// to whatever the preset declared visible.
    pub left_active_panel_id: Option<String>,
    /// Same as above for the right side.
    pub right_active_panel_id: Option<String>,
}

/// Resolve the on-disk file path, creating the parent directory if
/// needed. Returns the path regardless of whether the file exists.
fn resolve_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir.join(FILE_NAME))
}

/// Read the persisted state from disk, or return default on any error.
/// Logs a trace-level warning if the file exists but can't be parsed so
/// corruption doesn't silently wipe user state.
pub fn load_from(path: &Path) -> LayoutPersistence {
    let Ok(bytes) = fs::read(path) else {
        return LayoutPersistence::default();
    };
    match serde_json::from_slice::<LayoutPersistence>(&bytes) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!(?path, %err, "layout-state.json unreadable — using defaults");
            LayoutPersistence::default()
        }
    }
}

/// Serialize and atomically replace the persisted state file. Writes to
/// `<path>.tmp` then renames, so a crash mid-write can't produce a
/// partially-written JSON blob.
pub fn save_to(path: &Path, state: &LayoutPersistence) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

/// Return the persisted layout state, or a default if none exists yet.
#[tauri::command]
pub fn get_layout_persistence(app: AppHandle) -> Result<LayoutPersistence, String> {
    let path = resolve_path(&app)?;
    Ok(load_from(&path))
}

/// Overwrite the persisted layout state with the payload.
#[tauri::command]
pub fn save_layout_persistence(
    app: AppHandle,
    state: LayoutPersistence,
) -> Result<(), String> {
    let path = resolve_path(&app)?;
    save_to(&path, &state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_state() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let mut state = LayoutPersistence::default();
        state.last_preset_id = Some("vibe".into());
        state.layouts.insert(
            "obsidian".into(),
            PersistedLayoutState {
                left_side_panel_collapsed: false,
                right_side_panel_collapsed: true,
                left_active_panel_id: Some("files".into()),
                right_active_panel_id: None,
            },
        );

        save_to(&path, &state).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.last_preset_id.as_deref(), Some("vibe"));
        assert_eq!(loaded.layouts.len(), 1);
        assert!(loaded.layouts["obsidian"].right_side_panel_collapsed);
    }

    #[test]
    fn missing_file_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("no-such-file.json");
        let loaded = load_from(&path);
        assert_eq!(loaded.version, CURRENT_VERSION);
        assert!(loaded.layouts.is_empty());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"{ not json").unwrap();
        let loaded = load_from(tmp.path());
        assert!(loaded.layouts.is_empty());
    }
}
