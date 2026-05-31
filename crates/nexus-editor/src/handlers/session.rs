//! Session-lifecycle handlers: `open` (sync + async), `close`
//! (sync + async), `sync_content`. Includes the BL-072 persistent
//! undo-history machinery used by `open_async` / `close_async`.
//!
//! Lifted from `core_plugin.rs` by SD-03 editor chunk 2
//! (2026-05-18 SOLID/DRY audit).

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use nexus_kernel::{EventBus, Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core_plugin::{OpObserver, Session, SessionMap, PLUGIN_ID};
use crate::markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::{PersistedUndoTree, UndoTree};

use super::shared::{
    exec_err, get_session_entry, insert_session_entry, publish_changed, relpath_arg,
    remove_session_entry, resolve_within, sessions_poisoned, snapshot_of, snapshot_to_value,
    MULTIBUFFER_RELPATH_PREFIX, STORAGE_IPC_TIMEOUT, STORAGE_PLUGIN_ID,
};

/// Build a new session from already-loaded source text and insert it
/// into the session map. Shared tail of the sync + async `open` paths.
fn finish_open(
    sessions: &SessionMap,
    observer: Option<&Arc<dyn OpObserver>>,
    relpath: &str,
    source: &str,
) -> Result<Value, PluginError> {
    finish_open_with_undo(sessions, observer, relpath, source, None)
}

/// Like [`finish_open`], but installs `restored_undo` (typically from
/// a successful BL-072 probe) into the new session instead of the
/// default empty [`UndoTree`]. The session revision starts at 0 either
/// way — `revision` is a per-session monotonic mutation counter, not
/// a serialized cross-session sequence.
fn finish_open_with_undo(
    sessions: &SessionMap,
    observer: Option<&Arc<dyn OpObserver>>,
    relpath: &str,
    source: &str,
    restored_undo: Option<UndoTree>,
) -> Result<Value, PluginError> {
    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.to_string(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(source)
        .map_err(|e| exec_err(format!("open: parse '{relpath}': {e}")))?;

    // BL-074 hook: notify the observer *before* the session goes into
    // the map so it can fail fast if it cares to. The observer cannot
    // mutate the session — it's a read-only signal carrying tree +
    // canonical source.
    if let Some(obs) = observer {
        obs.on_session_opened(relpath, &tree, source.as_bytes());
    }

    let session = Session {
        tree,
        undo: restored_undo.unwrap_or_default(),
        relpath: relpath.to_string(),
        revision: 0,
        is_synthetic: false,
    };
    let entry = insert_session_entry(sessions, relpath.to_string(), session)?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    snapshot_to_value(&snapshot_of(&s), "open")
}

/// Return the existing snapshot for a `multibuffer://` relpath, or
/// an error if no synthetic session has been created via
/// `open_excerpts` for this id. Used by both `open_sync`
/// and `open_async` before they try to hit the disk.
fn try_open_existing_synthetic(
    sessions: &SessionMap,
    relpath: &str,
) -> Option<Result<Value, PluginError>> {
    if !relpath.starts_with(MULTIBUFFER_RELPATH_PREFIX) {
        return None;
    }
    let entry = match get_session_entry(sessions, relpath) {
        Ok(opt) => opt,
        Err(e) => return Some(Err(e)),
    };
    Some(match entry {
        Some(arc) => match arc.lock() {
            Ok(s) => snapshot_to_value(&snapshot_of(&s), "open"),
            Err(_) => Err(sessions_poisoned()),
        },
        None => Err(exec_err(format!(
            "open: synthetic session '{relpath}' not found — \
             multibuffer relpaths are only created by `open_excerpts`"
        ))),
    })
}

pub(crate) fn open_sync(
    forge_root: &Path,
    sessions: &SessionMap,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "open")?;
    if let Some(res) = try_open_existing_synthetic(sessions, &relpath) {
        return res;
    }
    let abs = resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("open: {e}")))?;
    let source = fs::read_to_string(&abs)
        .map_err(|e| exec_err(format!("open: read '{}': {e}", abs.display())))?;
    finish_open(sessions, observer, &relpath, &source)
}

