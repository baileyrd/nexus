//! Editor core plugin: kernel-IPC surface for in-memory editor sessions.
//!
//! Registers as [`PLUGIN_ID`] (`com.nexus.editor`). Each loaded
//! markdown document lives in an [`Session`] keyed by forge-relative
//! path; mutations go through serialized [`crate::Transaction`]s so
//! the plugin can own the authoritative block tree + undo history for
//! every consumer (Tauri UI, CLI, AI, MCP).
//!
//! Consumers call this plugin via
//! [`nexus_kernel::PluginContext::ipc_call`]; see
//! [`crates/nexus-bootstrap/src/lib.rs`](../../nexus-bootstrap/src/lib.rs)
//! for the command-id → handler-id mapping registered at boot.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::UndoTree;

/// Plugin id of the storage core plugin — the target of the editor's
/// IPC `read_file`/`write_file` calls.
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

/// Per-call timeout for storage IPC. File I/O is local; a generous
/// bound is safe.
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.editor";

// ── IPC handler ids ──────────────────────────────────────────────────────────
//
// These are stable within the plugin — the manifest in nexus-bootstrap maps
// command ids to these numbers. If you add a handler, append; never reuse a
// retired id.

/// Handler id for `open`. Args: `{ "relpath": String }`; Returns: [`EditorSnapshot`].
pub const HANDLER_OPEN: u32 = 1;
/// Handler id for `close`. Args: `{ "relpath": String }`; Returns: `{}`.
pub const HANDLER_CLOSE: u32 = 2;
/// Handler id for `get_tree`. Args: `{ "relpath": String }`; Returns: [`EditorSnapshot`].
pub const HANDLER_GET_TREE: u32 = 3;
/// Handler id for `save`. Args: `{ "relpath": String }`; Returns: `{}`.
pub const HANDLER_SAVE: u32 = 4;
/// Handler id for `apply_transaction`. Args: `{ "relpath": String, "transaction": Transaction }`; Returns: [`EditorSnapshot`].
pub const HANDLER_APPLY_TRANSACTION: u32 = 5;
/// Handler id for `undo`. Args: `{ "relpath": String }`; Returns: [`EditorSnapshot`].
pub const HANDLER_UNDO: u32 = 6;
/// Handler id for `redo`. Args: `{ "relpath": String }`; Returns: [`EditorSnapshot`].
pub const HANDLER_REDO: u32 = 7;
/// Handler id for `list_open`. Args: `{}`; Returns: `Vec<String>`.
pub const HANDLER_LIST_OPEN: u32 = 8;
/// Handler id for `sync_content`. Args: `{ "relpath": String, "content": String }`; Returns: `{}`.
///
/// Parses `content` and replaces the in-memory block tree for the session
/// identified by `relpath`. If no session exists for that path, one is
/// created. The undo tree is left untouched — this is a background resync
/// for read-only consumers (AI, MCP, outline), not a user transaction.
pub const HANDLER_SYNC_CONTENT: u32 = 9;

// ── Wire types ───────────────────────────────────────────────────────────────

/// Snapshot of an open editor session, suitable for IPC return.
///
/// The tree is returned in full — delta snapshots are a follow-up
/// optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSnapshot {
    /// Forge-relative path of the session.
    pub relpath: String,
    /// Full block tree.
    pub tree: BlockTree,
    /// Current `UndoTree` cursor. `None` means "at the virtual root"
    /// (no transactions yet, or fully-undone state).
    pub undo_position: Option<usize>,
    /// Total number of transactions recorded in history.
    pub undo_len: usize,
    /// `true` if `undo` would produce a state change.
    pub can_undo: bool,
    /// `true` if `redo` would produce a state change.
    pub can_redo: bool,
}

// ── Plugin state ─────────────────────────────────────────────────────────────

/// A single open document.
struct Session {
    tree: BlockTree,
    undo: UndoTree,
    relpath: String,
}

/// Editor core plugin.
///
/// Mirrors the structure of
/// [`nexus_storage::StorageCorePlugin`](../../nexus-storage/src/core_plugin.rs):
/// a single `Mutex`-guarded session map, locked briefly per IPC call.
///
/// `open` and `save` route their disk I/O through `com.nexus.storage` via
/// [`KernelPluginContext::ipc_call`] when a context has been wired in by the
/// bootstrap. That keeps the editor inside the kernel's capability /
/// atomic-write envelope rather than touching `std::fs` directly. Sync
/// dispatch retains a local-filesystem fallback so unit tests can exercise
/// the plugin without assembling a full runtime.
pub struct EditorCorePlugin {
    forge_root: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Plugin-facing kernel context. Installed via
    /// [`CorePlugin::wire_context`] once the bootstrap has the shared
    /// dispatcher assembled; `None` for sync-only test drivers.
    context: Option<Arc<KernelPluginContext>>,
}

