//! Forge opening + file-tree IPC.
//!
//! `nexus-app` owns the currently-open forge for the Tauri shell. The
//! active forge is resolved at boot from `$NEXUS_FORGE_DIR`, falling back
//! to `<app_config_dir>/default-forge/` which is created on first run so
//! the UI always has something to point at.
//!
//! Directory listing is path-safety checked: requested paths are resolved
//! against the forge root and rejected if they escape it.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// Summary info about the currently-open forge exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInfo {
    /// Display name — the basename of the forge root directory.
    pub name: String,
    /// Absolute path to the forge root.
    pub root: PathBuf,
}

/// One entry in a directory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeDirEntry {
    /// File or directory name (no path separators).
    pub name: String,
    /// Path relative to the forge root, using forward slashes.
    pub relpath: String,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
}

/// A file's contents returned by [`read_forge_file`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeFile {
    /// Path relative to the forge root, using forward slashes.
    pub relpath: String,
    /// File name (no path).
    pub name: String,
    /// UTF-8 file contents.
    pub content: String,
}

/// Maximum file size accepted by [`read_forge_file`] (in bytes).
/// Keeps the read-only viewer from accidentally loading huge files into
/// the renderer. Lifted once the editor has streaming support.
const MAX_FILE_BYTES: u64 = 1_000_000;

/// Tauri-managed handle to the currently-open forge.
pub struct ForgeState(pub Mutex<Option<ForgeInfo>>);

/// Tauri-managed handle that keeps the FS watcher alive for the
/// duration of the app. Dropped on shutdown.
pub struct WatcherHandle(pub Mutex<Option<Debouncer<notify::RecommendedWatcher>>>);

/// Tauri event emitted when any file under the active forge root
/// changes. Frontend listens via `@tauri-apps/api/event`.
pub const FS_CHANGED_EVENT: &str = "forge:fs-changed";

const FORGE_ENV: &str = "NEXUS_FORGE_DIR";
const DEFAULT_FORGE_DIRNAME: &str = "default-forge";

/// Resolve a forge root for this launch and ensure its layout exists.
///
/// Precedence:
/// 1. `$NEXUS_FORGE_DIR` if set.
/// 2. `<app_config_dir>/default-forge/` (created if missing).
pub fn bootstrap(app: &AppHandle) -> Result<ForgeInfo, String> {
    let root = if let Ok(env) = std::env::var(FORGE_ENV) {
        PathBuf::from(env)
    } else {
        app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join(DEFAULT_FORGE_DIRNAME)
    };
    init_layout(&root)?;
    Ok(info_for(&root))
}

/// Start a debounced recursive watcher on `root` that emits
/// [`FS_CHANGED_EVENT`] to the frontend on any change. The returned
/// debouncer must be kept alive (typically stored in [`WatcherHandle`]).
pub fn start_watcher(
    app: AppHandle,
    root: &Path,
) -> Result<Debouncer<notify::RecommendedWatcher>, String> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |res: DebounceEventResult| match res {
            Ok(_events) => {
                if let Err(e) = app.emit(FS_CHANGED_EVENT, ()) {
                    tracing::warn!(%e, "failed to emit forge:fs-changed");
                }
            }
            Err(err) => tracing::warn!(?err, "watcher error"),
        },
    )
    .map_err(|e| e.to_string())?;
    debouncer
        .watcher()
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    Ok(debouncer)
}

