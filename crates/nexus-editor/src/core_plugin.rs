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

use nexus_kernel::{EventBus, KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::UndoTree;

/// Plugin id of the storage core plugin — the target of the editor's
/// IPC `read_file`/`write_file` calls.
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

/// Plugin id of the database core plugin — the target of
/// `apply_view` calls from the inline `[[{db:query}]]` executor
/// ([`HANDLER_EXECUTE_DATABASE_VIEW`]).
const DATABASE_PLUGIN_ID: &str = "com.nexus.database";

/// Per-call timeout for storage IPC. File I/O is local; a generous
/// bound is safe.
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.editor";

/// Prefix for per-session mutation events. Each mutation handler
/// emits a `NexusEvent::Custom` with `type_id` of the form
/// `com.nexus.editor.changed.<relpath>` so shell subscribers can
/// filter by prefix (via [`nexus_kernel::EventFilter::CustomPrefix`])
/// and still see which file changed. Payload shape:
/// `{ "relpath": String, "revision": u64, "transaction_id": Option<Uuid> }`.
/// Phase 4 of `docs/editor-transaction-wiring-plan.md`.
pub const EVENT_CHANGED_PREFIX: &str = "com.nexus.editor.changed.";

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
/// Handler id for `get_markdown`. Args: `{ "relpath": String }`; Returns: `String`.
///
/// Serializes the session's current block tree via
/// [`MarkdownSerializer::serialize`] and returns the canonical markdown
/// form — the exact text the kernel would write back on save. Shells
/// use this for content hydration so rendered text round-trips through
/// the same parser/serializer pair the disk write uses (Phase 3 of the
/// editor transaction wiring plan).
pub const HANDLER_GET_MARKDOWN: u32 = 10;

/// Handler id for `stamp_block`. Args:
/// `{ "relpath": String, "block_id": Uuid }`; Returns:
/// `{ "block_id": Uuid, "stable_id": Uuid, "newly_stamped": bool }`.
///
/// Promotes the block addressed by `block_id` to a stable id (ADR
/// 0017). The session's in-memory tree is rekeyed onto a fresh v4
/// uuid; that uuid is set as [`crate::Block::stable_id`] so the next
/// [`HANDLER_SAVE`] writes a `<!-- ^<uuid> -->` marker, and
/// subsequent re-opens key the block under the same uuid regardless
/// of upstream insertions.
///
/// A fresh v4 (rather than reusing `block_id` itself) avoids the slot-
/// collision case where an unrelated block later lands at the
/// originally-stamped block's positional slot — the deterministic
/// hash for that slot would otherwise duplicate the stamp.
///
/// Idempotent: a second call against an already-stamped block
/// returns the existing `stable_id` with `newly_stamped: false`. The
/// returned `block_id` is the lookup id passed in (which after the
/// rekey equals `stable_id` for newly-stamped blocks, so callers can
/// continue using it as the kernel-side reference).
///
/// Cross-session stable ids unblock BL-048 (drag-to-embed), BL-049
/// (block-links navigator), and BL-050 (side-margin comments) — see
/// [`docs/adr/0017-block-id-stability.md`](../../../../docs/adr/0017-block-id-stability.md).
pub const HANDLER_STAMP_BLOCK: u32 = 11;

/// Handler id for `execute_database_view`. Args:
/// `{ "database_path": String, "view_config": DatabaseViewConfig }`;
/// Returns:
/// `{ "applied": <AppliedView>, "schema": BaseSchema }`.
///
/// Resolves an inline `[[{db:query}]]` block ([PRD-08 §8.1]) by
/// (1) loading the target `.bases` directory through
/// `com.nexus.storage::base_load`, (2) translating
/// [`crate::DatabaseViewConfig`] into a structured
/// [`nexus_types::bases::BaseView`] via
/// [`crate::database_view::config_to_view`], and (3) handing
/// schema + records + view to `com.nexus.database::apply_view`.
///
/// The handler is read-only: it touches no editor session and
/// emits no `com.nexus.editor.changed.*` event. Callers that need
/// a reactive refresh should subscribe to
/// `com.nexus.storage.bases.changed.*` separately.
///
/// This is split 1 of BL-012; the CM6 widget, decoration plumbing,
/// undo integration, and filter/sort UX layer on top in later
/// splits.
pub const HANDLER_EXECUTE_DATABASE_VIEW: u32 = 12;

/// Handler id for `resolve_block_link`. Args:
/// `{ "file_relpath": String, "block_id": Uuid }`; Returns:
/// `{ "found": bool, "block": Option<Block>, "root_index": Option<usize> }`.
///
/// Resolves the `[[<file>#^<block-id>]]` syntax (BL-049). When
/// `file_relpath` is already open as a session, the handler reads
/// the in-memory block tree (so unsaved edits flow through);
/// otherwise it reads the file from disk and parses transiently
/// without polluting the session map. `root_index` is the position
/// in `tree.root_blocks` of the root ancestor of the target block —
/// the shell uses it to scroll into view (the granularity available
/// before per-block source-position metadata lands).
///
/// The handler is read-only: it touches no editor session state.
/// Callers that need a reactive refresh should subscribe to the
/// `com.nexus.editor.changed.<relpath>` event already published by
/// `apply_transaction` / `undo` / `redo`.
pub const HANDLER_RESOLVE_BLOCK_LINK: u32 = 13;

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
    /// Monotonic per-session mutation counter. Incremented on every
    /// successful `apply_transaction` / `undo` / `redo` / `sync_content`
    /// before the snapshot is taken. Shell subscribers use this (via
    /// the `com.nexus.editor.changed.<relpath>` event) to detect stale
    /// local state and to dedupe the echoes of their own dispatches.
    pub revision: u64,
}