impl EditorCorePlugin {
    /// Create a new plugin rooted at `forge_root`.
    #[must_use]
    pub fn new(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            context: None,
        }
    }
}

impl CorePlugin for EditorCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        if !self.forge_root.exists() {
            return Err(PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_init".to_string(),
                reason: format!("forge root '{}' does not exist", self.forge_root.display()),
            });
        }
        tracing::debug!(
            plugin = PLUGIN_ID,
            forge_root = %self.forge_root.display(),
            "editor core plugin initialized"
        );
        Ok(())
    }

    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_OPEN => handle_open_sync(&self.forge_root, &self.sessions, args),
            HANDLER_CLOSE => handle_close(&self.sessions, args),
            HANDLER_GET_TREE => handle_get_tree(&self.sessions, args),
            HANDLER_SAVE => handle_save_sync(&self.forge_root, &self.sessions, args),
            HANDLER_APPLY_TRANSACTION => handle_apply_transaction(&self.sessions, args),
            HANDLER_UNDO => handle_undo(&self.sessions, args),
            HANDLER_REDO => handle_redo(&self.sessions, args),
            HANDLER_LIST_OPEN => handle_list_open(&self.sessions),
            HANDLER_SYNC_CONTENT => handle_sync_content(&self.sessions, args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    /// Async path for handlers that route disk I/O through the storage
    /// plugin. Everything other than `open` / `save` is synchronous and
    /// short-circuits to the sync path via `dispatch` in the kernel's
    /// fallback. Returns `None` for those so the kernel's own async shim
    /// doesn't have to allocate a future.
    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        match handler_id {
            HANDLER_OPEN | HANDLER_SAVE => {}
            _ => return None,
        }

        // Capture everything the future needs by value / Arc so nothing
        // outlives the `&mut self` borrow.
        let forge_root = self.forge_root.clone();
        let sessions = Arc::clone(&self.sessions);
        let ctx = self.context.clone();
        let args = args.clone();

        Some(Box::pin(async move {
            match handler_id {
                HANDLER_OPEN => handle_open_async(&forge_root, sessions, ctx, &args).await,
                HANDLER_SAVE => handle_save_async(&forge_root, sessions, ctx, &args).await,
                _ => Err(exec_err(format!("unknown async handler id {handler_id}"))),
            }
        }))
    }

    /// Capture the plugin-facing kernel context so async `open` / `save`
    /// handlers can issue nested `ipc_call`s into `com.nexus.storage`.
    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ── Handler implementations ──────────────────────────────────────────────────

/// Build a new session from already-loaded source text and insert it
/// into the session map. Shared tail of the sync + async `open` paths.
fn finish_open(
    sessions: &Mutex<HashMap<String, Session>>,
    relpath: String,
    source: &str,
) -> Result<Value, PluginError> {
    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(source)
        .map_err(|e| exec_err(format!("open: parse '{relpath}': {e}")))?;

    let session = Session {
        tree,
        undo: UndoTree::new(),
        relpath: relpath.clone(),
    };
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    guard.insert(relpath.clone(), session);
    let s = guard.get(&relpath).expect("just inserted");
    snapshot_to_value(&snapshot_of(s), "open")
}

fn handle_open_sync(
    forge_root: &Path,
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "open")?;
    let abs = resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("open: {e}")))?;
    let source = fs::read_to_string(&abs)
        .map_err(|e| exec_err(format!("open: read '{}': {e}", abs.display())))?;
    finish_open(sessions, relpath, &source)
}

async fn handle_open_async(
    forge_root: &Path,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "open")?;

    let source = if let Some(ctx) = ctx.as_deref() {
        // Preferred path: fetch through `com.nexus.storage` so capability
        // checks, atomic-write audit, and future observability hooks all
        // cover editor reads.
        #[derive(Deserialize)]
        struct Resp {
            bytes: Vec<u8>,
        }
        let value = ctx
            .ipc_call(
                STORAGE_PLUGIN_ID,
                "read_file",
                serde_json::json!({ "path": relpath }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| exec_err(format!("open: storage.read_file: {e}")))?;
        let resp: Resp = serde_json::from_value(value)
            .map_err(|e| exec_err(format!("open: storage.read_file decode: {e}")))?;
        String::from_utf8(resp.bytes)
            .map_err(|_| exec_err(format!("open: '{relpath}' is not UTF-8")))?
    } else {
        // Fallback used only when no context has been wired (unit tests
        // that drive the plugin directly without a runtime).
        let abs =
            resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("open: {e}")))?;
        fs::read_to_string(&abs)
            .map_err(|e| exec_err(format!("open: read '{}': {e}", abs.display())))?
    };

    finish_open(&sessions, relpath, &source)
}

