//! Forge opening + file-tree IPC.
//!
//! `nexus-app` owns the currently-open forge for the Tauri shell. The
//! active forge is resolved at boot from `$NEXUS_FORGE_DIR`, falling back
//! to `<app_config_dir>/default-forge/` which is created on first run so
//! the UI always has something to point at.
//!
//! File-tree CRUD commands (list/read/write/create/rename/delete) route
//! through `com.nexus.storage` via [`crate::editor::KernelRuntime`]'s
//! `ipc_call`. The Tauri shell does not touch `std::fs` for forge tree
//! operations — all I/O goes through the storage plugin so capability
//! checks, atomic writes, and audit hooks apply uniformly.

#![allow(
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use nexus_kernel::PluginContext;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::editor::KernelRuntime;

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
/// the renderer.
const MAX_FILE_BYTES: usize = 1_000_000;

/// Plugin id that owns the file-tree IPC handlers.
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

/// Per-call timeout for forge IPC. Forge tree ops touch the disk but not
/// the network, so a generous bound is safe.
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Tauri-managed handle to the currently-open forge.
pub struct ForgeState(pub Mutex<Option<ForgeInfo>>);

const FORGE_ENV: &str = "NEXUS_FORGE_DIR";
const DEFAULT_FORGE_DIRNAME: &str = "default-forge";

/// Resolve a forge root for this launch and ensure its layout exists.
///
/// Precedence:
/// 1. `$NEXUS_FORGE_DIR` if set (dev override; always wins).
/// 2. The path the user picked last time, if it still exists as a dir.
/// 3. `<app_config_dir>/default-forge/` (created if missing).
pub fn bootstrap(app: &AppHandle) -> Result<ForgeInfo, String> {
    let root = if let Ok(env) = std::env::var(FORGE_ENV) {
        PathBuf::from(env)
    } else if let Some(saved) = crate::persistence::read_last_forge_path(app)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    {
        saved
    } else {
        app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join(DEFAULT_FORGE_DIRNAME)
    };
    init_layout(&root)?;
    Ok(info_for(&root))
}

/// Create the forge's directory skeleton and run `StorageEngine::init`
/// (idempotent — creates the SQLite schema, search index, etc. on first
/// run; just verifies presence on subsequent runs). Without this the
/// storage plugin's `on_init` fails because `StorageEngine::open` refuses
/// to open an uninitialized forge.
fn init_layout(root: &Path) -> Result<(), String> {
    for sub in ["notes", "attachments", ".forge"] {
        fs::create_dir_all(root.join(sub)).map_err(|e| e.to_string())?;
    }
    nexus_bootstrap::init_forge(root).map_err(|e| format!("{e:#}"))?;
    seed_if_empty(root).map_err(|e| e.to_string())?;
    Ok(())
}

/// Write starter notes if `notes/` is empty, so the Files tree has
/// something to show on first launch. Skipped when the directory is
/// missing or already populated, so pointing Nexus at an existing
/// knowledge base never overwrites user content.
fn seed_if_empty(root: &Path) -> std::io::Result<()> {
    let notes = root.join("notes");
    let mut entries = match fs::read_dir(&notes) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if entries.next().is_some() {
        // Already has content — leave it alone.
        return Ok(());
    }

    for (relpath, body) in seed_files() {
        let abs = notes.join(relpath);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(abs, body)?;
    }
    Ok(())
}

/// Seed file set — paths relative to `notes/`. Each file exercises a
/// different formatter feature (wikilinks, tables, callouts, tasks,
/// code blocks) so the editor surface has something meaningful to
/// render on a fresh forge.
fn seed_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Welcome.md", include_str!("../seeds/Welcome.md")),
        ("Quick start.md", include_str!("../seeds/Quick start.md")),
        ("Ideas/Tasks.md", include_str!("../seeds/Tasks.md")),
        ("Ideas/Reading list.md", include_str!("../seeds/Reading list.md")),
        ("Design/Architecture.md", include_str!("../seeds/Architecture.md")),
        ("Design/Decision log.md", include_str!("../seeds/Decision log.md")),
    ]
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
///
/// Emits `forge:fs-changed` immediately so cached file-tree listings
/// invalidate before the storage plugin's watcher fires its first event
/// from the new root.
#[tauri::command]
pub fn open_forge(
    path: String,
    app: AppHandle,
    state: State<'_, ForgeState>,
) -> Result<ForgeInfo, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("not a directory: {path}"));
    }
    init_layout(&root)?;
    let info = info_for(&root);

    *state.0.lock().map_err(|_| "forge state poisoned")? = Some(info.clone());
    crate::persistence::write_last_forge_path(&app, &info.root);
    // Nudge the frontend so any cached listings invalidate immediately.
    let _ = app.emit(crate::FS_CHANGED_EVENT, ());
    Ok(info)
}

// ── IPC adapters ─────────────────────────────────────────────────────────────
//
// Each command is a thin adapter: it serializes its args into JSON and calls
// `nexus_kernel::PluginContext::ipc_call` on the kernel runtime held in
// Tauri state. The target plugin is `com.nexus.storage`; handler ids live
// in `nexus_storage::core_plugin`.