// ── Plugin state ─────────────────────────────────────────────────────────────

/// A single open document.
struct Session {
    tree: BlockTree,
    undo: UndoTree,
    relpath: String,
    /// Monotonic mutation counter. See [`EditorSnapshot::revision`].
    revision: u64,
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
    /// Kernel event bus used to publish
    /// `com.nexus.editor.changed.<relpath>` events after every
    /// successful mutation. `None` for unit tests that drive the
    /// plugin without a full runtime — events are silently dropped.
    event_bus: Option<Arc<EventBus>>,
}

impl EditorCorePlugin {
    /// Create a new plugin rooted at `forge_root`, without an event
    /// bus. Mutation events will be silently dropped — used by the
    /// unit tests in this module that drive the plugin directly.
    #[must_use]
    pub fn new(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            context: None,
            event_bus: None,
        }
    }

    /// Create a new plugin wired to an event bus. The bootstrap uses
    /// this path so shell subscribers can observe edits via
    /// [`EVENT_CHANGED_PREFIX`].
    #[must_use]
    pub fn with_event_bus(forge_root: PathBuf, event_bus: Arc<EventBus>) -> Self {
        Self {
            forge_root,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            context: None,
            event_bus: Some(event_bus),
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
            HANDLER_APPLY_TRANSACTION => {
                handle_apply_transaction(&self.sessions, self.event_bus.as_ref(), args)
            }
            HANDLER_UNDO => handle_undo(&self.sessions, self.event_bus.as_ref(), args),
            HANDLER_REDO => handle_redo(&self.sessions, self.event_bus.as_ref(), args),
            HANDLER_LIST_OPEN => handle_list_open(&self.sessions),
            HANDLER_SYNC_CONTENT => {
                handle_sync_content(&self.sessions, self.event_bus.as_ref(), args)
            }
            HANDLER_GET_MARKDOWN => handle_get_markdown(&self.sessions, args),
            HANDLER_STAMP_BLOCK => {
                handle_stamp_block(&self.sessions, self.event_bus.as_ref(), args)
            }
            HANDLER_EXECUTE_DATABASE_VIEW => Err(exec_err(
                "execute_database_view requires the async dispatch path \
                 (storage + database IPC); call via the kernel runtime"
                    .to_string(),
            )),
            HANDLER_RESOLVE_BLOCK_LINK => {
                handle_resolve_block_link_sync(&self.forge_root, &self.sessions, args)
            }
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
            HANDLER_OPEN
            | HANDLER_SAVE
            | HANDLER_EXECUTE_DATABASE_VIEW
            | HANDLER_RESOLVE_BLOCK_LINK => {}
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
                HANDLER_EXECUTE_DATABASE_VIEW => {
                    handle_execute_database_view(ctx, &args).await
                }
                HANDLER_RESOLVE_BLOCK_LINK => {
                    handle_resolve_block_link_async(&forge_root, sessions, ctx, &args).await
                }
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
    relpath: &str,
    source: &str,
) -> Result<Value, PluginError> {
    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.to_string(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(source)
        .map_err(|e| exec_err(format!("open: parse '{relpath}': {e}")))?;

    let session = Session {
        tree,
        undo: UndoTree::new(),
        relpath: relpath.to_string(),
        revision: 0,
    };
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    guard.insert(relpath.to_string(), session);
    let s = guard.get(relpath).expect("just inserted");
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
    finish_open(sessions, &relpath, &source)
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

    finish_open(&sessions, &relpath, &source)
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

/// Execute an inline `[[{db:query}]]` view: load the base, translate
/// the editor-side [`crate::DatabaseViewConfig`] to a structured
/// [`nexus_types::bases::BaseView`], and run it through
/// `com.nexus.database::apply_view`.
///
/// Requires a wired [`KernelPluginContext`] — there is no fallback
/// path because both lookups are kernel-mediated. Returns
/// [`crate::database_view::ExecuteDatabaseViewResponse`] as JSON.
async fn handle_execute_database_view(
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    #[derive(Deserialize)]
    struct LoadedBase {
        schema: nexus_types::bases::BaseSchema,
        records: Vec<nexus_types::bases::BaseRecord>,
    }

    let parsed: crate::database_view::ExecuteDatabaseViewArgs = serde_json::from_value(
        args.clone(),
    )
    .map_err(|e| exec_err(format!("execute_database_view: invalid args: {e}")))?;

    let ctx = ctx.ok_or_else(|| {
        exec_err(
            "execute_database_view: no kernel context wired (this handler \
             cannot run in context-less unit tests)"
                .to_string(),
        )
    })?;

    // 1. Load the base through storage.
    let base_value = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "base_load",
            serde_json::json!({ "path": parsed.database_path }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("execute_database_view: storage.base_load: {e}")))?;

    // The `base_load` handler returns a [`nexus_types::bases::Base`] —
    // we only need its schema + records here.
    let LoadedBase { schema, records } = serde_json::from_value(base_value).map_err(|e| {
        exec_err(format!(
            "execute_database_view: decode base_load response: {e}"
        ))
    })?;

    // 2. Translate config → structured view.
    let view = crate::database_view::config_to_view(
        &parsed.database_path,
        &parsed.view_config,
    )
    .map_err(|e| exec_err(format!("execute_database_view: {e}")))?;

    // 3. Apply via the database plugin.
    let applied = ctx
        .ipc_call(
            DATABASE_PLUGIN_ID,
            "apply_view",
            serde_json::json!({
                "records": records,
                "schema": schema,
                "view": view,
            }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("execute_database_view: database.apply_view: {e}")))?;

    serde_json::to_value(crate::database_view::ExecuteDatabaseViewResponse { applied, schema })
        .map_err(|e| exec_err(format!("execute_database_view: serialize response: {e}")))
}

/// Resolve `block_id` against the in-memory session for `relpath`
/// when one is open, returning the lookup result with the root
/// ancestor's index in `tree.root_blocks`. Returns `Ok(None)` when
/// no session exists for `relpath`; the caller falls back to a
/// fresh parse.
fn resolve_in_session(
    sessions: &Mutex<HashMap<String, Session>>,
    relpath: &str,
    block_id: uuid::Uuid,
) -> Result<Option<Value>, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let Some(s) = guard.get(relpath) else {
        return Ok(None);
    };
    Ok(Some(resolve_in_tree(&s.tree, block_id)))
}

/// Walk `tree.root_blocks` to find which root ancestor contains
/// `block_id`, returning the lookup payload as JSON. Pure — does
/// not consult any session map.
fn resolve_in_tree(tree: &crate::BlockTree, block_id: uuid::Uuid) -> Value {
    let Some(block) = tree.get(block_id) else {
        return serde_json::json!({
            "found": false,
            "block": null,
            "root_index": null,
        });
    };

    // Walk parents up to a root block.
    let mut cursor = block;
    while let Some(parent_id) = cursor.parent_id {
        match tree.get(parent_id) {
            Some(parent) => cursor = parent,
            None => break,
        }
    }
    let root_index = tree.root_blocks.iter().position(|id| *id == cursor.id);

    serde_json::json!({
        "found": true,
        "block": block,
        "root_index": root_index,
    })
}

fn parse_resolve_args(args: &Value) -> Result<(String, uuid::Uuid), PluginError> {
    let relpath = args
        .get("file_relpath")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            exec_err("resolve_block_link: missing 'file_relpath' string".to_string())
        })?;
    let block_id_str = args
        .get("block_id")
        .and_then(Value::as_str)
        .ok_or_else(|| exec_err("resolve_block_link: missing 'block_id' string".to_string()))?;
    let block_id = uuid::Uuid::parse_str(block_id_str)
        .map_err(|e| exec_err(format!("resolve_block_link: invalid 'block_id': {e}")))?;
    Ok((relpath, block_id))
}

