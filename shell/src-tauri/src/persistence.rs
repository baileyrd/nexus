//! Shell-side persisted state.
//!
//! Single JSON file at `<app_config_dir>/shell-state.json`, read once at
//! startup and atomically rewritten (tmp → rename). Ported from the
//! legacy shell's `persistence.rs` (retired under Phase 4 WI-37); the
//! file format and helpers carried over 1:1.
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
/// Cap on the remote-connections recents list. Matches `MAX_RECENT_FORGES`
/// but kept as a separate constant so the two limits can drift if needed.
const MAX_REMOTE_RECENTS: usize = 8;

/// A saved remote forge connection. `uri` is the canonical
/// `ssh://user@host:port/path` string that gets handed to
/// `nexus.workspace.openRemote`. `label` is an optional friendly name
/// the launcher surfaces in place of the URI (e.g. "alice@devbox").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteForgeRecent {
    pub uri: String,
    #[serde(default)]
    pub label: Option<String>,
}

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
    /// BL-148 — newest-first list of saved remote (`ssh://...`)
    /// connections, separate from `recent_forge_paths` so the launcher
    /// can render them with their friendly labels.
    #[serde(default)]
    pub remote_forge_recents: Vec<RemoteForgeRecent>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            last_forge_path: None,
            recent_forge_paths: Vec::new(),
            remote_forge_recents: Vec::new(),
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

/// BL-148 — promote a remote forge connection to the front of
/// `remote_forge_recents`. Dedupes on `uri` (an existing entry with the
/// same URI is removed before reinserting at the head, so the most
/// recently supplied `label` wins). Caps at `MAX_REMOTE_RECENTS`.
/// `last_forge_path` is also updated so "restore last forge" works for
/// remote URIs.
#[tauri::command]
pub fn write_remote_recent(
    app: AppHandle,
    uri: String,
    label: Option<String>,
) -> Result<ShellState, String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    let normalized_label = label.and_then(|l| {
        let trimmed = l.trim().to_string();
        if trimmed.is_empty() { None } else { Some(trimmed) }
    });
    state.remote_forge_recents.retain(|r| r.uri != uri);
    state.remote_forge_recents.insert(
        0,
        RemoteForgeRecent {
            uri: uri.clone(),
            label: normalized_label,
        },
    );
    state.remote_forge_recents.truncate(MAX_REMOTE_RECENTS);
    state.last_forge_path = Some(uri);
    save_to(&path, &state)?;
    Ok(state)
}

/// BL-148 — remove a remote connection from `remote_forge_recents` and
/// clear `last_forge_path` if it matches.
#[tauri::command]
pub fn forget_remote_recent(app: AppHandle, uri: String) -> Result<ShellState, String> {
    let path = resolve_path(&app)?;
    let mut state = load_from(&path);
    state.remote_forge_recents.retain(|r| r.uri != uri);
    if state.last_forge_path.as_deref() == Some(uri.as_str()) {
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

    #[test]
    fn remote_recents_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut state = ShellState::default();
        state.remote_forge_recents = vec![
            RemoteForgeRecent {
                uri: "ssh://alice@devbox/srv/forge".into(),
                label: Some("devbox".into()),
            },
            RemoteForgeRecent {
                uri: "ssh://bob@build:2222/var/forge".into(),
                label: None,
            },
        ];
        save_to(tmp.path(), &state).unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.remote_forge_recents, state.remote_forge_recents);
    }

    #[test]
    fn older_state_files_load_without_remote_recents() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            tmp.path(),
            br#"{"version":1,"lastForgePath":"/forge","recentForgePaths":["/forge"]}"#,
        )
        .unwrap();
        let loaded = load_from(tmp.path());
        assert_eq!(loaded.last_forge_path.as_deref(), Some("/forge"));
        assert!(loaded.remote_forge_recents.is_empty());
    }
}
