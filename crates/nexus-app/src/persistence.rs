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
/// Maximum number of recent forge paths retained. Older entries drop
/// off the end once the list exceeds this size.
const MAX_RECENT_FORGES: usize = 8;

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
    /// Absolute path of the last forge the user opened, restored on
    /// next boot if the directory still exists. Written by the forge
    /// module on every successful `open_forge` call. Older files
    /// without this field deserialize as `None`.
    #[serde(default)]
    pub last_forge_path: Option<String>,
    /// Most-recently-used forge roots, newest first. Capped to
    /// [`MAX_RECENT_FORGES`]. Updated alongside `last_forge_path`.
    #[serde(default)]
    pub recent_forge_paths: Vec<String>,
    /// Per-preset state keyed by preset id.
    pub layouts: std::collections::BTreeMap<String, PersistedLayoutState>,
    /// Per-forge UI state (expanded tree paths, last-open file)
    /// keyed by forge root absolute path. Older files without this
    /// field deserialize as an empty map.
    #[serde(default)]
    pub forge_state: std::collections::BTreeMap<String, ForgeUiState>,
}

impl Default for LayoutPersistence {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            last_preset_id: None,
            last_forge_path: None,
            recent_forge_paths: Vec::new(),
            layouts: std::collections::BTreeMap::new(),
            forge_state: std::collections::BTreeMap::new(),
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

/// UI state remembered per forge: which directories were expanded and
/// which file was open in the viewer. Restored when that forge is
/// re-opened.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeUiState {
    /// Relpaths of directories that were expanded in the file tree.
    #[serde(default)]
    pub expanded_paths: Vec<String>,
    /// Relpath of the file open in the viewer, if any.
    #[serde(default)]
    pub open_file: Option<String>,
}

/// Resolve the on-disk file path, creating the parent directory if
/// needed. Returns the path regardless of whether the file exists.
pub(crate) fn resolve_path(app: &AppHandle) -> Result<PathBuf, String> {
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

/// Overwrite the persisted layout state with the payload, preserving
/// any backend-managed fields (`last_forge_path`) the frontend doesn't
/// own. Without this merge, every layout-change save would clobber the
/// path the user picked in the forge dialog.
#[tauri::command]
pub fn save_layout_persistence(app: AppHandle, mut state: LayoutPersistence) -> Result<(), String> {
    let path = resolve_path(&app)?;
    let existing = load_from(&path);
    state.last_forge_path = existing.last_forge_path;
    state.recent_forge_paths = existing.recent_forge_paths;
    save_to(&path, &state)
}

// ─── Forge-path helpers ───────────────────────────────────────────────────────

/// Read the persisted `last_forge_path`, or `None` if no persistence file
/// exists or the field is unset.
pub fn read_last_forge_path(app: &AppHandle) -> Option<String> {
    let path = resolve_path(app).ok()?;
    load_from(&path).last_forge_path
}

/// Write `path` as the new `last_forge_path` and promote it to the
/// front of `recent_forge_paths` (dedup + cap). Other fields are
/// preserved. Errors are logged and swallowed since persistence drift
/// shouldn't break a successful forge open.
pub fn write_last_forge_path(app: &AppHandle, forge_path: &Path) {
    let Ok(file_path) = resolve_path(app) else {
        tracing::warn!("could not resolve persistence path; skipping forge save");
        return;
    };
    let mut state = load_from(&file_path);
    let path_str = forge_path.to_string_lossy().into_owned();
    state.last_forge_path = Some(path_str.clone());
    state.recent_forge_paths.retain(|p| p != &path_str);
    state.recent_forge_paths.insert(0, path_str);
    state.recent_forge_paths.truncate(MAX_RECENT_FORGES);
    if let Err(err) = save_to(&file_path, &state) {
        tracing::warn!(%err, "failed to persist last_forge_path");
    }
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

    #[test]
    fn last_forge_path_round_trips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = LayoutPersistence::default();
        state.last_forge_path = Some("/some/forge".into());
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.last_forge_path.as_deref(), Some("/some/forge"));
    }

    #[test]
    fn legacy_file_without_forge_path_loads() {
        // Mimic a v1 file written before the field existed.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"version":1,"lastPresetId":"vibe","layouts":{}}"#;
        fs::write(tmp.path(), json).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.last_preset_id.as_deref(), Some("vibe"));
        assert!(loaded.last_forge_path.is_none());
        assert!(loaded.forge_state.is_empty());
    }

    #[test]
    fn recent_forge_paths_round_trips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = LayoutPersistence::default();
        state.recent_forge_paths = vec!["/a".into(), "/b".into()];
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.recent_forge_paths, vec!["/a", "/b"]);
    }

    #[test]
    fn legacy_file_without_recent_forges_loads() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"version":1,"lastPresetId":null,"lastForgePath":"/x","layouts":{}}"#;
        fs::write(tmp.path(), json).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.last_forge_path.as_deref(), Some("/x"));
        assert!(loaded.recent_forge_paths.is_empty());
    }

    #[test]
    fn forge_state_round_trips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = LayoutPersistence::default();
        state.forge_state.insert(
            "/some/forge".into(),
            ForgeUiState {
                expanded_paths: vec!["notes".into(), "notes/sub".into()],
                open_file: Some("notes/hello.md".into()),
            },
        );
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        let entry = &loaded.forge_state["/some/forge"];
        assert_eq!(entry.expanded_paths, vec!["notes", "notes/sub"]);
        assert_eq!(entry.open_file.as_deref(), Some("notes/hello.md"));
    }
}