fn handle_resolve_block_link_sync(
    forge_root: &Path,
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some(value) = resolve_in_session(sessions, &relpath, block_id)? {
        return Ok(value);
    }

    // No open session — read + parse transiently. Same fs fallback
    // path as `handle_open_sync` (production traffic goes through
    // the async path via the kernel runtime).
    let abs = resolve_within(forge_root, &relpath)
        .map_err(|e| exec_err(format!("resolve_block_link: {e}")))?;
    let source = fs::read_to_string(&abs)
        .map_err(|e| exec_err(format!("resolve_block_link: read '{}': {e}", abs.display())))?;
    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(&source)
        .map_err(|e| exec_err(format!("resolve_block_link: parse '{relpath}': {e}")))?;
    Ok(resolve_in_tree(&tree, block_id))
}

async fn handle_resolve_block_link_async(
    forge_root: &Path,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some(value) = resolve_in_session(&sessions, &relpath, block_id)? {
        return Ok(value);
    }

    let source = if let Some(ctx) = ctx.as_deref() {
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
            .map_err(|e| exec_err(format!("resolve_block_link: storage.read_file: {e}")))?;
        let resp: Resp = serde_json::from_value(value).map_err(|e| {
            exec_err(format!("resolve_block_link: storage.read_file decode: {e}"))
        })?;
        String::from_utf8(resp.bytes)
            .map_err(|_| exec_err(format!("resolve_block_link: '{relpath}' is not UTF-8")))?
    } else {
        let abs = resolve_within(forge_root, &relpath)
            .map_err(|e| exec_err(format!("resolve_block_link: {e}")))?;
        fs::read_to_string(&abs)
            .map_err(|e| exec_err(format!("resolve_block_link: read '{}': {e}", abs.display())))?
    };

    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(&source)
        .map_err(|e| exec_err(format!("resolve_block_link: parse '{relpath}': {e}")))?;
    Ok(resolve_in_tree(&tree, block_id))
}