fn init_layout(root: &Path) -> Result<(), String> {
    for sub in ["notes", "attachments", ".forge"] {
        fs::create_dir_all(root.join(sub)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn info_for(root: &Path) -> ForgeInfo {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("forge")
        .to_string();
    ForgeInfo {
        name,
        root: root.to_path_buf(),
    }
}

/// Return the currently-open forge, or `None` if bootstrap hasn't run.
#[tauri::command]
pub fn current_forge(state: State<'_, ForgeState>) -> Option<ForgeInfo> {
    state.0.lock().ok().and_then(|g| g.clone())
}

/// Open a forge at `path`, initializing its layout if needed.
#[tauri::command]
pub fn open_forge(path: String, state: State<'_, ForgeState>) -> Result<ForgeInfo, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("not a directory: {path}"));
    }
    init_layout(&root)?;
    let info = info_for(&root);
    *state.0.lock().map_err(|_| "forge state poisoned")? = Some(info.clone());
    Ok(info)
}

/// List entries under `relpath` within the active forge root.
///
/// `relpath` is relative to the forge root and uses `/` as a separator.
/// An empty string lists the root itself. The `.forge/` internal
/// directory is hidden from results.
#[tauri::command]
pub fn list_forge_dir(
    relpath: String,
    state: State<'_, ForgeState>,
) -> Result<Vec<ForgeDirEntry>, String> {
    let forge = state
        .0
        .lock()
        .map_err(|_| "forge state poisoned")?
        .clone()
        .ok_or("no forge open")?;
    let target = resolve_within(&forge.root, &relpath)?;

    let mut entries: Vec<ForgeDirEntry> = Vec::new();
    for entry in fs::read_dir(&target).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let Ok(ft) = entry.file_type() else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if relpath.is_empty() && name == ".forge" {
            continue;
        }
        let rel = if relpath.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", relpath.trim_end_matches('/'), name)
        };
        entries.push(ForgeDirEntry {
            name,
            relpath: rel,
            is_dir: ft.is_dir(),
        });
    }

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

/// Read a single file from the active forge. Refuses non-files,
/// non-UTF-8 contents, and files larger than [`MAX_FILE_BYTES`].
#[tauri::command]
pub fn read_forge_file(
    relpath: String,
    state: State<'_, ForgeState>,
) -> Result<ForgeFile, String> {
    let forge = state
        .0
        .lock()
        .map_err(|_| "forge state poisoned")?
        .clone()
        .ok_or("no forge open")?;
    let target = resolve_within(&forge.root, &relpath)?;
    let meta = fs::metadata(&target).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("not a file: {relpath}"));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "file too large ({} bytes; limit {MAX_FILE_BYTES})",
            meta.len()
        ));
    }
    let bytes = fs::read(&target).map_err(|e| e.to_string())?;
    let content = String::from_utf8(bytes).map_err(|_| "file is not UTF-8")?;
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    Ok(ForgeFile {
        relpath,
        name,
        content,
    })
}

/// Resolve `relpath` against `root`, rejecting anything that escapes the
/// root (via `..`, absolute paths, or symlink traversal after canonicalize).
fn resolve_within(root: &Path, relpath: &str) -> Result<PathBuf, String> {
    let candidate = if relpath.is_empty() {
        root.to_path_buf()
    } else {
        let rel = Path::new(relpath);
        for c in rel.components() {
            match c {
                Component::Normal(_) => {}
                _ => return Err(format!("invalid relpath: {relpath}")),
            }
        }
        root.join(rel)
    };
    let canon_root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let canon = fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("path escapes forge root: {relpath}"));
    }
    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_layout_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        init_layout(tmp.path()).unwrap();
        init_layout(tmp.path()).unwrap();
        assert!(tmp.path().join("notes").is_dir());
        assert!(tmp.path().join("attachments").is_dir());
        assert!(tmp.path().join(".forge").is_dir());
    }

    #[test]
    fn info_for_uses_basename_as_name() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("my-forge");
        fs::create_dir_all(&child).unwrap();
        let info = info_for(&child);
        assert_eq!(info.name, "my-forge");
        assert_eq!(info.root, child);
    }

    #[test]
    fn resolve_within_rejects_parent_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        init_layout(tmp.path()).unwrap();
        let err = resolve_within(tmp.path(), "../outside").unwrap_err();
        assert!(err.contains("invalid relpath"), "got: {err}");
    }

    #[test]
    fn resolve_within_rejects_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        init_layout(tmp.path()).unwrap();
        let err = resolve_within(tmp.path(), "/etc/passwd").unwrap_err();
        assert!(err.contains("invalid relpath"), "got: {err}");
    }

    #[test]
    fn resolve_within_accepts_nested_relpath() {
        let tmp = tempfile::tempdir().unwrap();
        init_layout(tmp.path()).unwrap();
        let sub = tmp.path().join("notes/sub");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_within(tmp.path(), "notes/sub").unwrap();
        assert_eq!(resolved, fs::canonicalize(&sub).unwrap());
    }

    #[test]
    fn resolve_within_empty_returns_root() {
        let tmp = tempfile::tempdir().unwrap();
        init_layout(tmp.path()).unwrap();
        let resolved = resolve_within(tmp.path(), "").unwrap();
        assert_eq!(resolved, fs::canonicalize(tmp.path()).unwrap());
    }
}