fn handle_close(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "close")?;
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    guard.remove(&relpath);
    Ok(serde_json::json!({}))
}

fn handle_get_tree(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_tree")?;
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard
        .get(&relpath)
        .ok_or_else(|| exec_err(format!("get_tree: no open session for '{relpath}'")))?;
    snapshot_to_value(&snapshot_of(s), "get_tree")
}

/// Serialize the session's block tree to markdown under the session lock.
fn serialize_session(
    sessions: &Mutex<HashMap<String, Session>>,
    relpath: &str,
) -> Result<String, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard
        .get(relpath)
        .ok_or_else(|| exec_err(format!("save: no open session for '{relpath}'")))?;
    Ok(MarkdownSerializer::serialize(&s.tree))
}

fn handle_save_sync(
    forge_root: &Path,
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let markdown = serialize_session(sessions, &relpath)?;
    let abs = resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("save: {e}")))?;
    atomic_write(&abs, &markdown)
        .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
    Ok(serde_json::json!({}))
}

async fn handle_save_async(
    forge_root: &Path,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let markdown = serialize_session(&sessions, &relpath)?;

    if let Some(ctx) = ctx.as_deref() {
        // Canonical path: storage's `write_file` does temp + fsync +
        // rename and updates the SQLite index atomically with the disk
        // write so a later `open` sees consistent state.
        ctx.ipc_call(
            STORAGE_PLUGIN_ID,
            "write_file",
            serde_json::json!({ "path": relpath, "bytes": markdown.as_bytes() }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("save: storage.write_file: {e}")))?;
        Ok(serde_json::json!({}))
    } else {
        // Fallback for context-less unit tests.
        let abs =
            resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("save: {e}")))?;
        atomic_write(&abs, &markdown)
            .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
        Ok(serde_json::json!({}))
    }
}

fn handle_apply_transaction(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "apply_transaction")?;
    let tx_value = args
        .get("transaction")
        .ok_or_else(|| exec_err("apply_transaction: missing 'transaction'".to_string()))?
        .clone();
    let tx: crate::Transaction = serde_json::from_value(tx_value)
        .map_err(|e| exec_err(format!("apply_transaction: invalid transaction: {e}")))?;

    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard.get_mut(&relpath).ok_or_else(|| {
        exec_err(format!(
            "apply_transaction: no open session for '{relpath}'"
        ))
    })?;
    s.undo
        .execute(tx, &mut s.tree)
        .map_err(|e| exec_err(format!("apply_transaction: {e}")))?;
    snapshot_to_value(&snapshot_of(s), "apply_transaction")
}

fn handle_undo(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "undo")?;
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard
        .get_mut(&relpath)
        .ok_or_else(|| exec_err(format!("undo: no open session for '{relpath}'")))?;
    s.undo
        .undo(&mut s.tree)
        .map_err(|e| exec_err(format!("undo: {e}")))?;
    snapshot_to_value(&snapshot_of(s), "undo")
}

fn handle_redo(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "redo")?;
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard
        .get_mut(&relpath)
        .ok_or_else(|| exec_err(format!("redo: no open session for '{relpath}'")))?;
    s.undo
        .redo(&mut s.tree)
        .map_err(|e| exec_err(format!("redo: {e}")))?;
    snapshot_to_value(&snapshot_of(s), "redo")
}

fn handle_list_open(sessions: &Mutex<HashMap<String, Session>>) -> Result<Value, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let mut paths: Vec<String> = guard.keys().cloned().collect();
    paths.sort();
    serde_json::to_value(paths).map_err(|e| exec_err(format!("list_open: serialize: {e}")))
}

/// Re-parse `content` and update (or create) the block tree for `relpath`.
///
/// The undo history is left untouched: `sync_content` is a background resync
/// for read-only consumers (AI, MCP, outline), not a user-visible transaction.
fn handle_sync_content(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "sync_content")?;
    let content = args["content"]
        .as_str()
        .ok_or_else(|| exec_err("sync_content: missing 'content'".to_string()))?;

    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(content)
        .map_err(|e| exec_err(format!("sync_content: parse '{relpath}': {e}")))?;

    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let session = guard.entry(relpath.clone()).or_insert_with(|| Session {
        tree: BlockTree::default(),
        undo: UndoTree::new(),
        relpath: relpath.clone(),
    });
    session.tree = tree;

    Ok(serde_json::json!({}))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn snapshot_of(s: &Session) -> EditorSnapshot {
    let undo_len = s.undo.len();
    let undo_position = s.undo.current();
    // `can_undo`: there is a current transaction to reverse.
    let can_undo = undo_position.is_some();
    // `can_redo`: the current node has at least one recorded child branch.
    let can_redo = !s.undo.children_of(undo_position).is_empty();
    EditorSnapshot {
        relpath: s.relpath.clone(),
        tree: s.tree.clone(),
        undo_position,
        undo_len,
        can_undo,
        can_redo,
    }
}