fn handle_apply_transaction(
    sessions: &Mutex<HashMap<String, Session>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "apply_transaction")?;
    let tx_value = args
        .get("transaction")
        .ok_or_else(|| exec_err("apply_transaction: missing 'transaction'".to_string()))?
        .clone();
    // Issue #85. Cap the transaction payload size before
    // deserializing into a `Transaction` so a malicious/buggy caller
    // can't ask the CRDT engine to walk gigabytes of operations in
    // one IPC dispatch. 16 MiB is generous (a normal transaction is
    // a few KB; a large multi-block paste is < 1 MiB) and the cap
    // is on the JSON byte size, not op count, because per-op cost
    // varies wildly.
    const MAX_TRANSACTION_JSON_BYTES: usize = 16 * 1024 * 1024;
    let tx_json_size = serde_json::to_vec(&tx_value)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if tx_json_size > MAX_TRANSACTION_JSON_BYTES {
        return Err(exec_err(format!(
            "apply_transaction: transaction is {tx_json_size} bytes; \
             max is {MAX_TRANSACTION_JSON_BYTES} bytes"
        )));
    }
    let tx: crate::Transaction = serde_json::from_value(tx_value)
        .map_err(|e| exec_err(format!("apply_transaction: invalid transaction: {e}")))?;
    let tx_id = tx.id;

    let (value, revision) = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let s = guard.get_mut(&relpath).ok_or_else(|| {
            exec_err(format!(
                "apply_transaction: no open session for '{relpath}'"
            ))
        })?;
        s.undo
            .execute(tx, &mut s.tree)
            .map_err(|e| exec_err(format!("apply_transaction: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "apply_transaction")?;
        (val, rev)
    };
    publish_changed(event_bus, &relpath, revision, Some(tx_id));
    Ok(value)
}

fn handle_undo(
    sessions: &Mutex<HashMap<String, Session>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "undo")?;
    let (value, revision) = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let s = guard
            .get_mut(&relpath)
            .ok_or_else(|| exec_err(format!("undo: no open session for '{relpath}'")))?;
        s.undo
            .undo(&mut s.tree)
            .map_err(|e| exec_err(format!("undo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "undo")?;
        (val, rev)
    };
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

fn handle_redo(
    sessions: &Mutex<HashMap<String, Session>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "redo")?;
    let (value, revision) = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let s = guard
            .get_mut(&relpath)
            .ok_or_else(|| exec_err(format!("redo: no open session for '{relpath}'")))?;
        s.undo
            .redo(&mut s.tree)
            .map_err(|e| exec_err(format!("redo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "redo")?;
        (val, rev)
    };
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
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
    event_bus: Option<&Arc<EventBus>>,
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

    let revision = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let session = guard.entry(relpath.clone()).or_insert_with(|| Session {
            tree: BlockTree::default(),
            undo: UndoTree::new(),
            relpath: relpath.clone(),
            revision: 0,
        });
        session.tree = tree;
        session.revision = session.revision.saturating_add(1);
        session.revision
    };
    publish_changed(event_bus, &relpath, revision, None);

    Ok(serde_json::json!({}))
}

/// Serialize the session's block tree to markdown and return it as a
/// bare JSON string. Matches `serialize_session` but surfaces the
/// result over IPC rather than routing it to disk.
fn handle_get_markdown(
    sessions: &Mutex<HashMap<String, Session>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_markdown")?;
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let s = guard
        .get(&relpath)
        .ok_or_else(|| exec_err(format!("get_markdown: no open session for '{relpath}'")))?;
    let markdown = MarkdownSerializer::serialize(&s.tree);
    Ok(Value::String(markdown))
}

/// Stamp the addressed block with a fresh v4 stable id so the next
/// `save` writes a `<!-- ^<uuid> -->` marker and the id survives
/// upstream insertions on reload (ADR 0017). Idempotent: a second
/// call against an already-stamped block returns the existing stamp
/// without bumping the session revision or publishing a changed
/// event.
///
/// The block is rekeyed via [`crate::BlockTree::rekey`] from its
/// current positional id to the fresh stamp; references in the
/// parent's `children` list, `root_blocks`, and child blocks'
/// `parent_id` are all updated together. After rekey, the block's
/// `id` and `stable_id` are equal — the lookup `block_id` arg passed
/// in is returned as `block_id` in the response so the caller can
/// still reference it, while `stable_id` carries the new uuid that's
/// now the canonical key.
fn handle_stamp_block(
    sessions: &Mutex<HashMap<String, Session>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "stamp_block")?;
    let block_id_str = args
        .get("block_id")
        .and_then(Value::as_str)
        .ok_or_else(|| exec_err("stamp_block: missing 'block_id' string".to_string()))?;
    let block_id = uuid::Uuid::parse_str(block_id_str)
        .map_err(|e| exec_err(format!("stamp_block: invalid 'block_id': {e}")))?;

    let (stable_id, newly_stamped, revision) = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        let s = guard.get_mut(&relpath).ok_or_else(|| {
            exec_err(format!("stamp_block: no open session for '{relpath}'"))
        })?;
        let block = s.tree.get(block_id).ok_or_else(|| {
            exec_err(format!(
                "stamp_block: block '{block_id}' not present in '{relpath}'"
            ))
        })?;
        if let Some(existing) = block.stable_id {
            // Already stamped: return the existing stamp untouched.
            (existing, false, s.revision)
        } else {
            let new_id = uuid::Uuid::new_v4();
            s.tree
                .rekey(block_id, new_id)
                .map_err(|e| exec_err(format!("stamp_block: rekey: {e}")))?;
            s.revision = s.revision.saturating_add(1);
            (new_id, true, s.revision)
        }
    };

    if newly_stamped {
        publish_changed(event_bus, &relpath, revision, None);
    }

    Ok(serde_json::json!({
        "block_id": block_id,
        "stable_id": stable_id,
        "newly_stamped": newly_stamped,
    }))
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
        revision: s.revision,
    }
}

