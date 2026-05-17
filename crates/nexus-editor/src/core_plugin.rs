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

use crate::block::{Block, BlockType};
use crate::markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::{PersistedUndoTree, UndoTree};
use uuid::Uuid;

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

/// Observer that the editor calls on session lifecycle and successful
/// transactions. Used by `nexus-bootstrap`'s `CrdtPublisher` (BL-074
/// editor wiring) to mirror every session into a `CrdtDoc`, publish
/// per-op events on the kernel bus, and persist CRDT state on close.
///
/// Lives in `nexus-editor` (rather than `nexus-crdt`) to avoid a
/// circular dependency — `nexus-crdt` already deps on `nexus-editor`,
/// so the trait must be defined here and *implemented* in a third
/// crate that pulls both. The trait is intentionally narrow: only the
/// raw `Operation`s and the relpath cross the boundary.
pub trait OpObserver: Send + Sync {
    /// Called when a session is created (open or `sync_content`
    /// reset). The observer should construct or re-construct any
    /// per-relpath state from the supplied tree + canonical-markdown
    /// `source_bytes`. May be called more than once for the same
    /// `relpath` if the session is reset by `sync_content`.
    fn on_session_opened(&self, relpath: &str, tree: &crate::BlockTree, source_bytes: &[u8]);

    /// Called immediately before a session is removed from the map.
    /// The observer's last chance to flush per-relpath state to disk.
    fn on_session_closed(&self, relpath: &str);

    /// Called after a transaction has applied successfully. The ops
    /// are in their applied order; the observer must NOT re-apply
    /// them to any tree the editor owns — its own internal state is
    /// the only thing it should mutate.
    fn on_apply_transaction(&self, relpath: &str, ops: &[crate::Operation]);

    /// Called after a successful `undo`. The observer receives the
    /// transaction that was reversed, in its original (apply-order)
    /// op list, plus the post-undo block tree. To stay in sync with
    /// peers, the observer should author inverse ops against its own
    /// state — see [`crate::Operation::inverse`].
    ///
    /// Default impl: no-op. Implementors that don't track undo
    /// semantics across sessions can ignore it.
    fn on_undo_transaction(
        &self,
        _relpath: &str,
        _reversed: &crate::Transaction,
        _post_tree: &crate::BlockTree,
    ) {
    }

    /// Called after a successful `redo`. Mirror of
    /// [`Self::on_undo_transaction`] — the transaction was re-applied,
    /// so the observer should treat its ops as a fresh local apply.
    ///
    /// Default impl: no-op.
    fn on_redo_transaction(
        &self,
        _relpath: &str,
        _replayed: &crate::Transaction,
        _post_tree: &crate::BlockTree,
    ) {
    }
}

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
/// Handler id for `apply_transaction`. Args: `{ "relpath": String, "transaction": Transaction }`;
/// Returns: [`ApplyTransactionResponse`] — a tagged union of `slim`
/// (text-only ops; just `{ revision }`) or `full` (structural ops;
/// the post-apply [`EditorSnapshot`]). See BL-123.
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

/// Handler id for `open_excerpts`. Args:
/// `{ "items": [{ "relpath": String, "line_start": u32, "line_end": u32, "label": Option<String> }, ...] }`;
/// Returns:
/// `{ "relpath": String, "tree": BlockTree, ... }` (an
/// [`EditorSnapshot`] keyed by a synthetic `multibuffer://<uuid>`
/// relpath that subsequent reads should pass back to `get_tree` /
/// `close`).
///
/// Constructs a read-only synthetic session whose root blocks are
/// [`crate::BlockType::Excerpt`] entries — one per requested item,
/// each carrying the captured snapshot of `relpath` lines
/// `line_start..=line_end` (1-based, inclusive). Source files are
/// read via `com.nexus.storage::read_file` (so capability checks +
/// path resolution apply); a per-item read failure aborts the call.
///
/// BL-141 Phase 1 semantics:
/// - Sessions are **read-only**: `apply_transaction` and `save`
///   reject `is_synthetic` sessions explicitly. Phase 2 will route
///   per-excerpt edits to the source files' sessions.
/// - Empty `items` is rejected (`-32602` invalid params).
/// - Overlapping ranges within a single source file are merged
///   (same `relpath`, `(a.line_start..=a.line_end) ∩ (b.line_start..=b.line_end)
///   != ∅`) so a multibuffer over diagnostics doesn't render the
///   same lines twice.
pub const HANDLER_OPEN_EXCERPTS: u32 = 14;

/// Numeric id of the `refresh_excerpts` handler. BL-141 Approach B
/// step 3 — external-edit subscription.
///
/// Re-reads every Excerpt block's source file (through the storage
/// IPC, same as `open_excerpts`) and replaces each block's content
/// snapshot with the current source's slice for the recorded
/// `[line_start..line_end]` range. Lines stay 1-based inclusive;
/// content past the source's EOF clips silently (matches `slice_lines`).
///
/// Caller is expected to subscribe to `com.nexus.editor.changed.<source>`
/// events on the shell side and call this handler when any source the
/// multibuffer covers reports a change. Re-reading every source on
/// each call (rather than only the changed one) keeps the wire shape
/// trivial and the on-disk read cost amortised — typical multibuffers
/// touch < 20 files.
///
/// Returns the post-refresh [`EditorSnapshot`]. Errors:
///
/// - `relpath` doesn't resolve to a session (`SessionNotFound`).
/// - The session isn't synthetic (`InvalidParams`).
/// - A source file read fails (`ExecutionFailed`).
///
/// Bumps the synthetic session's revision counter and publishes
/// `com.nexus.editor.changed.<synthetic_relpath>` so any UI mirror
/// re-renders.
pub const HANDLER_REFRESH_EXCERPTS: u32 = 15;

// ── Wire types ───────────────────────────────────────────────────────────────

/// Per-item input shape for [`HANDLER_OPEN_EXCERPTS`].
#[derive(Debug, Clone, Deserialize)]
pub struct ExcerptRequest {
    /// Forge-relative path of the source file to read from.
    pub relpath: String,
    /// First line to include (1-based, inclusive).
    pub line_start: u32,
    /// Last line to include (1-based, inclusive).
    pub line_end: u32,
    /// Optional caller-supplied label (e.g. the diagnostic message
    /// or reference site name) rendered alongside the
    /// `{relpath}#L{line_start}-L{line_end}` header.
    #[serde(default)]
    pub label: Option<String>,
}

/// Response shape for [`HANDLER_APPLY_TRANSACTION`] (BL-123).
///
/// Text-only ops (`insert_text` / `delete_text`) get a [`Slim`] reply
/// carrying just the post-apply revision counter. The webview already
/// short-circuits the snapshot reconcile for these ops via the
/// `skipReconcile` shortcut in `transactionBridge.ts`, so the only
/// thing it needs from the kernel is the new revision number — which
/// makes the kernel-side cost O(1) instead of O(N blocks) (the
/// snapshot serialize is the dominant term in BL-122's baseline:
/// 39 → 330 → 24190 µs p50 across 10/100/5000-block docs).
///
/// Structural ops (`insert_block`, `delete_block`, `reparent_block`,
/// `update_block_content`, `update_annotations`) still get a full
/// [`EditorSnapshot`] so the shell can reflow block IDs, annotations,
/// and any other tree-shape change. `update_annotations` is in the
/// full path on purpose: the bridge's optimistic mirror doesn't track
/// annotation changes, so the snapshot is the only authoritative
/// source for the post-apply annotation list.
///
/// Wire shape (tagged union, `snake_case`):
/// - `{ "kind": "slim", "revision": 5 }`
/// - `{ "kind": "full", "relpath": "...", "tree": {...}, ... }`
///
/// [`Slim`]: ApplyTransactionResponse::Slim
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
// The Full variant carries an EditorSnapshot (block tree + counters);
// Slim is just a revision counter. The size delta is intentional —
// boxing Full would force a heap allocation for every structural op
// response, where the snapshot's own internal allocations already
// dominate, so the optimization is a wash. The Slim path (the
// typing-hot one) doesn't pay for the variant size.
#[allow(clippy::large_enum_variant)]
pub enum ApplyTransactionResponse {
    /// Text-only op response — just the post-apply revision counter.
    Slim {
        /// Post-apply value of the session's monotonic revision
        /// counter. Same field used by `com.nexus.editor.changed.*`.
        revision: u64,
    },
    /// Structural op response — the full post-apply session snapshot.
    Full(EditorSnapshot),
}

impl ApplyTransactionResponse {
    /// Post-apply revision counter, regardless of variant.
    #[must_use]
    pub fn revision(&self) -> u64 {
        match self {
            Self::Slim { revision } => *revision,
            Self::Full(snapshot) => snapshot.revision,
        }
    }

    /// Borrow the inner snapshot if this is a [`Full`] response.
    ///
    /// [`Full`]: ApplyTransactionResponse::Full
    #[must_use]
    pub fn snapshot(&self) -> Option<&EditorSnapshot> {
        match self {
            Self::Full(snapshot) => Some(snapshot),
            Self::Slim { .. } => None,
        }
    }
}

/// Snapshot of an open editor session, suitable for IPC return.
///
/// The tree is returned in full — delta snapshots are a follow-up
/// optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    /// BL-141 — `true` for multibuffer / excerpt sessions assembled
    /// from `open_excerpts`. Synthetic sessions are not backed by a
    /// single source file on disk; `save` and `apply_transaction`
    /// reject them so the caller can't silently corrupt the source
    /// files behind their excerpts. (Read-write multibuffer with
    /// per-excerpt edit routing is BL-141 Phase 2.)
    is_synthetic: bool,
}

