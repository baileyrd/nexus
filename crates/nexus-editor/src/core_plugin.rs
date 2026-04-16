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
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::UndoTree;

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
pub struct EditorCorePlugin {
    forge_root: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
}

impl EditorCorePlugin {
    /// Create a new plugin rooted at `forge_root`.
    #[must_use]
    pub fn new(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            sessions: Mutex::new(HashMap::new()),
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
            HANDLER_OPEN => handle_open(&self.forge_root, &self.sessions, args),
            HANDLER_CLOSE => handle_close(&self.sessions, args),
            HANDLER_GET_TREE => handle_get_tree(&self.sessions, args),
            HANDLER_SAVE => handle_save(&self.forge_root, &self.sessions, args),
            HANDLER_APPLY_TRANSACTION => handle_apply_transaction(&self.sessions, args),
            HANDLER_UNDO => handle_undo(&self.sessions, args),
            HANDLER_REDO => handle_redo(&self.sessions, args),
            HANDLER_LIST_OPEN => handle_list_open(&self.sessions),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── Handler implementations ──────────────────────────────────────────────────

fn handle_open(
    forge_root: &Path,
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "open")?;
    let abs = resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("open: {e}")))?;

    let source = fs::read_to_string(&abs)
        .map_err(|e| exec_err(format!("open: read '{}': {e}", abs.display())))?;

    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(&source)
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

fn handle_save(
    forge_root: &Path,
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let markdown = {
        let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let s = guard
            .get(&relpath)
            .ok_or_else(|| exec_err(format!("save: no open session for '{relpath}'")))?;
        MarkdownSerializer::serialize(&s.tree)
    };

    let abs = resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("save: {e}")))?;
    atomic_write(&abs, &markdown)
        .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
    Ok(serde_json::json!({}))
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
/// absolute paths. Parallels
/// [`crates/nexus-app/src/forge.rs::resolve_within`](../../nexus-app/src/forge.rs)
/// — duplicated here because the editor plugin must not depend on
/// `nexus-app`.
fn resolve_within(root: &Path, relpath: &str) -> Result<PathBuf, String> {
    if relpath.is_empty() {
        return Err("empty relpath".into());
    }
    let rel = Path::new(relpath);
    for c in rel.components() {
        match c {
            Component::Normal(_) => {}
            _ => return Err(format!("invalid relpath: {relpath}")),
        }
    }
    let candidate = root.join(rel);
    let canon_root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let canon = fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("path escapes forge root: {relpath}"));
    }
    Ok(canon)
}

/// Write `contents` to `path` via a sibling `.tmp` + rename. Same
/// strategy the storage layer uses; keeps partially-written files off
/// disk on crash.
fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("no parent dir for '{}'", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("no filename in '{}'", path.display()))?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
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