fn snapshot_to_value(snapshot: &EditorSnapshot, command: &str) -> Result<Value, PluginError> {
    serde_json::to_value(snapshot)
        .map_err(|e| exec_err(format!("{command}: serialize snapshot: {e}")))
}

fn relpath_arg(args: &Value, command: &str) -> Result<String, PluginError> {
    args.get("relpath")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("{command}: missing 'relpath' string")))
}

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn sessions_poisoned() -> PluginError {
    exec_err("sessions lock poisoned".to_string())
}

/// Resolve `relpath` under `forge_root`, rejecting traversal and
/// absolute paths, then canonicalize.
///
/// Delegates component validation to
/// [`nexus_types::paths::resolve_within`] so every core plugin shares
/// the same path-confinement code path. This wrapper adds a
/// `canonicalize` pass (production file I/O routes through
/// `com.nexus.storage`; this is only used by the context-less unit-test
/// fallback in [`handle_open_sync`] / [`handle_save_sync`]). Rejects
/// empty relpaths — the sync fallback always addresses a specific file.
fn resolve_within(root: &Path, relpath: &str) -> Result<PathBuf, String> {
    if relpath.is_empty() {
        return Err("empty relpath".into());
    }
    let candidate = nexus_types::paths::resolve_within(root, relpath)
        .map_err(|e| e.to_string())?;
    let canon_root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let canon = fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("path escapes forge root: {relpath}"));
    }
    Ok(canon)
}