/// BL-126 follow-up: per-session lock. The outer map is acquired
/// briefly to clone the per-relpath `Arc`, then released so two
/// concurrent dispatches against different relpaths can hold their
/// inner locks simultaneously. The inner mutex serialises mutation
/// of a single session — unchanged from the pre-refactor invariant.
type SessionEntry = Arc<Mutex<Session>>;

/// BL-126 follow-up: the editor-plugin session map. See
/// [`SessionEntry`] for the locking discipline; helpers
/// [`acquire_session_entry`] / [`get_session_entry`] /
/// [`insert_session_entry`] / [`remove_session_entry`] encapsulate
/// every outer-lock-and-drop access pattern.
type SessionMap = Mutex<HashMap<String, SessionEntry>>;

/// BL-126 follow-up: acquire the per-session `Arc` for `relpath`,
/// holding the outer map lock only long enough to clone it. Returns
/// `Err` with a uniformly-shaped "no open session for …" message
/// keyed by the operation name when the session is missing.
fn acquire_session_entry(
    sessions: &SessionMap,
    relpath: &str,
    op: &str,
) -> Result<SessionEntry, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    guard
        .get(relpath)
        .map(Arc::clone)
        .ok_or_else(|| exec_err(format!("{op}: no open session for '{relpath}'")))
}

/// Like [`acquire_session_entry`] but returns `None` rather than an
/// error when no session is registered for `relpath`. Used by paths
/// that have a fallback when the session map is empty (the resolve /
/// synthetic-open code paths).
fn get_session_entry(
    sessions: &SessionMap,
    relpath: &str,
) -> Result<Option<SessionEntry>, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    Ok(guard.get(relpath).map(Arc::clone))
}

/// BL-126 follow-up: insert a freshly built [`Session`] into the
/// map, returning the per-session `Arc` so the caller can lock it
/// without re-acquiring the outer lock. Replaces any existing
/// session under the same relpath, mirroring the pre-refactor
/// `guard.insert(...)` semantics.
fn insert_session_entry(
    sessions: &SessionMap,
    relpath: String,
    session: Session,
) -> Result<SessionEntry, PluginError> {
    let entry: SessionEntry = Arc::new(Mutex::new(session));
    let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    guard.insert(relpath, Arc::clone(&entry));
    Ok(entry)
}