async fn call_storage(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(STORAGE_PLUGIN_ID, command, args, IPC_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Load a `.bases` directory into a full [`nexus_types::bases::Base`]
/// — schema + records + views + relations + metadata — for the
/// view renderers to consume. Read-only; see
/// [`save_forge_base`] for the write path.
///
/// Definition lives below (next to `save_forge_base`) so both commands
/// share the `resolve_in_forge` traversal guard. The function reads
/// `schema.json` / `records.json` / `views.toml` directly from disk
/// via [`nexus_types::bases::load_base`] rather than round-tripping
/// through the storage plugin — the SQLite index only tracks
/// `.bases` directory presence, not per-record contents.

/// List entries under `relpath` within the active forge root.
///
/// `relpath` is relative to the forge root and uses `/` as a separator.
/// An empty string lists the root itself. The `.forge/` internal
/// directory is hidden from results.
#[tauri::command]
pub async fn list_forge_dir(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<Vec<ForgeDirEntry>, String> {
    let value = call_storage(
        runtime,
        "list_dir",
        serde_json::json!({ "relpath": relpath }),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| format!("list_dir: decode failed: {e}"))
}

/// Read a single file from the active forge. Refuses non-UTF-8 contents
/// and files larger than [`MAX_FILE_BYTES`].
#[tauri::command]
pub async fn read_forge_file(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<ForgeFile, String> {
    #[derive(Deserialize)]
    struct Resp {
        bytes: Vec<u8>,
    }
    let value = call_storage(runtime, "read_file", serde_json::json!({ "path": relpath })).await?;
    let resp: Resp =
        serde_json::from_value(value).map_err(|e| format!("read_file: decode failed: {e}"))?;
    if resp.bytes.len() > MAX_FILE_BYTES {
        return Err(format!(
            "file too large ({} bytes; limit {MAX_FILE_BYTES})",
            resp.bytes.len()
        ));
    }
    let content = String::from_utf8(resp.bytes).map_err(|_| "file is not UTF-8")?;
    let name = Path::new(&relpath)
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

/// Load a `.bases` directory at `relpath` (forge-relative) into a
/// full [`nexus_types::bases::Base`]. Read-only — does not touch the
/// storage engine's SQLite index. Path traversal is blocked by the
/// [`resolve_in_forge`] helper: absolute paths and `..` components
/// are rejected before hitting disk.
#[tauri::command]
pub fn load_forge_base(
    relpath: String,
    state: State<'_, ForgeState>,
) -> Result<nexus_types::bases::Base, String> {
    let abs = resolve_in_forge(&state, &relpath)?;
    nexus_types::bases::load_base(&abs).map_err(|e| e.to_string())
}

/// Persist a [`nexus_types::bases::Base`] to the `.bases` directory
/// at `relpath`. Overwrites schema / records / views / relations /
/// metadata files atomically-ish (each file writes independently;
/// use the storage layer's atomic writer in a follow-up).
#[tauri::command]
pub fn save_forge_base(
    relpath: String,
    base: nexus_types::bases::Base,
    state: State<'_, ForgeState>,
) -> Result<(), String> {
    let abs = resolve_in_forge(&state, &relpath)?;
    nexus_types::bases::save_base(&abs, &base).map_err(|e| e.to_string())
}

/// Resolve a forge-relative path to an absolute path, rejecting
/// traversal attempts. Shared by [`load_forge_base`] /
/// [`save_forge_base`] so both commands have identical sandbox
/// semantics.
fn resolve_in_forge(state: &State<'_, ForgeState>, relpath: &str) -> Result<PathBuf, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "forge state lock poisoned".to_string())?;
    let root = guard
        .as_ref()
        .map(|info| info.root.clone())
        .ok_or_else(|| "no forge open".to_string())?;
    drop(guard);
    let rel = Path::new(relpath);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("refusing path traversal in relpath '{relpath}'"));
    }
    Ok(root.join(rel))
}

/// Write content to a file within the active forge. Uses the storage
/// plugin's atomic write (temp file → fsync → rename).
#[tauri::command]
pub async fn write_forge_file(
    relpath: String,
    content: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    let bytes = content.into_bytes();
    let _ = call_storage(
        runtime,
        "write_file",
        serde_json::json!({ "path": relpath, "bytes": bytes }),
    )
    .await?;
    Ok(())
}

/// Create a new empty file at `relpath` within the active forge.
/// Refuses to overwrite an existing file.
#[tauri::command]
pub async fn create_forge_file(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    let _ = call_storage(
        runtime,
        "create_file",
        serde_json::json!({ "relpath": relpath }),
    )
    .await?;
    Ok(())
}

/// Create a new empty directory at `relpath`.
#[tauri::command]
pub async fn create_forge_dir(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    let _ = call_storage(
        runtime,
        "create_dir",
        serde_json::json!({ "relpath": relpath }),
    )
    .await?;
    Ok(())
}

/// Rename or move an entry within the forge. Both `from` and `to` must
/// resolve under the forge root; `to` must not already exist.
#[tauri::command]
pub async fn rename_forge_entry(
    from: String,
    to: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    let _ = call_storage(
        runtime,
        "rename_entry",
        serde_json::json!({ "from": from, "to": to }),
    )
    .await?;
    Ok(())
}

/// Delete an entry within the forge. Files and directories are both
/// accepted; directories are removed recursively.
#[tauri::command]
pub async fn delete_forge_entry(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<(), String> {
    let _ = call_storage(
        runtime,
        "delete_entry",
        serde_json::json!({ "relpath": relpath }),
    )
    .await?;
    Ok(())
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
}