pub(crate) async fn open_async(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "open")?;
    if let Some(res) = try_open_existing_synthetic(&sessions, &relpath) {
        return res;
    }

    let source_bytes = if let Some(ctx) = ctx.as_deref() {
        // Preferred path: fetch through `com.nexus.storage` so capability
        // checks, atomic-write audit, and future observability hooks all
        // cover editor reads.
        #[derive(Deserialize)]
        struct Resp {
            bytes: Option<Vec<u8>>,
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
        resp.bytes
            .ok_or_else(|| exec_err(format!("open: file not found: '{relpath}'")))?
    } else {
        // Fallback used only when no context has been wired (unit tests
        // that drive the plugin directly without a runtime).
        let abs =
            resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("open: {e}")))?;
        fs::read(&abs).map_err(|e| exec_err(format!("open: read '{}': {e}", abs.display())))?
    };

    let source = String::from_utf8(source_bytes.clone())
        .map_err(|_| exec_err(format!("open: '{relpath}' is not UTF-8")))?;

    // BL-072: probe for a persisted undo tree against the same source
    // bytes. The integrity check is hash-based rather than mtime-based
    // — file-as-truth means the only correct answer to "does this
    // history match?" is "did the bytes change?".
    let restored_undo = if let Some(ctx) = ctx.as_deref() {
        let hash = content_hash_hex(&source_bytes);
        try_restore_undo(ctx, &relpath, &hash).await
    } else {
        None
    };

    finish_open_with_undo(&sessions, observer, &relpath, &source, restored_undo)
}

pub(crate) fn close(
    sessions: &SessionMap,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "close")?;
    if let Some(obs) = observer {
        obs.on_session_closed(&relpath);
    }
    remove_session_entry(sessions, &relpath)?;
    Ok(serde_json::json!({}))
}

pub(crate) async fn close_async(
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "close")?;

    // BL-074 hook: fire BEFORE the session is removed so the observer
    // can flush per-relpath state. Mirrors the BL-072 undo persistence
    // pattern where the side-effect runs alongside session teardown.
    if let Some(obs) = observer {
        obs.on_session_closed(&relpath);
    }

    // Capture the session's tree + undo before removing it so the
    // persistence write happens against a consistent snapshot but the
    // session map is freed for re-open as soon as possible.
    let captured = remove_session_entry(&sessions, &relpath)?.map(|s| (s.tree, s.undo));

    if let (Some(ctx), Some((tree, undo))) = (ctx.as_deref(), captured) {
        // Hash the canonical-markdown serialization — that's what
        // `save` would write to disk, and what `open` will compare
        // against on reload.
        let markdown = MarkdownSerializer::serialize(&tree);
        let hash = content_hash_hex(markdown.as_bytes());
        persist_undo(ctx, &relpath, hash, &undo).await;
    }

    Ok(serde_json::json!({}))
}

/// Re-parse `content` and update (or create) the block tree for `relpath`.
///
/// The undo history is left untouched: `sync_content` is a background resync
/// for read-only consumers (AI, MCP, outline), not a user-visible transaction.
pub(crate) fn sync_content(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
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

    // BL-074 hook: a sync_content reset is observable as a fresh
    // session-open from the observer's perspective. Fire the hook
    // *outside* the session lock so the observer can do disk I/O
    // without deadlocking against future apply_transaction calls.
    if let Some(obs) = observer {
        obs.on_session_opened(&relpath, &tree, content.as_bytes());
    }

    // BL-126 follow-up: `sync_content` is a create-or-update — we hold
    // the outer lock just long enough to insert a fresh entry when the
    // session is missing, then drop it and acquire the per-session
    // inner lock for the actual mutation. This mirrors the
    // `acquire_session_entry` discipline used by every other handler.
    let entry = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        Arc::clone(guard.entry(relpath.clone()).or_insert_with(|| {
            Arc::new(Mutex::new(Session {
                tree: BlockTree::default(),
                undo: UndoTree::new(),
                relpath: relpath.clone(),
                revision: 0,
                is_synthetic: false,
            }))
        }))
    };
    let revision = {
        let mut session = entry.lock().map_err(|_| sessions_poisoned())?;
        session.tree = tree;
        session.revision = session.revision.saturating_add(1);
        session.revision
    };
    publish_changed(event_bus, &relpath, revision, None);

    Ok(serde_json::json!({}))
}

// ── BL-072: persistent undo history ──────────────────────────────────────────

/// Persisted undo cap. The serialized snapshot keeps at most this many
/// transactions on the current branch (older / off-branch entries are
/// dropped). Roughly 500 bulk-insert transactions × ~1 KiB JSON each is
/// around 500 KiB on disk, comfortable below the 1 MiB-ish point where
/// reads start showing up in profiles.
const UNDO_PERSIST_MAX_OPS: usize = 500;

/// Stale-file age. Persisted undo files older than this are treated as
/// missing on open and the file is deleted opportunistically.
const UNDO_STALE_AFTER_SECS: u64 = 7 * 24 * 60 * 60;

/// On-disk wrapper around [`PersistedUndoTree`] that records what file
/// content the history was attached to, so a mismatch on reopen
/// (external edit, unsaved close, etc.) skips the restore instead of
/// applying undo against the wrong tree shape.
#[derive(Serialize, Deserialize)]
struct PersistedUndoState {
    /// Schema version. Bump when the on-disk shape changes; older
    /// versions are ignored on read so we degrade gracefully rather
    /// than panic.
    version: u32,
    /// Wall-clock seconds since the unix epoch at write time.
    persisted_at_unix: u64,
    /// SHA-256 (hex) of the source bytes the history was built
    /// against. Computed at close time over the canonical-markdown
    /// serialization of the in-memory tree (matches what `save`
    /// writes), and re-checked on open against the bytes returned by
    /// `storage.read_file` for the same path.
    content_hash: String,
    undo: PersistedUndoTree,
}