/// BL-126 follow-up: remove the session for `relpath` (if any) and
/// return the inner `Session` value by unwrapping the `Arc<Mutex<…>>`.
/// Returns `Ok(None)` when no session is registered. The unwrap can
/// only fail if another caller still holds an `Arc` clone — handlers
/// drop the outer lock before doing per-session work, so the only
/// way to keep a clone alive is a long-running mutation; in that
/// case we fall back to draining the inner `Mutex<Session>` by
/// temporarily acquiring it and replacing with a synthetic empty
/// session. In practice handlers never store their entry across
/// awaits, so the `try_unwrap` path is always taken.
fn remove_session_entry(
    sessions: &SessionMap,
    relpath: &str,
) -> Result<Option<Session>, PluginError> {
    let removed = {
        let mut guard = sessions.lock().map_err(|_| sessions_poisoned())?;
        guard.remove(relpath)
    };
    let Some(entry) = removed else {
        return Ok(None);
    };
    // Common path: no other handler is holding a clone of this Arc,
    // so we can move the inner `Session` out without acquiring the
    // inner lock at all.
    match Arc::try_unwrap(entry) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(session) => Ok(Some(session)),
            Err(_) => Err(sessions_poisoned()),
        },
        Err(arc) => {
            // Fallback: another handler raced us and is still mutating
            // this session. Wait for its lock, then move the Session
            // out by `mem::replace` against a synthetic empty body —
            // the synthetic body is immediately dropped along with the
            // mutex when this scope ends.
            let mut guard = arc.lock().map_err(|_| sessions_poisoned())?;
            let placeholder = Session {
                tree: BlockTree::default(),
                undo: UndoTree::new(),
                relpath: relpath.to_string(),
                revision: 0,
                is_synthetic: false,
            };
            Ok(Some(std::mem::replace(&mut *guard, placeholder)))
        }
    }
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
    sessions: Arc<SessionMap>,
    /// Plugin-facing kernel context. Installed via
    /// [`CorePlugin::wire_context`] once the bootstrap has the shared
    /// dispatcher assembled; `None` for sync-only test drivers.
    context: Option<Arc<KernelPluginContext>>,
    /// Kernel event bus used to publish
    /// `com.nexus.editor.changed.<relpath>` events after every
    /// successful mutation. `None` for unit tests that drive the
    /// plugin without a full runtime — events are silently dropped.
    event_bus: Option<Arc<EventBus>>,
    /// Lifecycle observer (BL-074 editor wiring). When set, fires on
    /// every session open/close and successful transaction. `None`
    /// for unit tests and any runtime that hasn't opted into CRDT
    /// publishing.
    op_observer: Option<Arc<dyn OpObserver>>,
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
            op_observer: None,
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
            op_observer: None,
        }
    }

    /// Install a session/transaction observer. Called by the bootstrap
    /// after construction (BL-074 editor wiring) — separating the
    /// observer from the constructor avoids piling more positional
    /// arguments onto an already-overloaded fixture surface.
    pub fn set_op_observer(&mut self, observer: Arc<dyn OpObserver>) {
        self.op_observer = Some(observer);
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
            HANDLER_OPEN => handle_open_sync(
                &self.forge_root,
                &self.sessions,
                self.op_observer.as_ref(),
                args,
            ),
            // BL-072: persistent undo writes happen on the async path
            // (storage IPC). The sync entry point still drops the
            // session — unit tests use it directly.
            HANDLER_CLOSE => handle_close(&self.sessions, self.op_observer.as_ref(), args),
            HANDLER_GET_TREE => handle_get_tree(&self.sessions, args),
            HANDLER_SAVE => handle_save_sync(&self.forge_root, &self.sessions, args),
            HANDLER_APPLY_TRANSACTION => handle_apply_transaction(
                &self.sessions,
                self.event_bus.as_ref(),
                self.op_observer.as_ref(),
                args,
            ),
            HANDLER_UNDO => handle_undo(
                &self.sessions,
                self.event_bus.as_ref(),
                self.op_observer.as_ref(),
                args,
            ),
            HANDLER_REDO => handle_redo(
                &self.sessions,
                self.event_bus.as_ref(),
                self.op_observer.as_ref(),
                args,
            ),
            HANDLER_LIST_OPEN => handle_list_open(&self.sessions),
            HANDLER_SYNC_CONTENT => handle_sync_content(
                &self.sessions,
                self.event_bus.as_ref(),
                self.op_observer.as_ref(),
                args,
            ),
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
            HANDLER_OPEN_EXCERPTS => Err(exec_err(
                "open_excerpts requires the async dispatch path \
                 (storage IPC for source-file reads); call via the \
                 kernel runtime"
                    .to_string(),
            )),
            HANDLER_REFRESH_EXCERPTS => Err(exec_err(
                "refresh_excerpts requires the async dispatch path \
                 (storage IPC for source-file reads); call via the \
                 kernel runtime"
                    .to_string(),
            )),
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
            | HANDLER_CLOSE
            | HANDLER_SAVE
            | HANDLER_EXECUTE_DATABASE_VIEW
            | HANDLER_RESOLVE_BLOCK_LINK
            | HANDLER_OPEN_EXCERPTS
            | HANDLER_REFRESH_EXCERPTS => {}
            _ => return None,
        }

        // Capture everything the future needs by value / Arc so nothing
        // outlives the `&mut self` borrow.
        let forge_root = self.forge_root.clone();
        let sessions = Arc::clone(&self.sessions);
        let ctx = self.context.clone();
        let event_bus = self.event_bus.clone();
        let observer = self.op_observer.clone();
        let args = args.clone();

        Some(Box::pin(async move {
            match handler_id {
                HANDLER_OPEN => {
                    handle_open_async(&forge_root, sessions, ctx, observer.as_ref(), &args).await
                }
                HANDLER_CLOSE => {
                    handle_close_async(sessions, ctx, observer.as_ref(), &args).await
                }
                HANDLER_SAVE => handle_save_async(&forge_root, sessions, ctx, &args).await,
                HANDLER_EXECUTE_DATABASE_VIEW => {
                    handle_execute_database_view(ctx, &args).await
                }
                HANDLER_RESOLVE_BLOCK_LINK => {
                    handle_resolve_block_link_async(
                        &forge_root,
                        sessions,
                        ctx,
                        event_bus.as_ref(),
                        &args,
                    )
                    .await
                }
                HANDLER_OPEN_EXCERPTS => {
                    handle_open_excerpts(&forge_root, sessions, ctx, &args).await
                }
                HANDLER_REFRESH_EXCERPTS => {
                    handle_refresh_excerpts(
                        &forge_root,
                        sessions,
                        ctx,
                        event_bus.as_ref(),
                        &args,
                    )
                    .await
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

/// BL-141 — synthetic multibuffer relpaths use the `multibuffer://`
/// scheme. They're only ever created by `open_excerpts`; subsequent
/// `open` calls (typically from the shell's session-manager
/// acquire path) must NOT try to read from disk — the URI doesn't
/// resolve to a file. Instead, return the existing snapshot
/// idempotently, or error if no synthetic session is registered
/// under this id.
pub(crate) const MULTIBUFFER_RELPATH_PREFIX: &str = "multibuffer://";

/// Return the existing snapshot for a `multibuffer://` relpath, or
/// an error if no synthetic session has been created via
/// `open_excerpts` for this id. Used by both `handle_open_sync`
/// and `handle_open_async` before they try to hit the disk.
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

fn handle_open_sync(
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

async fn handle_open_async(
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

fn handle_close(
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
fn content_hash_hex(bytes: &[u8]) -> String {
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
async fn persist_undo(
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

async fn handle_close_async(
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

fn handle_get_tree(
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_tree")?;
    let entry = acquire_session_entry(sessions, &relpath, "get_tree")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    snapshot_to_value(&snapshot_of(&s), "get_tree")
}

/// BL-141 Phase 2 — one excerpt to splice back into its source
/// file's line range. Built by `plan_save` for synthetic sessions;
/// applied by `splice_excerpts` after the source is read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExcerptSplice {
    /// First line of the range to replace (1-based, inclusive).
    line_start: u32,
    /// Last line of the range to replace (1-based, inclusive).
    line_end: u32,
    /// New content (the user-edited multibuffer text for this
    /// excerpt). May contain embedded newlines.
    new_content: String,
}

/// BL-141 Phase 2 — what to do for a single `save` dispatch.
enum SavePlan {
    /// Regular session — write the canonical markdown serialization
    /// to `relpath`.
    Regular { markdown: String },
    /// Synthetic (multibuffer) session — for each `(source_relpath,
    /// splices)` pair, read the source file, apply every splice in
    /// reverse-line-order, write back.
    Splice {
        sources: Vec<(String, Vec<ExcerptSplice>)>,
    },
}

/// Plan the save under the session lock (so the IPC handler holds
/// the lock for the minimum time + the actual I/O can happen
/// without contending with concurrent dispatches).
///
/// Splits the path: regular sessions return their serialized
/// markdown directly; synthetic sessions group their Excerpt
/// blocks by source file so the caller can issue exactly one
/// read+write per source even when multiple excerpts hit the same
/// file.
fn plan_save(
    sessions: &SessionMap,
    relpath: &str,
) -> Result<SavePlan, PluginError> {
    let entry = acquire_session_entry(sessions, relpath, "save")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    if !s.is_synthetic {
        return Ok(SavePlan::Regular {
            markdown: MarkdownSerializer::serialize(&s.tree),
        });
    }
    let mut by_source: HashMap<String, Vec<ExcerptSplice>> = HashMap::new();
    for block_id in &s.tree.root_blocks {
        let Some(block) = s.tree.blocks.get(block_id) else {
            continue;
        };
        if let BlockType::Excerpt {
            source_relpath,
            line_start,
            line_end,
            ..
        } = &block.ty
        {
            by_source
                .entry(source_relpath.clone())
                .or_default()
                .push(ExcerptSplice {
                    line_start: *line_start,
                    line_end: *line_end,
                    new_content: block.content.clone(),
                });
        }
    }
    // Stable ordering across saves: sort source relpaths alphabetically
    // so test fixtures + audit logs see a deterministic write order.
    let mut sources: Vec<(String, Vec<ExcerptSplice>)> = by_source.into_iter().collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(SavePlan::Splice { sources })
}

/// Splice every entry in `splices` into `old`. Splices are applied
/// in reverse-line-order so earlier splices don't shift the line
/// numbers of later ones. Out-of-range entries (line_start past
/// `old`'s end) are skipped defensively rather than panicking — a
/// stale multibuffer (e.g. one whose source was edited externally
/// since the excerpt was captured) shouldn't crash the save.
///
/// Preserves the trailing newline if `old` ended with one, matching
/// the canonical `MarkdownSerializer::serialize` convention.
fn splice_excerpts(old: &str, mut splices: Vec<ExcerptSplice>) -> String {
    // Sort by line_start descending — splice from the END of the
    // file toward the start so a splice that grows or shrinks one
    // range never invalidates the line numbers of a not-yet-applied
    // splice further up the file.
    splices.sort_by(|a, b| b.line_start.cmp(&a.line_start));
    let trailing_nl = old.ends_with('\n');
    let mut lines: Vec<String> = old.lines().map(String::from).collect();
    for sp in splices {
        let start_idx = (sp.line_start.saturating_sub(1)) as usize;
        if start_idx > lines.len() {
            // Excerpt range starts past EOF — skip; preserving the
            // existing source content is better than appending the
            // edit into an arbitrary spot.
            continue;
        }
        let end_idx_exclusive = std::cmp::min(sp.line_end as usize, lines.len());
        let new_lines: Vec<String> = sp.new_content.lines().map(String::from).collect();
        lines.splice(start_idx..end_idx_exclusive, new_lines);
    }
    let mut out = lines.join("\n");
    if trailing_nl {
        out.push('\n');
    }
    out
}

fn handle_save_sync(
    forge_root: &Path,
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    match plan_save(sessions, &relpath)? {
        SavePlan::Regular { markdown } => {
            let abs = resolve_within(forge_root, &relpath)
                .map_err(|e| exec_err(format!("save: {e}")))?;
            atomic_write(&abs, &markdown)
                .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
        }
        SavePlan::Splice { sources } => {
            for (source_relpath, splices) in sources {
                let abs = resolve_within(forge_root, &source_relpath)
                    .map_err(|e| exec_err(format!("save: {e}")))?;
                let old = fs::read_to_string(&abs).map_err(|e| {
                    exec_err(format!(
                        "save: read source '{}': {e}",
                        abs.display()
                    ))
                })?;
                let new = splice_excerpts(&old, splices);
                atomic_write(&abs, &new).map_err(|e| {
                    exec_err(format!(
                        "save: write source '{}': {e}",
                        abs.display()
                    ))
                })?;
            }
        }
    }
    Ok(serde_json::json!({}))
}

async fn handle_save_async(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let plan = plan_save(&sessions, &relpath)?;

    if let Some(ctx) = ctx.as_deref() {
        // Canonical path: storage's `write_file` does temp + fsync +
        // rename and updates the SQLite index atomically with the disk
        // write so a later `open` sees consistent state.
        match plan {
            SavePlan::Regular { markdown } => {
                ctx.ipc_call(
                    STORAGE_PLUGIN_ID,
                    "write_file",
                    serde_json::json!({ "path": relpath, "bytes": markdown.as_bytes() }),
                    STORAGE_IPC_TIMEOUT,
                )
                .await
                .map_err(|e| exec_err(format!("save: storage.write_file: {e}")))?;
            }
            SavePlan::Splice { sources } => {
                for (source_relpath, splices) in sources {
                    let old =
                        read_source_for_excerpts(forge_root, Some(ctx), &source_relpath).await?;
                    let new = splice_excerpts(&old, splices);
                    ctx.ipc_call(
                        STORAGE_PLUGIN_ID,
                        "write_file",
                        serde_json::json!({
                            "path": source_relpath,
                            "bytes": new.as_bytes(),
                        }),
                        STORAGE_IPC_TIMEOUT,
                    )
                    .await
                    .map_err(|e| {
                        exec_err(format!(
                            "save: storage.write_file '{source_relpath}': {e}"
                        ))
                    })?;
                }
            }
        }
        Ok(serde_json::json!({}))
    } else {
        // Fallback for context-less unit tests — mirror the sync path
        // (direct local-fs writes).
        match plan {
            SavePlan::Regular { markdown } => {
                let abs = resolve_within(forge_root, &relpath)
                    .map_err(|e| exec_err(format!("save: {e}")))?;
                atomic_write(&abs, &markdown)
                    .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
            }
            SavePlan::Splice { sources } => {
                for (source_relpath, splices) in sources {
                    let abs = resolve_within(forge_root, &source_relpath)
                        .map_err(|e| exec_err(format!("save: {e}")))?;
                    let old = fs::read_to_string(&abs).map_err(|e| {
                        exec_err(format!(
                            "save: read source '{}': {e}",
                            abs.display()
                        ))
                    })?;
                    let new = splice_excerpts(&old, splices);
                    atomic_write(&abs, &new).map_err(|e| {
                        exec_err(format!(
                            "save: write source '{}': {e}",
                            abs.display()
                        ))
                    })?;
                }
            }
        }
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
///
/// BL-073: when the resolved block has no [`Block::stable_id`] yet,
/// auto-stamp it via [`BlockTree::rekey`] so the next save persists a
/// `<!-- ^<uuid> -->` marker. The new stable id is what the lookup
/// returns. The second tuple element is `Some(revision)` if a stamp
/// happened (caller publishes a `changed` event), `None` otherwise.
/// The filesystem-fallback path used for closed sessions deliberately
/// does **not** auto-stamp — silently mutating the on-disk file from a
/// read-shaped IPC call would be a surprise.
fn resolve_in_session(
    sessions: &SessionMap,
    relpath: &str,
    block_id: uuid::Uuid,
) -> Result<Option<(Value, Option<u64>)>, PluginError> {
    let Some(entry) = get_session_entry(sessions, relpath)? else {
        return Ok(None);
    };
    let mut s = entry.lock().map_err(|_| sessions_poisoned())?;
    let needs_stamp = matches!(
        s.tree.get(block_id),
        Some(block) if block.stable_id.is_none()
    );
    if needs_stamp {
        let new_id = uuid::Uuid::new_v4();
        s.tree
            .rekey(block_id, new_id)
            .map_err(|e| exec_err(format!("resolve_block_link: auto-stamp rekey: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let value = resolve_in_tree(&s.tree, new_id);
        return Ok(Some((value, Some(s.revision))));
    }
    Ok(Some((resolve_in_tree(&s.tree, block_id), None)))
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
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some((value, _stamp_revision)) = resolve_in_session(sessions, &relpath, block_id)? {
        // The sync entry point is unit-test-only (no kernel context →
        // no event bus). The async path below publishes a changed
        // event when an auto-stamp happens.
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
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some((value, stamp_revision)) = resolve_in_session(&sessions, &relpath, block_id)? {
        if let Some(revision) = stamp_revision {
            publish_changed(event_bus, &relpath, revision, None);
        }
        return Ok(value);
    }

    let source = if let Some(ctx) = ctx.as_deref() {
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
            .map_err(|e| exec_err(format!("resolve_block_link: storage.read_file: {e}")))?;
        let resp: Resp = serde_json::from_value(value).map_err(|e| {
            exec_err(format!("resolve_block_link: storage.read_file decode: {e}"))
        })?;
        let bytes = resp.bytes.ok_or_else(|| {
            exec_err(format!("resolve_block_link: file not found: '{relpath}'"))
        })?;
        String::from_utf8(bytes)
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

// ── BL-141 — open_excerpts ──────────────────────────────────────────────────

/// Implementation of [`HANDLER_OPEN_EXCERPTS`]. Assembles a synthetic
/// read-only session whose root blocks are
/// [`crate::BlockType::Excerpt`] entries, one per merged input item.
///
/// Source files are read via `com.nexus.storage::read_file` (with a
/// local-fs fallback for context-less unit tests). Per-source files
/// are read once even if the input lists multiple ranges from the
/// same path.
async fn handle_open_excerpts(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let items_value = args
        .get("items")
        .ok_or_else(|| exec_err("open_excerpts: missing 'items'".to_string()))?
        .clone();
    let raw_items: Vec<ExcerptRequest> = serde_json::from_value(items_value)
        .map_err(|e| exec_err(format!("open_excerpts: invalid 'items': {e}")))?;
    if raw_items.is_empty() {
        return Err(exec_err(
            "open_excerpts: 'items' must be non-empty".to_string(),
        ));
    }

    // Validate each item's line range up front; cheaper to fail fast
    // than to pay for storage reads on a doomed call.
    for (idx, item) in raw_items.iter().enumerate() {
        if item.line_start == 0 || item.line_end == 0 {
            return Err(exec_err(format!(
                "open_excerpts: item {idx} has zero line number (1-based)"
            )));
        }
        if item.line_start > item.line_end {
            return Err(exec_err(format!(
                "open_excerpts: item {idx} has line_start ({}) > line_end ({})",
                item.line_start, item.line_end
            )));
        }
    }

    // Walk in input order; for each item merge with any previously-kept
    // entry from the same source file whose range touches or overlaps.
    // Preserves first-seen order across the assembled view, which
    // matters for diagnostics / find-refs lists that flow in a
    // meaningful sequence (e.g. file → file → file).
    let merged = merge_excerpt_requests(raw_items);

    // Read every unique source file exactly once. Per-file failures
    // abort the call — a partial multibuffer is more confusing than a
    // clear error.
    let mut sources: HashMap<String, String> = HashMap::new();
    for item in &merged {
        if sources.contains_key(&item.relpath) {
            continue;
        }
        let source = read_source_for_excerpts(forge_root, ctx.as_deref(), &item.relpath).await?;
        sources.insert(item.relpath.clone(), source);
    }

    // Build the synthetic block tree.
    let mut tree = BlockTree::default();
    for item in merged {
        let source = sources.get(&item.relpath).expect("just inserted");
        let snippet = slice_lines(source, item.line_start, item.line_end);
        let block = Block::new(BlockType::Excerpt {
            source_relpath: item.relpath.clone(),
            line_start: item.line_start,
            line_end: item.line_end,
            label: item.label.clone(),
        })
        .with_content(snippet);
        let next_idx = tree.root_blocks.len();
        tree.insert(block, None, next_idx)
            .map_err(|e| exec_err(format!("open_excerpts: tree insert: {e}")))?;
    }

    let synthetic_relpath = format!("{MULTIBUFFER_RELPATH_PREFIX}{}", Uuid::new_v4());
    let session = Session {
        tree,
        undo: UndoTree::new(),
        relpath: synthetic_relpath.clone(),
        revision: 0,
        is_synthetic: true,
    };

    let entry = insert_session_entry(&sessions, synthetic_relpath, session)?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    snapshot_to_value(&snapshot_of(&s), "open_excerpts")
}

/// Implementation of [`HANDLER_REFRESH_EXCERPTS`]. Re-reads every
/// source file referenced by a synthetic session's Excerpt blocks
/// and replaces each block's content with the source's current
/// slice. In-place mutation preserves block ids so any cursor state
/// the shell is tracking against this multibuffer stays valid.
///
/// Errors:
/// - `relpath` not in the session map.
/// - Session isn't synthetic (no excerpts to refresh).
/// - Source read fails.
async fn handle_refresh_excerpts(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "refresh_excerpts")?;
    let entry = acquire_session_entry(&sessions, &relpath, "refresh_excerpts")?;

    // Collect the unique source relpaths without holding the lock
    // across the awaits. The lock is briefly re-acquired below to
    // splice the new snippets into the tree.
    let sources_to_read: Vec<String> = {
        let guard = entry.lock().map_err(|_| sessions_poisoned())?;
        if !guard.is_synthetic {
            return Err(exec_err(format!(
                "refresh_excerpts: session '{relpath}' is not a multibuffer"
            )));
        }
        excerpt_sources(&guard.tree)
    };
    if sources_to_read.is_empty() {
        // No-op refresh: still bump revision so callers can observe
        // completion via the event bus.
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        guard.revision = guard.revision.saturating_add(1);
        let rev = guard.revision;
        let value = snapshot_to_value(&snapshot_of(&guard), "refresh_excerpts")?;
        drop(guard);
        publish_changed(event_bus, &relpath, rev, None);
        return Ok(value);
    }

    let mut fresh: HashMap<String, String> = HashMap::with_capacity(sources_to_read.len());
    for src in &sources_to_read {
        let text = read_source_for_excerpts(forge_root, ctx.as_deref(), src).await?;
        fresh.insert(src.clone(), text);
    }

    let (value, revision) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        for id in s.tree.root_blocks.clone() {
            let Some(block) = s.tree.get_mut(id) else {
                continue;
            };
            let BlockType::Excerpt {
                source_relpath,
                line_start,
                line_end,
                ..
            } = &block.ty
            else {
                continue;
            };
            let Some(source) = fresh.get(source_relpath) else {
                continue;
            };
            let snippet = slice_lines(source, *line_start, *line_end);
            if block.content != snippet {
                block.content = snippet;
            }
        }
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let value = snapshot_to_value(&snapshot_of(s), "refresh_excerpts")?;
        (value, rev)
    };
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

/// Unique source relpaths referenced by every `Excerpt` block in
/// `tree`, in first-appearance order across `root_blocks`. Pure —
/// exported (crate-internal) so the shell-side subscriber can ask
/// "which sources does this multibuffer cover?" without re-walking
/// the tree itself.
pub(crate) fn excerpt_sources(tree: &BlockTree) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for id in &tree.root_blocks {
        let Some(block) = tree.blocks.get(id) else {
            continue;
        };
        if let BlockType::Excerpt { source_relpath, .. } = &block.ty {
            if seen.insert(source_relpath.as_str()) {
                out.push(source_relpath.clone());
            }
        }
    }
    out
}

/// Per-source-file read used by `open_excerpts`. Mirrors the
/// `read_file` shape of `handle_open_async` / `handle_resolve_block_link_async`.
async fn read_source_for_excerpts(
    forge_root: &Path,
    ctx: Option<&KernelPluginContext>,
    relpath: &str,
) -> Result<String, PluginError> {
    if let Some(ctx) = ctx {
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
            .map_err(|e| exec_err(format!("open_excerpts: storage.read_file '{relpath}': {e}")))?;
        let resp: Resp = serde_json::from_value(value).map_err(|e| {
            exec_err(format!(
                "open_excerpts: storage.read_file decode '{relpath}': {e}"
            ))
        })?;
        let bytes = resp
            .bytes
            .ok_or_else(|| exec_err(format!("open_excerpts: file not found: '{relpath}'")))?;
        String::from_utf8(bytes)
            .map_err(|_| exec_err(format!("open_excerpts: '{relpath}' is not UTF-8")))
    } else {
        let abs = resolve_within(forge_root, relpath)
            .map_err(|e| exec_err(format!("open_excerpts: {e}")))?;
        fs::read_to_string(&abs).map_err(|e| {
            exec_err(format!(
                "open_excerpts: read '{}': {e}",
                abs.display()
            ))
        })
    }
}

/// Merge per-source-file overlapping or adjacent ranges in
/// first-appearance order. Two ranges `(a_s..=a_e)` and `(b_s..=b_e)`
/// merge when `b_s <= a_e + 1` (i.e. touching counts as overlapping).
/// Labels from later-merged-in items are dropped — the first item's
/// label wins, matching the first-appearance semantic.
fn merge_excerpt_requests(items: Vec<ExcerptRequest>) -> Vec<ExcerptRequest> {
    let mut merged: Vec<ExcerptRequest> = Vec::new();
    for item in items {
        let mut consumed = false;
        for existing in &mut merged {
            if existing.relpath != item.relpath {
                continue;
            }
            let touches = item.line_start <= existing.line_end.saturating_add(1)
                && existing.line_start <= item.line_end.saturating_add(1);
            if touches {
                existing.line_start = existing.line_start.min(item.line_start);
                existing.line_end = existing.line_end.max(item.line_end);
                consumed = true;
                break;
            }
        }
        if !consumed {
            merged.push(item);
        }
    }
    merged
}

/// Extract the inclusive 1-based line range `[start, end]` from
/// `source`, joining with `\n`. Out-of-range start clamps to an
/// empty string; out-of-range end clamps to the source's last line.
fn slice_lines(source: &str, start: u32, end: u32) -> String {
    let total_usize = source.lines().count();
    let total = u32::try_from(total_usize).unwrap_or(u32::MAX);
    if start > total {
        return String::new();
    }
    let start_idx = (start - 1) as usize;
    let end_idx = (end.min(total) - 1) as usize;
    source
        .lines()
        .skip(start_idx)
        .take(end_idx - start_idx + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

/// BL-126: structural sum of the user-controlled bytes carried by
/// `tx`. Replaces the pre-BL-126 `serde_json::to_vec(&tx_value).len()`
/// pre-check that paid for a throwaway full-tx serialize on the
/// typing hot path.
///
/// Counts the bytes of every string field that scales with payload
/// (`text` / `deleted_text` / block content / content-update old/new)
/// plus a fixed-cost approximation for each annotation. Constant-
/// cost fields (UUIDs, positions, structural metadata) are
/// excluded — they're bounded by op count, which the caller already
/// bounds implicitly through the same cap (a 16 MiB tx ceiling
/// translates to ≥ tens of millions of zero-payload ops, far past
/// the CRDT engine's intended operating range).
///
/// Annotations get a fixed 64-byte allowance per entry — the typed
/// `Annotation` is at most `{ start, end, ty }` where `ty` is a
/// small enum, and the empirical max annotation payload (a wikilink
/// with a long path) is ~200 bytes. 64 is a conservative average
/// that bounds the worst case to within ~3x the JSON-serialized
/// byte count, well under the 16 MiB safety ceiling.
fn transaction_payload_size(tx: &crate::Transaction) -> usize {
    tx.operations.iter().map(op_payload_size).sum()
}

fn op_payload_size(op: &crate::Operation) -> usize {
    use crate::Operation;
    const PER_ANNOTATION: usize = 64;
    let ann_cost = |xs: &[crate::Annotation]| xs.len() * PER_ANNOTATION;
    match op {
        Operation::InsertText {
            text, pre_annotations, ..
        } => text.len() + ann_cost(pre_annotations),
        Operation::DeleteText {
            deleted_text,
            pre_annotations,
            ..
        } => deleted_text.len() + ann_cost(pre_annotations),
        Operation::InsertBlock { block, .. } => {
            block.content.len() + ann_cost(&block.annotations)
        }
        Operation::DeleteBlock { old_block, .. } => {
            old_block.content.len() + ann_cost(&old_block.annotations)
        }
        Operation::ReparentBlock { .. } => 0,
        Operation::UpdateBlockContent {
            old_content,
            new_content,
            old_annotations,
            new_annotations,
            ..
        } => {
            old_content.len()
                + new_content.len()
                + ann_cost(old_annotations)
                + ann_cost(new_annotations)
        }
        Operation::UpdateAnnotations {
            old_annotations,
            new_annotations,
            ..
        } => ann_cost(old_annotations) + ann_cost(new_annotations),
    }
}

fn handle_apply_transaction(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "apply_transaction")?;
    let tx_value = args
        .get("transaction")
        .ok_or_else(|| exec_err("apply_transaction: missing 'transaction'".to_string()))?
        .clone();
    // Cap the transaction payload before applying. Issue #85's
    // original implementation re-serialized `tx_value` via
    // `serde_json::to_vec(&tx_value)` purely to count JSON bytes —
    // ~10–20% of the BL-122-measured small-tx latency was spent in
    // that throwaway serialize on the typing hot path. BL-126
    // replaces it with a structural sum (op-by-op string-length
    // tally on the typed `Transaction`); cheaper and bounded against
    // the same conceptual 16 MiB limit. A small headroom margin
    // accounts for JSON's per-string overhead — the limit is on
    // bytes of user-controlled payload, not exact JSON-serialized
    // bytes, so we under-bound rather than over-bound to keep the
    // safety property.
    const MAX_TRANSACTION_BYTES: usize = 16 * 1024 * 1024;
    let tx: crate::Transaction = serde_json::from_value(tx_value)
        .map_err(|e| exec_err(format!("apply_transaction: invalid transaction: {e}")))?;
    let tx_size = transaction_payload_size(&tx);
    if tx_size > MAX_TRANSACTION_BYTES {
        return Err(exec_err(format!(
            "apply_transaction: transaction payload is {tx_size} bytes; \
             max is {MAX_TRANSACTION_BYTES} bytes"
        )));
    }
    let tx_id = tx.id;
    let op_count = tx.operations.len();
    // BL-123: text-only ops (insert_text / delete_text) get a slim
    // response. UpdateAnnotations stays on the full path because the
    // bridge's optimistic mirror doesn't track annotations — the
    // snapshot is the only authoritative source for the post-apply
    // annotation list.
    let text_only = !tx.operations.is_empty()
        && tx.operations.iter().all(|op| {
            matches!(
                op,
                crate::Operation::InsertText { .. } | crate::Operation::DeleteText { .. }
            )
        });

    // BL-122 / BL-126: instrumentation span for the typing-latency
    // perf harness. Records per-call op count + transaction payload
    // bytes (structural sum of insert/delete/content strings), and
    // (after serialize) bytes-out. A subscriber installed at `info`
    // level captures wall-time via `span.enter()`/exit; with no
    // subscriber the span is a no-op pointer bump.
    let span = tracing::info_span!(
        "apply_transaction",
        op_count,
        text_only,
        payload_bytes = tx_size,
        bytes_out = tracing::field::Empty,
    );
    let _enter = span.enter();

    let entry = acquire_session_entry(sessions, &relpath, "apply_transaction")?;
    let (response, revision, applied_ops) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        // BL-141 Phase 2 — multibuffer sessions accept per-character
        // text ops (`InsertText` / `DeleteText`) as well as the
        // Approach-A `UpdateBlockContent` commit op, all targeted at
        // Excerpt blocks. Structural ops (`InsertBlock` /
        // `DeleteBlock` / `ReparentBlock` / `UpdateAnnotations`) stay
        // rejected — they don't have a clean line-range splice mapping,
        // and they'd also corrupt the synthetic tree's
        // Excerpt-only invariant.
        //
        // Approach B step 2 (this gate-widening) treats each Excerpt's
        // `content` as the authoritative text the user is editing.
        // Save (handle_save) still walks every Excerpt and splices the
        // current content back into the source file via
        // `splice_excerpts`, so the on-disk round-trip is unchanged.
        // The user-visible win is per-keystroke editing instead of
        // "edit-in-textarea, dispatch one UpdateBlockContent on commit".
        //
        // Source-session dispatch (the original Approach B sketch) is
        // not needed at this layer: external-edit sync (step 3) and
        // line-range drift (step 4) operate by re-slicing source text,
        // not by replaying ops on the source `Session`.
        if s.is_synthetic {
            for op in &tx.operations {
                let id: &uuid::Uuid = match op {
                    crate::Operation::InsertText { block_id, .. }
                    | crate::Operation::DeleteText { block_id, .. } => block_id,
                    crate::Operation::UpdateBlockContent { id, .. } => id,
                    _ => {
                        return Err(exec_err(format!(
                            "apply_transaction: session '{relpath}' is a \
                             multibuffer; only InsertText / DeleteText / \
                             UpdateBlockContent ops on Excerpt blocks are \
                             accepted in Phase 2 (got a non-content op — \
                             InsertBlock / DeleteBlock / ReparentBlock / \
                             UpdateAnnotations have no line-range splice \
                             mapping and would corrupt the synthetic tree's \
                             Excerpt-only invariant)"
                        )));
                    }
                };
                let target = s.tree.blocks.get(id).ok_or_else(|| {
                    exec_err(format!(
                        "apply_transaction: multibuffer block {id} not found"
                    ))
                })?;
                if !matches!(target.ty, BlockType::Excerpt { .. }) {
                    return Err(exec_err(format!(
                        "apply_transaction: multibuffer block {id} is not \
                         an Excerpt block; only Excerpt blocks are editable \
                         in a synthetic session"
                    )));
                }
            }
        }
        // BL-073: capture the operation set before consuming `tx` so we
        // can scan new wikilink / block-ref annotations after apply and
        // auto-stamp inbound-link targets. The post-apply scan reads
        // the freshly-mutated block content (wikilink fragments live in
        // the source text, not the annotation payload), so it has to
        // run after `execute` returns. BL-074: same captured `ops` is
        // handed to the observer (out of band from the lock).
        let ops = tx.operations.clone();
        s.undo
            .execute(tx, &mut s.tree)
            .map_err(|e| exec_err(format!("apply_transaction: {e}")))?;
        auto_stamp_inbound_targets(&mut s.tree, &ops);
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let response = if text_only {
            ApplyTransactionResponse::Slim { revision: rev }
        } else {
            ApplyTransactionResponse::Full(snapshot_of(s))
        };
        (response, rev, ops)
    };
    let value = serde_json::to_value(&response)
        .map_err(|e| exec_err(format!("apply_transaction: serialize response: {e}")))?;
    if let Ok(buf) = serde_json::to_vec(&value) {
        span.record("bytes_out", buf.len());
    }
    if let Some(obs) = observer {
        obs.on_apply_transaction(&relpath, &applied_ops);
    }
    publish_changed(event_bus, &relpath, revision, Some(tx_id));
    Ok(value)
}

/// BL-073 helper: stamp every block in `tree` that newly became the
/// target of an inbound `Wikilink` (with a `#^<uuid>` fragment) or
/// `BlockRef` annotation introduced by `ops`. Stamping rekeys the
/// target's positional id to a fresh v4 UUID and sets `stable_id` so
/// the next save persists a `<!-- ^<uuid> -->` marker. Idempotent —
/// blocks that already carry a `stable_id` are skipped, and any
/// stamping failure (block missing, rekey collision) is silent: the
/// transaction itself already committed and shouldn't be invalidated
/// by a metadata-only side effect.
fn auto_stamp_inbound_targets(tree: &mut BlockTree, ops: &[crate::Operation]) {
    use crate::Operation;

    let mut targets: Vec<uuid::Uuid> = Vec::new();
    for op in ops {
        let (host_id, old, new) = match op {
            Operation::UpdateAnnotations {
                block_id,
                old_annotations,
                new_annotations,
            } => (*block_id, old_annotations.as_slice(), new_annotations.as_slice()),
            Operation::UpdateBlockContent {
                id,
                old_annotations,
                new_annotations,
                ..
            } => (*id, old_annotations.as_slice(), new_annotations.as_slice()),
            _ => continue,
        };
        // Annotations carry their own equality, so the simplest "what's
        // new" view is set difference by structural equality. The
        // annotation count per block is small (<100 in realistic docs),
        // so the O(n*m) scan is fine.
        for ann in new {
            if old.iter().any(|prev| prev == ann) {
                continue;
            }
            if let Some(target) = inbound_target(tree, host_id, ann) {
                targets.push(target);
            }
        }
    }

    targets.sort_unstable();
    targets.dedup();
    for old_id in targets {
        let needs_stamp = matches!(tree.get(old_id), Some(b) if b.stable_id.is_none());
        if !needs_stamp {
            continue;
        }
        let new_id = uuid::Uuid::new_v4();
        // Best-effort: a rekey collision (impossibly rare with v4) or
        // a block that disappeared between scan and stamp is harmless
        // — the user's link still resolves on the next explicit
        // `stamp_block` or `resolve_block_link` call.
        let _ = tree.rekey(old_id, new_id);
    }
}

/// Resolve a single annotation to the in-tree block id it points at,
/// when the annotation is one of the inbound-link kinds that BL-073
/// auto-stamps. Returns the *current* (positional) id of the target
/// so the caller can pass it to [`BlockTree::rekey`].
///
/// `Wikilink`s carry only the file part of the path in their payload
/// (the fragment is dropped at parse time per
/// `markdown::inline::parse_wikilink_inner`), so we recover the
/// `#^<uuid>` fragment from the *content* of the host block — the
/// raw `[[file#^uuid]]` text lives there and the annotation's
/// `start`/`end` byte range pins it down.
fn inbound_target(
    tree: &BlockTree,
    host_id: uuid::Uuid,
    ann: &crate::Annotation,
) -> Option<uuid::Uuid> {
    use crate::AnnotationType;
    match &ann.ty {
        AnnotationType::BlockRef { block_id } => Some(*block_id),
        AnnotationType::Wikilink { .. } => {
            let host = tree.get(host_id)?;
            let bytes = host.content.as_bytes();
            if ann.end > bytes.len() || ann.start >= ann.end {
                return None;
            }
            let slice = std::str::from_utf8(&bytes[ann.start..ann.end]).ok()?;
            extract_wikilink_block_uuid(slice)
        }
        _ => None,
    }
}

/// Parse a `[[...]]` literal and return the block uuid encoded in its
/// `#^<uuid>` fragment, when present. Returns `None` for path-only
/// links, fragment-less links, heading-only fragments (`#section`,
/// not `#^uuid`), and any uuid parse failure. Mirrors
/// `markdown::inline::parse_wikilink_inner` but keeps only the
/// fragment branch we care about.
fn extract_wikilink_block_uuid(literal: &str) -> Option<uuid::Uuid> {
    let inner = literal.strip_prefix("[[")?.strip_suffix("]]")?;
    // Display-text suffix (`|display`) is stripped first so we don't
    // confuse a `#` inside the display text with the path fragment.
    let target = match inner.find('|') {
        Some(pipe) => &inner[..pipe],
        None => inner,
    };
    let hash = target.find('#')?;
    let fragment = &target[hash + 1..];
    let id_str = fragment.strip_prefix('^')?;
    uuid::Uuid::parse_str(id_str).ok()
}

fn handle_undo(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "undo")?;
    // Capture the transaction being reversed *before* the undo runs
    // so the observer can author inverse ops against the
    // pre-undo (post-tx) state of its own mirror tree.
    let entry = acquire_session_entry(sessions, &relpath, "undo")?;
    let (value, revision, captured) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        let cur_tx = s
            .undo
            .current()
            .map(|idx| Arc::clone(&s.undo.transactions()[idx]));
        s.undo
            .undo(&mut s.tree)
            .map_err(|e| exec_err(format!("undo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "undo")?;
        let post_tree = s.tree.clone();
        (val, rev, cur_tx.map(|tx| (tx, post_tree)))
    };
    if let (Some(obs), Some((tx, post_tree))) = (observer, captured.as_ref()) {
        obs.on_undo_transaction(&relpath, tx, post_tree);
    }
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

fn handle_redo(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "redo")?;
    let entry = acquire_session_entry(sessions, &relpath, "redo")?;
    let (value, revision, captured) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        s.undo
            .redo(&mut s.tree)
            .map_err(|e| exec_err(format!("redo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "redo")?;
        // Post-redo, `current` points at the just-replayed tx.
        let replayed_tx = s
            .undo
            .current()
            .map(|idx| Arc::clone(&s.undo.transactions()[idx]));
        let post_tree = s.tree.clone();
        (val, rev, replayed_tx.map(|tx| (tx, post_tree)))
    };
    if let (Some(obs), Some((tx, post_tree))) = (observer, captured.as_ref()) {
        obs.on_redo_transaction(&relpath, tx, post_tree);
    }
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

fn handle_list_open(sessions: &SessionMap) -> Result<Value, PluginError> {
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

/// Serialize the session's block tree to markdown and return it as a
/// bare JSON string. Matches `serialize_session` but surfaces the
/// result over IPC rather than routing it to disk.
fn handle_get_markdown(
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_markdown")?;
    let entry = acquire_session_entry(sessions, &relpath, "get_markdown")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
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
    sessions: &SessionMap,
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

    let entry = acquire_session_entry(sessions, &relpath, "stamp_block")?;
    let (stable_id, newly_stamped, revision) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
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
mod splice_tests {
    use super::{splice_excerpts, ExcerptSplice};

    fn sp(line_start: u32, line_end: u32, new_content: &str) -> ExcerptSplice {
        ExcerptSplice {
            line_start,
            line_end,
            new_content: new_content.to_string(),
        }
    }

    #[test]
    fn splice_single_range_preserves_surrounding_lines() {
        let old = "L1\nL2\nL3\nL4\nL5\n";
        let got = splice_excerpts(old, vec![sp(2, 4, "X\nY")]);
        assert_eq!(got, "L1\nX\nY\nL5\n");
    }

    #[test]
    fn splice_preserves_no_trailing_newline_when_source_had_none() {
        let old = "L1\nL2\nL3";
        let got = splice_excerpts(old, vec![sp(2, 2, "MID")]);
        assert_eq!(got, "L1\nMID\nL3");
    }

    #[test]
    fn splice_handles_growing_replacement() {
        let old = "A\nB\nC\n";
        let got = splice_excerpts(old, vec![sp(2, 2, "B1\nB2\nB3")]);
        assert_eq!(got, "A\nB1\nB2\nB3\nC\n");
    }

    #[test]
    fn splice_handles_shrinking_replacement() {
        let old = "A\nB\nC\nD\nE\n";
        let got = splice_excerpts(old, vec![sp(2, 4, "B+D")]);
        assert_eq!(got, "A\nB+D\nE\n");
    }

    #[test]
    fn splice_processes_multiple_ranges_in_reverse_order() {
        // Two non-overlapping splices. The first one (lines 1-2) is
        // a 2→1 line shrink; the second (lines 4-5) replaces with
        // a 2→3 line grow. If we applied them in input order the
        // first splice would shift the second's target range and
        // we'd splice the wrong lines. Reverse-order processing
        // dodges this entirely.
        let old = "A\nB\nC\nD\nE\n";
        let got = splice_excerpts(
            old,
            vec![sp(1, 2, "AB"), sp(4, 5, "DE1\nDE2\nDE3")],
        );
        assert_eq!(got, "AB\nC\nDE1\nDE2\nDE3\n");
    }

    #[test]
    fn splice_out_of_range_start_is_skipped_defensively() {
        // The excerpt range starts past EOF — e.g. a stale
        // multibuffer whose source was truncated externally. Better
        // to preserve the existing source than to append the edit
        // into an arbitrary spot.
        let old = "A\nB\n";
        let got = splice_excerpts(old, vec![sp(10, 12, "ignored")]);
        assert_eq!(got, "A\nB\n");
    }

    #[test]
    fn splice_clamps_end_at_eof() {
        let old = "A\nB\nC\n";
        let got = splice_excerpts(old, vec![sp(2, 99, "X")]);
        assert_eq!(got, "A\nX\n");
    }
}

#[cfg(test)]
mod excerpt_sources_tests {
    use super::excerpt_sources;
    use crate::block::{Block, BlockType};
    use crate::tree::BlockTree;

    fn excerpt(source_relpath: &str, line_start: u32, line_end: u32) -> Block {
        Block::new(BlockType::Excerpt {
            source_relpath: source_relpath.to_string(),
            line_start,
            line_end,
            label: None,
        })
    }

    #[test]
    fn excerpt_sources_empty_tree_returns_empty() {
        let tree = BlockTree::default();
        assert!(excerpt_sources(&tree).is_empty());
    }

    #[test]
    fn excerpt_sources_dedupes_in_first_appearance_order() {
        let mut tree = BlockTree::default();
        for (i, b) in [
            excerpt("a.md", 1, 5),
            excerpt("b.md", 1, 5),
            excerpt("a.md", 10, 15),
            excerpt("c.md", 1, 5),
            excerpt("b.md", 20, 30),
        ]
        .into_iter()
        .enumerate()
        {
            tree.insert(b, None, i).unwrap();
        }
        assert_eq!(excerpt_sources(&tree), vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn excerpt_sources_skips_non_excerpt_root_blocks() {
        let mut tree = BlockTree::default();
        tree.insert(Block::new(BlockType::Paragraph).with_content("nope"), None, 0)
            .unwrap();
        tree.insert(excerpt("only.md", 1, 5), None, 1).unwrap();
        assert_eq!(excerpt_sources(&tree), vec!["only.md"]);
    }
}

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

        // BL-123: InsertText is text-only → slim response. The full
        // snapshot is fetched separately via get_tree for the asserts.
        let response = apply_value(&mut p, "notes/a.md", &tx);
        assert!(
            matches!(response, ApplyTransactionResponse::Slim { revision: 1 }),
            "expected slim response with revision 1, got {response:?}",
        );
        let snap = get_tree_value(&mut p, "notes/a.md");
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

    /// BL-126: structural-bound payload-size check.
    ///
    /// The pre-BL-126 implementation re-serialized `tx_value` via
    /// `serde_json::to_vec` purely to count bytes, which paid a
    /// throwaway full-tx serialize on every keystroke. BL-126
    /// replaces that with a structural sum over typed op fields.
    /// This test pins three properties:
    ///   1. A merely-large transaction (within the 16 MiB ceiling)
    ///      goes through the apply path unaffected.
    ///   2. A transaction whose `InsertText` payload alone exceeds
    ///      the ceiling is rejected by the structural cap.
    ///   3. The helper's per-op accounting matches the documented
    ///      breakdown (text string lengths + per-annotation fixed
    ///      cost).
    #[test]
    fn apply_transaction_rejects_payload_above_structural_cap() {
        use crate::{Operation, Transaction, TransactionMetadata};
        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "Hello\n");
        let mut p = new_plugin(root);
        let snap = open_value(&mut p, "notes/a.md");
        let para_id = snap.tree.root_blocks[0];

        // Property 3: payload-size helper accounts for InsertText
        // string length + per-annotation overhead.
        let tx_small = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: 0,
                text: "xxxxxxxxxx".into(), // 10 bytes
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        assert_eq!(
            transaction_payload_size(&tx_small),
            10,
            "InsertText payload cost == text byte length",
        );

        // Property 1: a small transaction goes through.
        let resp = apply_value(&mut p, "notes/a.md", &tx_small);
        assert!(matches!(resp, ApplyTransactionResponse::Slim { .. }));

        // Property 2: an oversized transaction is rejected before
        // any mutation. Build a single InsertText whose `text` is
        // 17 MiB (1 MiB above the 16 MiB ceiling).
        const MAX: usize = 16 * 1024 * 1024;
        let big = "x".repeat(MAX + 1024 * 1024);
        let big_len = big.len();
        let tx_big = Transaction::new(
            vec![Operation::InsertText {
                block_id: para_id,
                pos: 0,
                text: big,
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        // The check is on payload size, so the helper should report
        // the full 17 MiB rather than the JSON-encoded byte count.
        assert!(transaction_payload_size(&tx_big) > MAX);
        let err = p
            .dispatch(
                HANDLER_APPLY_TRANSACTION,
                &serde_json::json!({
                    "relpath": "notes/a.md",
                    "transaction": serde_json::to_value(&tx_big).unwrap(),
                }),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("payload is") && msg.contains("max is"),
            "rejection mentions payload bytes vs cap: {msg}",
        );
        assert!(
            msg.contains(&format!("{big_len}")),
            "rejection includes actual payload size {big_len}: {msg}",
        );
        // The pre-reject path must not have mutated the session —
        // get_tree returns the post-tx_small state, no further
        // mutations.
        let post = get_tree_value(&mut p, "notes/a.md");
        assert_eq!(post.tree.blocks[&para_id].content, "xxxxxxxxxxHello");
    }

    /// BL-123: response-shape contract. Text-only ops (`insert_text`,
    /// `delete_text`, and any mix of the two) get a slim response;
    /// every other op kind — including `update_annotations` — gets a
    /// full snapshot. Empty op lists get the full response too (no
    /// text-only ops to apply, but the safe default is to return the
    /// snapshot so callers can detect the no-op via revision parity).
    #[test]
    fn apply_transaction_response_shape_per_op_kind() {
        use crate::{
            Annotation, AnnotationType, Operation, Transaction, TransactionMetadata,
        };
        let (_tmp, root) = setup_forge();
        write_note(
            &root,
            "notes/a.md",
            "first paragraph\n\nsecond paragraph\n",
        );
        let mut p = new_plugin(root);
        let snap = open_value(&mut p, "notes/a.md");
        let block_id = snap.tree.root_blocks[0];
        let block = &snap.tree.blocks[&block_id];

        // 1. InsertText → slim.
        let insert = Transaction::new(
            vec![Operation::InsertText {
                block_id,
                pos: block.content.len(),
                text: " more".into(),
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        let resp = apply_value(&mut p, "notes/a.md", &insert);
        assert!(matches!(resp, ApplyTransactionResponse::Slim { .. }));

        // 2. DeleteText → slim.
        let post = get_tree_value(&mut p, "notes/a.md");
        let post_block = &post.tree.blocks[&block_id];
        let delete = Transaction::new(
            vec![Operation::DeleteText {
                block_id,
                pos: 0,
                deleted_text: post_block.content.chars().next().unwrap().to_string(),
                pre_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        let resp = apply_value(&mut p, "notes/a.md", &delete);
        assert!(matches!(resp, ApplyTransactionResponse::Slim { .. }));

        // 3. InsertText + DeleteText combined → still slim.
        let post = get_tree_value(&mut p, "notes/a.md");
        let post_block = &post.tree.blocks[&block_id];
        let combined = Transaction::new(
            vec![
                Operation::InsertText {
                    block_id,
                    pos: 0,
                    text: "X".into(),
                    pre_annotations: post_block.annotations.clone(),
                },
                Operation::DeleteText {
                    block_id,
                    pos: 0,
                    deleted_text: "X".into(),
                    pre_annotations: post_block.annotations.clone(),
                },
            ],
            TransactionMetadata::default(),
        );
        let resp = apply_value(&mut p, "notes/a.md", &combined);
        assert!(matches!(resp, ApplyTransactionResponse::Slim { .. }));

        // 4. UpdateAnnotations → full (the bridge's optimistic mirror
        //    doesn't track annotations, so the snapshot is the only
        //    authoritative source for the post-apply annotation list).
        let post = get_tree_value(&mut p, "notes/a.md");
        let post_block = &post.tree.blocks[&block_id];
        let ann_tx = Transaction::new(
            vec![Operation::UpdateAnnotations {
                block_id,
                old_annotations: post_block.annotations.clone(),
                new_annotations: vec![Annotation {
                    start: 0,
                    end: 1,
                    ty: AnnotationType::Bold,
                }],
            }],
            TransactionMetadata::default(),
        );
        let resp = apply_value(&mut p, "notes/a.md", &ann_tx);
        assert!(matches!(resp, ApplyTransactionResponse::Full(_)));

        // 5. UpdateBlockContent → full.
        let post = get_tree_value(&mut p, "notes/a.md");
        let post_block = &post.tree.blocks[&block_id];
        let ubc = Transaction::new(
            vec![Operation::UpdateBlockContent {
                id: block_id,
                old_content: post_block.content.clone(),
                new_content: "rewritten".to_string(),
                old_annotations: post_block.annotations.clone(),
                new_annotations: Vec::new(),
            }],
            TransactionMetadata::default(),
        );
        let resp = apply_value(&mut p, "notes/a.md", &ubc);
        assert!(matches!(resp, ApplyTransactionResponse::Full(_)));

        // Wire shape spot-check: slim serializes with the discriminator
        // and just the revision field; full serializes with the
        // discriminator and the flattened EditorSnapshot fields.
        let slim = ApplyTransactionResponse::Slim { revision: 7 };
        let slim_json = serde_json::to_value(&slim).unwrap();
        assert_eq!(slim_json["kind"], "slim");
        assert_eq!(slim_json["revision"], 7);
        assert!(slim_json.get("tree").is_none());

        let snap = get_tree_value(&mut p, "notes/a.md");
        let full = ApplyTransactionResponse::Full(snap);
        let full_json = serde_json::to_value(&full).unwrap();
        assert_eq!(full_json["kind"], "full");
        assert!(full_json.get("tree").is_some());
        assert!(full_json.get("revision").is_some());
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
        // BL-123: InsertText is text-only → slim response.
        let response = apply_value(&mut p, "notes/a.md", &tx);
        assert_eq!(
            response.revision(),
            1,
            "apply_transaction bumps revision (slim path)",
        );

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
        // BL-073: the first resolve against an unstamped block
        // auto-stamps it, so the response carries a fresh `stable_id`
        // (not the original positional id) and the block's `id` field
        // is rekeyed to match.
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
        let resolved_id = block.get("id").and_then(Value::as_str).unwrap();
        let stable_id = block.get("stable_id").and_then(Value::as_str).unwrap();
        assert_eq!(resolved_id, stable_id, "id and stable_id match after stamp");
        assert_ne!(
            resolved_id,
            block_id.to_string(),
            "auto-stamp must rekey to a fresh uuid"
        );
        // Resolving the same lookup again hits the already-stamped
        // path and is a no-op.
        let resp2 = p
            .dispatch(
                HANDLER_RESOLVE_BLOCK_LINK,
                &serde_json::json!({
                    "file_relpath": "notes/a.md",
                    "block_id": resolved_id,
                }),
            )
            .unwrap();
        let block2 = resp2.get("block").unwrap();
        assert_eq!(
            block2.get("id").and_then(Value::as_str),
            Some(resolved_id),
            "second resolve preserves the stamped id"
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

    #[test]
    fn extract_wikilink_block_uuid_handles_common_shapes() {
        let id = uuid::Uuid::new_v4();
        let with_fragment = format!("[[notes/foo#^{id}]]");
        assert_eq!(extract_wikilink_block_uuid(&with_fragment), Some(id));

        let with_display = format!("[[notes/foo#^{id}|see this]]");
        assert_eq!(extract_wikilink_block_uuid(&with_display), Some(id));

        // Heading fragments aren't block refs.
        assert_eq!(
            extract_wikilink_block_uuid("[[notes/foo#section]]"),
            None,
        );
        // Path-only links have no fragment to stamp against.
        assert_eq!(extract_wikilink_block_uuid("[[notes/foo]]"), None);
        // Fragment present but not a uuid.
        assert_eq!(extract_wikilink_block_uuid("[[notes/foo#^abc]]"), None);
    }

    #[test]
    fn apply_transaction_auto_stamps_block_ref_target() {
        // BL-073: a transaction that adds an inbound `BlockRef`
        // annotation pointing at an unstamped block must auto-stamp
        // the target so the link can survive the next reload.
        use crate::{Annotation, AnnotationType, Operation, Transaction, TransactionMetadata};
        let (_dir, mut p) = forge_with_file(
            "notes/a.md",
            "first paragraph\n\nsecond paragraph\n",
        );
        let snap = open_value(&mut p, "notes/a.md");
        let source_id = snap.tree.root_blocks[0];
        let target_id = snap.tree.root_blocks[1];
        assert!(snap.tree.blocks[&target_id].stable_id.is_none());

        let source_block = &snap.tree.blocks[&source_id];
        let new_anns = vec![Annotation {
            start: 0,
            end: 1,
            ty: AnnotationType::BlockRef {
                block_id: target_id,
            },
        }];
        let tx = Transaction::new(
            vec![Operation::UpdateAnnotations {
                block_id: source_id,
                old_annotations: source_block.annotations.clone(),
                new_annotations: new_anns,
            }],
            TransactionMetadata::default(),
        );
        // UpdateAnnotations is structural → full response carrying the
        // post-apply snapshot.
        let response = apply_value(&mut p, "notes/a.md", &tx);
        let snap = response
            .snapshot()
            .cloned()
            .expect("UpdateAnnotations must yield a full snapshot");
        // The target's positional id has been rekeyed; root_blocks[1]
        // now holds the new stamped id.
        let stamped_id = snap.tree.root_blocks[1];
        assert_ne!(stamped_id, target_id, "target was rekeyed");
        let stamped = &snap.tree.blocks[&stamped_id];
        assert_eq!(stamped.stable_id, Some(stamped_id));
    }

    #[test]
    fn apply_transaction_auto_stamps_wikilink_fragment_target() {
        // The wikilink fragment lives in the host block's *content*,
        // not the annotation payload, so auto-stamping has to recover
        // it via byte-slicing the post-apply content.
        use crate::{Annotation, AnnotationType, Operation, Transaction, TransactionMetadata};
        let (_dir, mut p) = forge_with_file(
            "notes/a.md",
            "first paragraph\n\nsecond paragraph\n",
        );
        let snap = open_value(&mut p, "notes/a.md");
        let source_id = snap.tree.root_blocks[0];
        let target_id = snap.tree.root_blocks[1];
        let source_block = &snap.tree.blocks[&source_id];
        let old_content = source_block.content.clone();
        let old_annotations = source_block.annotations.clone();

        // Build content like `<original> [[notes/a#^<target_id>]]` and
        // attach the wikilink annotation over the bracketed range.
        let link_text = format!("[[notes/a#^{target_id}]]");
        let prefix = format!("{old_content} ");
        let new_content = format!("{prefix}{link_text}");
        let link_start = prefix.len();
        let link_end = link_start + link_text.len();
        let new_annotations = vec![Annotation {
            start: link_start,
            end: link_end,
            ty: AnnotationType::Wikilink {
                path: "notes/a".into(),
                display_text: None,
                is_resolved: false,
            },
        }];
        let tx = Transaction::new(
            vec![Operation::UpdateBlockContent {
                id: source_id,
                old_content,
                new_content,
                old_annotations,
                new_annotations,
            }],
            TransactionMetadata::default(),
        );
        // UpdateBlockContent is structural → full response carrying
        // the post-apply snapshot.
        let response = apply_value(&mut p, "notes/a.md", &tx);
        let snap = response
            .snapshot()
            .cloned()
            .expect("UpdateBlockContent must yield a full snapshot");
        let stamped_id = snap.tree.root_blocks[1];
        assert_ne!(stamped_id, target_id, "auto-stamp rekeys to a fresh uuid");
        let stamped = &snap.tree.blocks[&stamped_id];
        assert_eq!(stamped.stable_id, Some(stamped_id));
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

    /// Dispatch `apply_transaction` and return the response, decoded
    /// as a [`ApplyTransactionResponse`] discriminated union (BL-123).
    fn apply_value(
        p: &mut EditorCorePlugin,
        relpath: &str,
        tx: &crate::Transaction,
    ) -> ApplyTransactionResponse {
        let resp = p
            .dispatch(
                HANDLER_APPLY_TRANSACTION,
                &serde_json::json!({
                    "relpath": relpath,
                    "transaction": serde_json::to_value(tx).unwrap(),
                }),
            )
            .unwrap();
        serde_json::from_value(resp).unwrap()
    }

    /// Fetch the current snapshot via `get_tree` (always full).
    fn get_tree_value(p: &mut EditorCorePlugin, relpath: &str) -> EditorSnapshot {
        let resp = p
            .dispatch(
                HANDLER_GET_TREE,
                &serde_json::json!({ "relpath": relpath }),
            )
            .unwrap();
        serde_json::from_value(resp).unwrap()
    }

    /// BL-126 follow-up — proves the per-session-lock invariant: two
    /// relpaths' inner mutexes can be held simultaneously. Pre-refactor
    /// the `Mutex<HashMap<String, Session>>` map-level lock serialised
    /// every access, so a second `acquire_session_entry` would have
    /// blocked behind the first session's outer-lock acquisition (and
    /// the inner `Arc<Mutex<Session>>` didn't exist at all). The
    /// channel-with-timeout assert times out instead of deadlocking
    /// if a regression re-introduces a single shared mutex.
    #[test]
    fn per_session_locks_allow_concurrent_holds_across_relpaths() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "A\n");
        write_note(&root, "notes/b.md", "B\n");
        let mut p = new_plugin(root);
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

        let sessions = Arc::clone(&p.sessions);
        let entry_a = acquire_session_entry(&sessions, "notes/a.md", "test").unwrap();
        let entry_b = acquire_session_entry(&sessions, "notes/b.md", "test").unwrap();

        // Hold A's inner lock on this thread, then spawn a thread that
        // locks B. If the per-session-lock invariant holds, the spawn
        // proceeds without waiting on us.
        let guard_a = entry_a.lock().unwrap();
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            let _guard_b = entry_b.lock().unwrap();
            tx.send(()).unwrap();
            // Hold B's lock briefly so the two guards overlap on the wall
            // clock — both inner mutexes are held at once when this
            // sleep returns.
            std::thread::sleep(Duration::from_millis(20));
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("per-session lock for b.md should not block while a.md is held");
        drop(guard_a);
        t.join().unwrap();
    }

    /// BL-126 follow-up — drives `handle_apply_transaction` from two
    /// threads against two different relpaths. The test fails by
    /// deadlock (caught by the test runner's wall-clock cap) or by a
    /// missing revision bump if a regression re-introduces the
    /// single-map-mutex contention pattern. Each thread fires 100
    /// inserts; if the inner locks worked the threads run independently
    /// and both sessions reach revision 100.
    #[test]
    fn concurrent_apply_transaction_against_different_relpaths_does_not_deadlock() {
        use crate::{Operation, Transaction, TransactionMetadata};

        const ROUNDS: usize = 100;

        let (_tmp, root) = setup_forge();
        write_note(&root, "notes/a.md", "A\n");
        write_note(&root, "notes/b.md", "B\n");
        let mut p = new_plugin(root);
        let snap_a: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/a.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let snap_b: EditorSnapshot = serde_json::from_value(
            p.dispatch(
                HANDLER_OPEN,
                &serde_json::json!({ "relpath": "notes/b.md" }),
            )
            .unwrap(),
        )
        .unwrap();
        let para_a = snap_a.tree.root_blocks[0];
        let para_b = snap_b.tree.root_blocks[0];

        let make_tx = |block_id: uuid::Uuid, ch: &str| {
            serde_json::to_value(Transaction::new(
                vec![Operation::InsertText {
                    block_id,
                    pos: 1,
                    text: ch.into(),
                    pre_annotations: Vec::new(),
                }],
                TransactionMetadata::default(),
            ))
            .unwrap()
        };

        let sessions_a = Arc::clone(&p.sessions);
        let sessions_b = Arc::clone(&p.sessions);
        let tx_a = make_tx(para_a, "x");
        let tx_b = make_tx(para_b, "y");

        let h_a = std::thread::spawn(move || {
            for _ in 0..ROUNDS {
                handle_apply_transaction(
                    &sessions_a,
                    None,
                    None,
                    &serde_json::json!({
                        "relpath": "notes/a.md",
                        "transaction": tx_a.clone(),
                    }),
                )
                .unwrap();
            }
        });
        let h_b = std::thread::spawn(move || {
            for _ in 0..ROUNDS {
                handle_apply_transaction(
                    &sessions_b,
                    None,
                    None,
                    &serde_json::json!({
                        "relpath": "notes/b.md",
                        "transaction": tx_b.clone(),
                    }),
                )
                .unwrap();
            }
        });
        h_a.join().unwrap();
        h_b.join().unwrap();

        let snap_a = get_tree_value(&mut p, "notes/a.md");
        let snap_b = get_tree_value(&mut p, "notes/b.md");
        assert_eq!(snap_a.revision, ROUNDS as u64);
        assert_eq!(snap_b.revision, ROUNDS as u64);
    }
}