/// Write `contents` to `path` via a sibling `.tmp` + fsync + rename.
///
/// Only used when the plugin is driven without a [`KernelPluginContext`]
/// (unit tests); production saves route through `com.nexus.storage` via
/// [`handle_save_async`] and get its fuller atomic-write guarantees.
/// Even here we fsync the temp file (so a crash between write and
/// rename never leaves a half-flushed file visible via the rename) —
/// the pre-refactor version skipped the fsync entirely.
///
/// Parent-directory fsync is best-effort: `File::sync_all` on a
/// directory is a no-op on Windows but persists the rename on POSIX.
fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write as _;

    let parent = path
        .parent()
        .ok_or_else(|| format!("no parent dir for '{}'", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("no filename in '{}'", path.display()))?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));

    // Write + flush + fsync the temp file.
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }

    // Atomic rename into place.
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;

    // Best-effort directory fsync so the rename itself is durable.
    // Silently ignore failures — Windows returns an error when opening
    // a directory for writing, and on POSIX the worst case is that the
    // rename is replayed by the filesystem journal anyway.
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_forge() -> (TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".forge")).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        (tmp, root)
    }

    fn write_note(root: &Path, relpath: &str, body: &str) {
        let abs = root.join(relpath);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(abs, body).unwrap();
    }

    fn new_plugin(root: PathBuf) -> EditorCorePlugin {
        let mut p = EditorCorePlugin::new(root);
        p.on_init().unwrap();
        p
    }

    #[test]
    fn open_parses_and_stores_session() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "# Hi\n\nBody\n");
        let mut p = new_plugin(root);
        let args = serde_json::json!({ "relpath": "notes/a.md" });
        let resp = p.dispatch(HANDLER_OPEN, &args).unwrap();
        let snapshot: EditorSnapshot = serde_json::from_value(resp).unwrap();
        assert_eq!(snapshot.relpath, "notes/a.md");
        assert_eq!(snapshot.tree.root_blocks.len(), 2);
        assert_eq!(snapshot.undo_len, 0);
        assert!(!snapshot.can_undo);
        assert!(!snapshot.can_redo);
    }

    #[test]
    fn open_rejects_path_escape() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        let args = serde_json::json!({ "relpath": "../outside.md" });
        let err = p.dispatch(HANDLER_OPEN, &args).unwrap_err();
        assert!(format!("{err}").contains("invalid relpath"), "got: {err}");
    }

    #[test]
    fn open_missing_file_errors() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        let args = serde_json::json!({ "relpath": "notes/missing.md" });
        assert!(p.dispatch(HANDLER_OPEN, &args).is_err());
    }

    #[test]
    fn open_twice_replaces_prior_session() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "first\n");
        let mut p = new_plugin(root.clone());
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        // Overwrite on disk and re-open.
        write_note(&root, "notes/a.md", "second\n");
        let resp = p
            .dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap();
        let snap: EditorSnapshot = serde_json::from_value(resp).unwrap();
        let root_id = snap.tree.root_blocks[0];
        assert_eq!(snap.tree.blocks[&root_id].content, "second");
    }

    #[test]
    fn get_tree_returns_fresh_snapshot() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "x\n");
        let mut p = new_plugin(root);
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        let resp = p
            .dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap();
        let snap: EditorSnapshot = serde_json::from_value(resp).unwrap();
        assert_eq!(snap.undo_position, None);
    }

    #[test]
    fn get_tree_on_unopen_errors() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        assert!(p
            .dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": "never-opened.md" }),
            )
            .is_err());
    }

    #[test]
    fn save_writes_roundtripped_markdown() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "# Hello\n\nWorld\n");
        let mut p = new_plugin(root.clone());
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_SAVE,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        let on_disk = fs::read_to_string(root.join("notes/a.md")).unwrap();
        // Should still contain the heading and body (normalized form).
        assert!(on_disk.contains("# Hello"));
        assert!(on_disk.contains("World"));
    }

    #[test]
    fn apply_transaction_records_undo_history() {
        use crate::{Annotation, AnnotationType, Operation, Transaction, TransactionMetadata};
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "Hello\n");
        let mut p = new_plugin(root);

        let resp = p
            .dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap();
        let snap: EditorSnapshot = serde_json::from_value(resp).unwrap();
        let para_id = snap.tree.root_blocks[0];
        let content_len = snap.tree.blocks[&para_id].content.len();

        let pre_anns: Vec<Annotation> = Vec::new();
        let tx = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: content_len,
                text: " world".into(),
                pre_annotations: pre_anns.clone(),
            }],
            TransactionMetadata::default(),
        );
        let _ = AnnotationType::Bold; // ensure the re-export is reachable

        let tx_value = serde_json::to_value(&tx).unwrap();
        let resp = p
            .dispatch(
                HANDLER_APPLY_TRANSACTION,
                &serde_json::json!({ "relpath": "notes/a.md", "transaction": tx_value }),
            )
            .unwrap();
        let snap: EditorSnapshot = serde_json::from_value(resp).unwrap();
        assert_eq!(snap.undo_len, 1);
        assert_eq!(snap.undo_position, Some(0));
        assert!(snap.can_undo);
        assert!(!snap.can_redo);
        assert_eq!(snap.tree.blocks[&para_id].content, "Hello world");
    }

    #[test]
    fn undo_redo_cycle() {
        use crate::{Operation, Transaction, TransactionMetadata};
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "a\n");
        let mut p = new_plugin(root);

        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let para_id = snap.tree.root_blocks[0];

        let tx = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: 1,
                text: "b".into(),
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        p.dispatch(
            HANDLER_APPLY_TRANSACTION,
            &serde_json::json!({ "relpath": "notes/a.md", "transaction": serde_json::to_value(&tx).unwrap() }),
        )
        .unwrap();

        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_UNDO,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snap.undo_position, None);
        assert!(snap.can_redo);
        assert_eq!(snap.tree.blocks[&para_id].content, "a");

        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_REDO,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snap.undo_position, Some(0));
        assert!(!snap.can_redo);
        assert_eq!(snap.tree.blocks[&para_id].content, "ab");
    }

    #[test]
    fn close_drops_session() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "x\n");
        let mut p = new_plugin(root);
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_CLOSE,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        assert!(p
            .dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .is_err());
    }

    #[test]
    fn list_open_reflects_session_map() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "a\n");
        write_note(&root, "notes/b.md", "b\n");
        let mut p = new_plugin(root);
        let empty = p
            .dispatch(HANDLER_LIST_OPEN, &serde_json::json!({}))
            .unwrap();
        assert_eq!(empty, serde_json::json!([] as [String; 0]));
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/b.md" }),
        )
        .unwrap();
        let both = p
            .dispatch(HANDLER_LIST_OPEN, &serde_json::json!({}))
            .unwrap();
        let paths: Vec<String> = serde_json::from_value(both).unwrap();
        assert_eq!(paths, vec!["notes/a.md".to_string(), "notes/b.md".into()]);
    }

    #[test]
    fn unknown_handler_id_errors() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        let err = p.dispatch(999, &serde_json::json!({})).unwrap_err();
        assert!(format!("{err}").contains("unknown handler id 999"));
    }
}