/// Publish a `com.nexus.editor.changed.<relpath>` custom event with
/// `{ relpath, revision, transaction_id }`. `transaction_id` is the
/// applied transaction's UUID for `apply_transaction` and `None`
/// (serialized as JSON `null`) for `undo` / `redo` / `sync_content`
/// — none of those carry a client-supplied id the shell could echo-
/// suppress on. Mirrors the publish-on-mutation pattern used by
/// `com.nexus.theme` (see `crates/nexus-theme/src/core_plugin.rs`).
fn publish_changed(
    event_bus: Option<&Arc<EventBus>>,
    relpath: &str,
    revision: u64,
    transaction_id: Option<uuid::Uuid>,
) {
    let Some(bus) = event_bus else { return };
    let type_id = format!("{EVENT_CHANGED_PREFIX}{relpath}");
    let payload = serde_json::json!({
        "relpath": relpath,
        "revision": revision,
        "transaction_id": transaction_id,
    });
    // Bus publish errors are namespace/closed-channel cases we can't
    // meaningfully recover from inside a handler — log and move on
    // so the mutation itself still succeeds for the caller.
    if let Err(err) = bus.publish_plugin(PLUGIN_ID, &type_id, payload) {
        tracing::warn!(
            plugin = PLUGIN_ID,
            %err,
            relpath = %relpath,
            "failed to publish editor changed event"
        );
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
    fn get_markdown_returns_serialized_tree() {
        use crate::{Operation, Transaction, TransactionMetadata};
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "Hello\n");
        let mut p = new_plugin(root);

        // Open and apply a transaction so the on-disk file and the
        // in-memory tree diverge — then verify get_markdown reflects
        // the in-memory state, not the disk contents.
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let para_id = snap.tree.root_blocks[0];
        let content_len = snap.tree.blocks[&para_id].content.len();
        let tx = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: content_len,
                text: " world".into(),
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        p.dispatch(
            HANDLER_APPLY_TRANSACTION,
            &serde_json::json!({
                "relpath": "notes/a.md",
                "transaction": serde_json::to_value(&tx).unwrap(),
            }),
        )
        .unwrap();

        // Call get_markdown and compare against a direct serialize of
        // the session's current tree (round-trip check).
        let resp = p
            .dispatch(
                HANDLER_GET_MARKDOWN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap();
        let md: String = serde_json::from_value(resp).unwrap();
        let snap2: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(md, MarkdownSerializer::serialize(&snap2.tree));
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn get_markdown_on_unopen_errors() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        assert!(p
            .dispatch(
                HANDLER_GET_MARKDOWN,
                &serde_json::json!({ "relpath": "never-opened.md" }),
            )
            .is_err());
    }

    #[test]
    fn apply_transaction_publishes_changed_event_with_revision_and_tx_id() {
        use crate::{Operation, Transaction, TransactionMetadata};
        use nexus_kernel::{EventFilter, NexusEvent};

        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "abc\n");

        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.editor.changed.".to_string(),
        ));
        let mut p = EditorCorePlugin::with_event_bus(root, Arc::clone(&bus));
        p.on_init().unwrap();

        // open should NOT emit a changed event — it's not a mutation.
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snap.revision, 0, "fresh session starts at revision 0");
        let para_id = snap.tree.root_blocks[0];

        let tx = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: 3,
                text: "d".into(),
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        let tx_id = tx.id;
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_APPLY_TRANSACTION,
                &serde_json::json!({
                    "relpath": "notes/a.md",
                    "transaction": serde_json::to_value(&tx).unwrap(),
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snap.revision, 1, "apply_transaction bumps revision");

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            NexusEvent::Custom {
                type_id,
                payload,
                emitting_plugin,
            } => {
                assert_eq!(type_id, "com.nexus.editor.changed.notes/a.md");
                assert_eq!(emitting_plugin, PLUGIN_ID);
                assert_eq!(payload["relpath"], "notes/a.md");
                assert_eq!(payload["revision"], 1);
                assert_eq!(
                    payload["transaction_id"].as_str().unwrap(),
                    tx_id.to_string(),
                );
            }
            other => panic!("expected Custom, got {other:?}"),
        }

        // undo also emits, with transaction_id: null.
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_UNDO,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(snap.revision, 2, "undo bumps revision");
        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            NexusEvent::Custom { payload, .. } => {
                assert_eq!(payload["revision"], 2);
                assert!(payload["transaction_id"].is_null());
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn sync_content_publishes_changed_event() {
        use nexus_kernel::{EventFilter, NexusEvent};

        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "first\n");

        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.editor.changed.".to_string(),
        ));
        let mut p = EditorCorePlugin::with_event_bus(root, Arc::clone(&bus));
        p.on_init().unwrap();

        // sync_content on a previously-unopened session is allowed — it
        // creates the session. Still counts as a mutation → event fires.
        p.dispatch(
            HANDLER_SYNC_CONTENT,
            &serde_json::json!({ "relpath": "notes/a.md", "content": "updated\n" }),
        )
        .unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, "com.nexus.editor.changed.notes/a.md");
                assert_eq!(payload["revision"], 1);
                assert!(payload["transaction_id"].is_null());
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn unknown_handler_id_errors() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        let err = p.dispatch(999, &serde_json::json!({})).unwrap_err();
        assert!(format!("{err}").contains("unknown handler id 999"));
    }

    // ── ADR 0017: stamp_block handler ──

    #[test]
    fn stamp_block_promotes_block_id_and_persists_through_save() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "# Hi\n\nBody\n");
        let mut p = new_plugin(root.clone());
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        // Pick the body paragraph (root_blocks[1]).
        let para_id = snap.tree.root_blocks[1];

        // Stamp it.
        let resp = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({
                    "relpath": "notes/a.md",
                    "block_id": para_id.to_string(),
                }),
            )
            .unwrap();
        assert_eq!(resp["block_id"].as_str().unwrap(), para_id.to_string());
        assert_eq!(resp["newly_stamped"], serde_json::json!(true));
        let stamp_id = uuid::Uuid::parse_str(resp["stable_id"].as_str().unwrap()).unwrap();
        assert_ne!(
            stamp_id, para_id,
            "stamp must be a fresh v4, distinct from the positional id"
        );

        // The in-memory block was rekeyed: the old positional id is
        // gone, a new entry exists at the stamped id.
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!snap.tree.blocks.contains_key(&para_id));
        let block = snap.tree.blocks.get(&stamp_id).unwrap();
        assert_eq!(block.stable_id, Some(stamp_id));
        assert_eq!(block.id, stamp_id);

        // Save and re-read disk: the marker should be present.
        p.dispatch(
            HANDLER_SAVE,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        let on_disk = fs::read_to_string(root.join("notes/a.md")).unwrap();
        assert!(
            on_disk.contains(&format!("<!-- ^{stamp_id} -->")),
            "expected stamp marker on disk, got: {on_disk}"
        );
    }

    #[test]
    fn stamp_block_is_idempotent() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "Body\n");
        let mut p = new_plugin(root);
        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let id = snap.tree.root_blocks[0];

        let r1 = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({ "relpath": "notes/a.md", "block_id": id.to_string() }),
            )
            .unwrap();
        assert_eq!(r1["newly_stamped"], serde_json::json!(true));
        let stamp_id_str = r1["stable_id"].as_str().unwrap().to_string();
        // Second call addresses the new (stamped) id; the rekey moved
        // the block off its original positional id.
        let r2 = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({ "relpath": "notes/a.md", "block_id": stamp_id_str }),
            )
            .unwrap();
        assert_eq!(r2["newly_stamped"], serde_json::json!(false));
        assert_eq!(r2["stable_id"], r1["stable_id"]);
    }

    #[test]
    fn stamp_block_rejects_unknown_block() {
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "x\n");
        let mut p = new_plugin(root);
        p.dispatch(
            HANDLER_OPEN,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        let bogus = uuid::Uuid::nil();
        let err = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({ "relpath": "notes/a.md", "block_id": bogus.to_string() }),
            )
            .unwrap_err();
        assert!(format!("{err}").contains("not present"));
    }

    #[test]
    fn stamp_block_rejects_missing_args() {
        let (_tmp, root) = setup_forge();
        let mut p = new_plugin(root);
        let err = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap_err();
        assert!(format!("{err}").contains("block_id"));
    }

    #[test]
    fn stamp_block_round_trips_through_save_and_reopen() {
        // End-to-end: stamp → save → close → re-open → confirm the
        // re-parsed tree still keys the block under the stamped id,
        // even after an out-of-band insertion shifts every downstream
        // positional id.
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "First\n\nSecond\n");
        let mut p = new_plugin(root.clone());

        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let target_id = snap.tree.root_blocks[1];
        let stamp_resp = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({
                    "relpath": "notes/a.md",
                    "block_id": target_id.to_string(),
                }),
            )
            .unwrap();
        let stamp_id =
            uuid::Uuid::parse_str(stamp_resp["stable_id"].as_str().unwrap()).unwrap();
        p.dispatch(
            HANDLER_SAVE,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_CLOSE,
            &serde_json::json!({ "relpath": "notes/a.md" }),
        )
        .unwrap();

        // Prepend a new heading out-of-band — same kind of edit that
        // would normally renumber every downstream positional id.
        let body = fs::read_to_string(root.join("notes/a.md")).unwrap();
        let edited = format!("# New top\n\n{body}");
        write_note(&root, "notes/a.md", &edited);

        let snap: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            snap.tree.blocks.contains_key(&stamp_id),
            "stamped id must survive upstream insertion: {:?}",
            snap.tree.root_blocks,
        );
        let block = snap.tree.blocks.get(&stamp_id).unwrap();
        assert_eq!(block.stable_id, Some(stamp_id));
    }

    // ── BL-049: resolve_block_link ────────────────────────────────────────

    /// Build a forge with a single markdown file at `relpath` whose
    /// content is `body`, return the editor plugin already bound to
    /// that forge. Tests stamp a block via `HANDLER_STAMP_BLOCK` and
    /// then resolve it via `HANDLER_RESOLVE_BLOCK_LINK`.
    fn forge_with_file(relpath: &str, body: &str) -> (tempfile::TempDir, EditorCorePlugin) {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join(relpath);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&abs, body).unwrap();
        let plugin = EditorCorePlugin::new(dir.path().to_path_buf());
        (dir, plugin)
    }

    #[test]
    fn resolve_block_link_returns_block_for_open_session() {
        let (_dir, mut p) = forge_with_file("notes/a.md", "first paragraph\n\nsecond\n");
        let snap = open_value(&mut p, "notes/a.md");
        let block_id = snap.tree.root_blocks[0];

        let resp = p
            .dispatch(
                HANDLER_RESOLVE_BLOCK_LINK,
                &serde_json::json!({
                    "file_relpath": "notes/a.md",
                    "block_id": block_id.to_string(),
                }),
            )
            .unwrap();
        assert_eq!(resp.get("found").and_then(Value::as_bool), Some(true));
        assert_eq!(resp.get("root_index").and_then(Value::as_u64), Some(0));
        let block = resp.get("block").unwrap();
        assert_eq!(
            block.get("id").and_then(Value::as_str),
            Some(block_id.to_string()).as_deref(),
        );
    }

    #[test]
    fn resolve_block_link_falls_back_to_disk_when_session_is_closed() {
        // Stamp a block, save, close — resolve must still find it
        // via the fs fallback (no session, no kernel context).
        let (_dir, mut p) = forge_with_file("notes/b.md", "alpha\n\nbeta\n");
        let snap = open_value(&mut p, "notes/b.md");
        let target_id = snap.tree.root_blocks[1];
        let stamped: Value = p
            .dispatch(
                HANDLER_STAMP_BLOCK,
                &serde_json::json!({
                    "relpath": "notes/b.md",
                    "block_id": target_id.to_string(),
                }),
            )
            .unwrap();
        let stable_id = stamped
            .get("stable_id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        p.dispatch(
            HANDLER_SAVE,
            &serde_json::json!({ "relpath": "notes/b.md" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_CLOSE,
            &serde_json::json!({ "relpath": "notes/b.md" }),
        )
        .unwrap();

        let resp = p
            .dispatch(
                HANDLER_RESOLVE_BLOCK_LINK,
                &serde_json::json!({
                    "file_relpath": "notes/b.md",
                    "block_id": stable_id,
                }),
            )
            .unwrap();
        assert_eq!(resp.get("found").and_then(Value::as_bool), Some(true));
        assert_eq!(resp.get("root_index").and_then(Value::as_u64), Some(1));
    }

    #[test]
    fn resolve_block_link_returns_not_found_for_unknown_id() {
        let (_dir, mut p) = forge_with_file("notes/c.md", "only\n");
        open_value(&mut p, "notes/c.md");
        let bogus = uuid::Uuid::nil();
        let resp = p
            .dispatch(
                HANDLER_RESOLVE_BLOCK_LINK,
                &serde_json::json!({
                    "file_relpath": "notes/c.md",
                    "block_id": bogus.to_string(),
                }),
            )
            .unwrap();
        assert_eq!(resp.get("found").and_then(Value::as_bool), Some(false));
        assert!(resp.get("block").is_some_and(Value::is_null));
        assert!(resp.get("root_index").is_some_and(Value::is_null));
    }

    #[test]
    fn resolve_block_link_root_index_walks_to_root_for_nested_blocks() {
        // Synthetic tree with a known parent → child relationship so
        // the test doesn't depend on the markdown parser's container
        // representation. The resolver must walk up to the root and
        // report the root's `root_blocks` index for the *child*.
        use crate::{Block, BlockTree};
        let mut tree = BlockTree::new(crate::DocumentMetadata::default());
        let root_a = uuid::Uuid::new_v4();
        let root_b = uuid::Uuid::new_v4();
        let child = uuid::Uuid::new_v4();
        let mk_block = |id: uuid::Uuid| Block {
            id,
            stable_id: None,
            ty: crate::BlockType::Paragraph,
            content: String::new(),
            annotations: Vec::new(),
            properties: crate::BlockProperties::default(),
            parent_id: None,
            children: Vec::new(),
            index_in_parent: 0,
            created_at: 0,
            updated_at: 0,
            is_deleted: false,
        };
        tree.insert(mk_block(root_a), None, 0).unwrap();
        tree.insert(mk_block(root_b), None, 1).unwrap();
        let mut child_block = mk_block(child);
        child_block.parent_id = Some(root_b);
        tree.insert(child_block, Some(root_b), 0).unwrap();

        let resolved = resolve_in_tree(&tree, child);
        assert_eq!(resolved.get("found").and_then(Value::as_bool), Some(true));
        assert_eq!(resolved.get("root_index").and_then(Value::as_u64), Some(1));
    }

    #[test]
    fn resolve_block_link_rejects_invalid_uuid() {
        let (_dir, mut p) = forge_with_file("notes/e.md", "x\n");
        let err = p
            .dispatch(
                HANDLER_RESOLVE_BLOCK_LINK,
                &serde_json::json!({
                    "file_relpath": "notes/e.md",
                    "block_id": "not-a-uuid",
                }),
            )
            .unwrap_err();
        let s = format!("{err:?}");
        assert!(s.contains("invalid 'block_id'"), "got: {s}");
    }

    fn open_value(p: &mut EditorCorePlugin, relpath: &str) -> EditorSnapshot {
        let resp = p
            .dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": relpath }),
            )
            .unwrap();
        serde_json::from_value(resp).unwrap()
    }
}