const UNDO_STATE_VERSION: u32 = 1;

/// Build the `.forge/.editor/undo/<sha-of-relpath>.json` storage path
/// for `relpath`. We hash the path so the on-disk filename is opaque
/// (no traversal, no clashes with `/`-bearing relpaths) — the source
/// path is recoverable from inside the file via the schema if needed.
fn undo_state_path(relpath: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(relpath.as_bytes());
    let digest = hasher.finalize();
    // 16 hex chars / 64 bits is enough for collision resistance over
    // the few hundred files a forge actually edits in a session.
    let mut hex = String::with_capacity(16);
    for b in digest.iter().take(8) {
        write!(&mut hex, "{b:02x}").expect("write to String");
    }
    format!(".forge/.editor/undo/{hex}.json")
}

/// SHA-256 hex of `bytes`. Used as the integrity tag on persisted
/// undo state so an external edit between close and open invalidates
/// the cached history.
pub(crate) fn content_hash_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in &digest {
        write!(&mut hex, "{b:02x}").expect("write to String");
    }
    hex
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Try to load a persisted [`PersistedUndoState`] for `relpath` whose
/// `content_hash` matches `expected_hash`. Returns the hydrated
/// [`UndoTree`] on success, `None` for any of: file missing, stale
/// (>`UNDO_STALE_AFTER_SECS`), version mismatch, hash mismatch, or
/// any decode failure. Stale / version-mismatched files are deleted
/// opportunistically.
async fn try_restore_undo(
    ctx: &KernelPluginContext,
    relpath: &str,
    expected_hash: &str,
) -> Option<UndoTree> {
    #[derive(Deserialize)]
    struct ReadResp {
        bytes: Vec<u8>,
    }

    let path = undo_state_path(relpath);
    let read = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "read_file",
            serde_json::json!({ "path": path }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .ok()?;
    let resp: ReadResp = serde_json::from_value(read).ok()?;

    let state: PersistedUndoState = match serde_json::from_slice(&resp.bytes) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!(
                plugin = PLUGIN_ID,
                relpath,
                %err,
                "BL-072: persisted undo decode failed; ignoring"
            );
            delete_undo_file(ctx, &path).await;
            return None;
        }
    };

    if state.version != UNDO_STATE_VERSION {
        delete_undo_file(ctx, &path).await;
        return None;
    }
    let now = now_unix_secs();
    if now.saturating_sub(state.persisted_at_unix) > UNDO_STALE_AFTER_SECS {
        delete_undo_file(ctx, &path).await;
        return None;
    }
    if state.content_hash != expected_hash {
        // The file changed between close and open (unsaved edits,
        // external edit, etc.). Don't apply the cached history —
        // its op offsets are anchored to the old tree shape. Leave
        // the file in place: the user might re-save and reopen.
        return None;
    }
    Some(UndoTree::from(state.undo))
}

async fn delete_undo_file(ctx: &KernelPluginContext, path: &str) {
    let _ = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "delete_file",
            serde_json::json!({ "path": path }),
            STORAGE_IPC_TIMEOUT,
        )
        .await;
}

/// Persist `undo` for `relpath` against `content_hash`. Truncates to
/// [`UNDO_PERSIST_MAX_OPS`] on the current branch. Errors are logged
/// at warn level and swallowed: persistence is additive, a write
/// failure must not surface as a close failure.
pub(crate) async fn persist_undo(
    ctx: &KernelPluginContext,
    relpath: &str,
    content_hash: String,
    undo: &UndoTree,
) {
    if undo.is_empty() {
        // Nothing to restore; opportunistically clear any stale file
        // for this relpath so the on-disk state matches.
        delete_undo_file(ctx, &undo_state_path(relpath)).await;
        return;
    }
    let state = PersistedUndoState {
        version: UNDO_STATE_VERSION,
        persisted_at_unix: now_unix_secs(),
        content_hash,
        undo: undo.to_persisted(Some(UNDO_PERSIST_MAX_OPS)),
    };
    let bytes = match serde_json::to_vec(&state) {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(
                plugin = PLUGIN_ID,
                relpath,
                %err,
                "BL-072: serialize persisted undo failed"
            );
            return;
        }
    };
    let path = undo_state_path(relpath);
    if let Err(err) = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "write_vault_file",
            serde_json::json!({ "path": path, "bytes": bytes }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
    {
        tracing::warn!(
            plugin = PLUGIN_ID,
            relpath,
            %err,
            "BL-072: write persisted undo failed"
        );
    }
}
