//! Core plugin: bridges the forge watcher to the kernel event bus.
//!
//! Registers as `com.nexus.storage` and translates [`StorageEvent`]s into
//! `com.nexus.storage.*` custom events on the kernel event bus.
//!
//! # Re-indexing
//!
//! The bridge thread publishes bus events; it does **not** update the `SQLite`
//! index.  Callers that need real-time index updates should call
//! [`StorageEngine::process_watcher_events`] on their own polling loop, or
//! call [`StorageEngine::rebuild_index`] / [`StorageEngine::reconcile_index`]
//! explicitly after batches of changes.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use nexus_kernel::{EventBus, EventFilter};
use nexus_plugins::{CorePlugin, PluginError};

use crate::{StorageConfig, StorageEngine};
use crate::watcher::{StorageEvent, Watcher};

/// Topic the git plugin emits on every HEAD change; the storage
/// plugin subscribes here so that an external pull / rebase / checkout
/// triggers an incremental reconcile and the code-symbol index stays
/// fresh without the user needing to manually rebuild. See
/// `crates/nexus-git/src/core_plugin.rs` for the producer side.
const GIT_COMMIT_TOPIC: &str = "com.nexus.git.commit";

/// P2-06 — tick interval for the BL-114 git-commit subscriber thread.
/// Override via a future StorageConfig field; today the const is the
/// only source.
pub const DEFAULT_GIT_COMMIT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const GIT_COMMIT_POLL_INTERVAL: Duration = DEFAULT_GIT_COMMIT_POLL_INTERVAL;

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.storage";

// ── IPC handler ids ──────────────────────────────────────────────────────────
//
// These are stable within the plugin — the manifest in nexus-bootstrap maps
// command ids to these numbers. If you add a handler, append; never reuse a
// retired id.

/// Handler id for `query_files`. Args: [`FileFilter`]; Returns: `Vec<FileRecord>`.
pub const HANDLER_QUERY_FILES: u32 = 1;
/// Handler id for `read_file`. Args: `{ "path": String }`; Returns:
/// `{ "bytes": Option<Vec<u8>> }` — `null` when the file does not exist,
/// so callers can distinguish a missing file from a genuine failure without
/// the IPC layer collapsing the error into `PluginCrashedDuringCall`.
pub const HANDLER_READ_FILE: u32 = 2;
/// Handler id for `backlinks`. Args: `{ "path": String }`; Returns: `Vec<BacklinkResult>`.
pub const HANDLER_BACKLINKS: u32 = 3;
/// Handler id for `query_tasks`. Args: [`TaskFilter`]; Returns: `Vec<TaskRecord>`.
pub const HANDLER_QUERY_TASKS: u32 = 4;
/// Handler id for `graph_stats`. Args: `{}`; Returns: [`crate::GraphStats`].
pub const HANDLER_GRAPH_STATS: u32 = 5;
/// Handler id for `rebuild_index`. Args: `{}`; Returns: [`crate::RebuildStats`].
pub const HANDLER_REBUILD_INDEX: u32 = 6;
/// Handler id for `search`. Args: `{ "query": String, "limit": usize }`; Returns: `Vec<SearchResult>`.
pub const HANDLER_SEARCH: u32 = 7;
/// Handler id for `write_file`. Args: `{ "path": String, "bytes": Vec<u8> }`; Returns: [`crate::FileMetadata`].
pub const HANDLER_WRITE_FILE: u32 = 8;
/// Handler id for `delete_file`. Args: `{ "path": String }`; Returns: `{}`.
pub const HANDLER_DELETE_FILE: u32 = 9;
/// Handler id for `file_exists`. Args: `{ "path": String }`; Returns: `{ "exists": bool }`.
pub const HANDLER_FILE_EXISTS: u32 = 10;
/// Handler id for `rebuild_search_index`. Args: `{}`; Returns: `{}`.
pub const HANDLER_REBUILD_SEARCH_INDEX: u32 = 11;
/// Handler id for `toggle_task`. Args: `{ "task_id": u64 }`; Returns: [`crate::TaskRecord`].
pub const HANDLER_TOGGLE_TASK: u32 = 12;
/// Handler id for `outgoing_links`. Args: `{ "path": String }`; Returns: `Vec<OutgoingLink>`.
pub const HANDLER_OUTGOING_LINKS: u32 = 13;
/// Handler id for `unresolved_links`. Args: `{}`; Returns: `Vec<UnresolvedLink>`.
pub const HANDLER_UNRESOLVED_LINKS: u32 = 14;
/// Handler id for `graph_neighbors`. Args: `{ "path": String, "depth": usize }`; Returns: `Vec<String>`.
pub const HANDLER_GRAPH_NEIGHBORS: u32 = 15;
/// Handler id for `query_tags`. Args: `{ "name": String }`; Returns: `Vec<TagResult>`.
pub const HANDLER_QUERY_TAGS: u32 = 16;
/// Handler id for `vector_insert`. Args: `{ "file_path": String, "chunks": Vec<ChunkEmbedding> }`; Returns: `{}`.
pub const HANDLER_VECTOR_INSERT: u32 = 17;
/// Handler id for `vector_query`. Args: `{ "embedding": Vec<f32>, "limit": usize }`; Returns: `Vec<ChunkMatch>`.
pub const HANDLER_VECTOR_QUERY: u32 = 18;
/// Handler id for `vector_delete_by_file`. Args: `{ "path": String }`; Returns: `{}`.
pub const HANDLER_VECTOR_DELETE_BY_FILE: u32 = 19;
/// Handler id for `vectorstore_count`. Args: `{}`; Returns: `{ "count": usize }`.
pub const HANDLER_VECTORSTORE_COUNT: u32 = 20;
/// Handler id for `query_blocks`. Args: `{ "path": String }`; Returns: `Vec<BlockRecord>`.
pub const HANDLER_QUERY_BLOCKS: u32 = 21;
/// Handler id for `config_read`. Args: `{ "kind": "app"|"workspace"|"mcp"|"ai" }`;
/// Returns: `{ "format": "toml"|"json", "content": String }`.
pub const HANDLER_CONFIG_READ: u32 = 22;
/// Handler id for `config_reset`. Args: `{ "kind": "app"|"workspace"|"mcp"|"ai" }`;
/// Returns: `{}`. Writes defaults.
pub const HANDLER_CONFIG_RESET: u32 = 23;
/// Handler id for `base_index`. Args: `{ "path": String }`. Loads the base
/// from disk (via `nexus_types::bases::load_base`) and inserts it into the
/// `SQLite` index. Returns: `{ "base_id": i64 }`.
pub const HANDLER_BASE_INDEX: u32 = 24;
/// Handler id for `base_list`. Args: `{}`. Returns: `Vec<BaseSummary>`.
pub const HANDLER_BASE_LIST: u32 = 25;
/// Handler id for `base_query`. Args:
/// `{ "path": String, "filters": [String], "sorts": [String], "limit": Option<u32>, "offset": Option<u32> }`.
/// Returns: [`crate::bases::query::QueryResult`].
pub const HANDLER_BASE_QUERY: u32 = 26;
/// Handler id for `list_dir`. Args: `{ "relpath": String }`; Returns: `Vec<TreeEntry>`.
pub const HANDLER_LIST_DIR: u32 = 27;
/// Handler id for `create_file`. Args: `{ "relpath": String }`; Returns: `{}`.
pub const HANDLER_CREATE_FILE: u32 = 28;
/// Handler id for `create_dir`. Args: `{ "relpath": String }`; Returns: `{}`.
pub const HANDLER_CREATE_DIR: u32 = 29;
/// Handler id for `rename_entry`. Args: `{ "from": String, "to": String }`; Returns: `{}`.
pub const HANDLER_RENAME_ENTRY: u32 = 30;
/// Handler id for `delete_entry`. Args: `{ "relpath": String }`; Returns: `{}`.
/// Unlike [`HANDLER_DELETE_FILE`], this handles both files and directories.
pub const HANDLER_DELETE_ENTRY: u32 = 31;
/// Handler id for `base_load`. Args: `{ "path": String }` — forge-relative
/// path to a `.bases` directory. Returns the full
/// [`nexus_types::bases::Base`] (schema + records + views + relations +
/// metadata) parsed straight from disk. Unlike [`HANDLER_BASE_INDEX`]
/// this is read-only and doesn't touch the `SQLite` index — a UI that
/// just wants to render a base in a view panel can skip the index
/// roundtrip.
pub const HANDLER_BASE_LOAD: u32 = 32;
/// Handler id for `write_vault_file`. Args: `{ "path": String, "bytes": Vec<u8> }`;
/// Returns: `{}`. Writes bytes to disk atomically like
/// [`HANDLER_WRITE_FILE`] but **skips** the FTS index, knowledge graph, and
/// all post-write listeners. Intended for shell-owned `.forge/` metadata
/// (e.g. `workspace.json`) that must not pollute search results.
pub const HANDLER_WRITE_VAULT_FILE: u32 = 33;
/// Handler id for `list_all_links`. Args: `{}`. Returns:
/// [`crate::graph::GraphSnapshot`] — every node and every edge in one
/// payload, used by the global graph view.
pub const HANDLER_LIST_ALL_LINKS: u32 = 34;
/// Handler id for `canvas_read`. Args: `{ "path": String }` — forge-relative
/// `.canvas` path. Returns the parsed [`crate::CanvasFile`].
pub const HANDLER_CANVAS_READ: u32 = 35;
/// Handler id for `canvas_write`. Args:
/// `{ "path": String, "canvas": CanvasFile }`. Serializes `canvas` and
/// writes it through [`crate::StorageEngine::write_file`] so the canvas
/// `SQLite` index + knowledge graph stay in sync. Returns
/// [`crate::FileMetadata`].
pub const HANDLER_CANVAS_WRITE: u32 = 36;
/// Handler id for `canvas_patch`. Args:
/// `{ "path": String, "ops": Vec<CanvasPatchOp> }`. Reads the file, applies
/// the op list in order, and rewrites. Returns [`crate::FileMetadata`].
/// The shell debounces patch flushes so this is called once per idle
/// burst, not per frame.
pub const HANDLER_CANVAS_PATCH: u32 = 37;
/// Handler id for `canvas_nodes`. Args: `{ "path": String }`. Returns all
/// indexed nodes for that canvas — `Vec<CanvasNodeRecord>`. Empty vector
/// when the path is not yet indexed.
pub const HANDLER_CANVAS_NODES: u32 = 38;
/// Handler id for `canvas_edges`. Args: `{ "path": String }`. Returns all
/// indexed edges for that canvas — `Vec<CanvasEdgeRecord>`. Empty vector
/// when the path is not yet indexed.
pub const HANDLER_CANVAS_EDGES: u32 = 39;
/// Handler id for `base_record_create`. Args:
/// `{ "path": String, "record": BaseRecord }`. Appends `record` to the
/// base at `path`, saves the `.bases` directory to disk, and reindexes.
/// Generates a v4 UUID when `record.id` is empty. Returns the stored
/// record (with the generated id if applicable).
pub const HANDLER_BASE_RECORD_CREATE: u32 = 40;
/// Handler id for `base_record_update`. Args:
/// `{ "path": String, "record_id": String, "fields": Map<String, Value> }`.
/// Merges `fields` into the record (shallow overwrite), saves, and
/// reindexes. Returns the updated record.
pub const HANDLER_BASE_RECORD_UPDATE: u32 = 41;
/// Handler id for `base_record_delete`. Args:
/// `{ "path": String, "record_id": String }`. Removes the record from
/// disk + index. Missing ids are a no-op (idempotent). Returns `{}`.
pub const HANDLER_BASE_RECORD_DELETE: u32 = 42;
/// Handler id for `base_property_create`. Args:
/// `{ "path": String, "name": String, "definition": Value }`. Adds
/// `name → definition` to `schema.fields`; rejects duplicates. Returns
/// `{}`.
pub const HANDLER_BASE_PROPERTY_CREATE: u32 = 43;
/// Handler id for `base_property_update`. Args:
/// `{ "path": String, "name": String, "definition": Value }`. Replaces
/// the definition of an existing property (no rename, no value
/// migration — see the engine doc on [`crate::StorageEngine::base_property_update`]).
/// Returns `{}`.
pub const HANDLER_BASE_PROPERTY_UPDATE: u32 = 44;
/// Handler id for `base_property_delete`. Args:
/// `{ "path": String, "name": String }`. Removes the property from the
/// schema and drops the key from every record. Missing names are a
/// no-op. Returns `{}`.
pub const HANDLER_BASE_PROPERTY_DELETE: u32 = 45;
/// Handler id for `base_view_create`. Args:
/// `{ "path": String, "view": BaseView }`. Appends `view` to the views
/// list keyed by `view.name`; rejects duplicate names. Returns `{}`.
pub const HANDLER_BASE_VIEW_CREATE: u32 = 46;
/// Handler id for `base_view_update`. Args:
/// `{ "path": String, "view": BaseView }`. Replaces the existing view
/// with the same `view.name`. To rename, call delete + create.
/// Returns `{}`.
pub const HANDLER_BASE_VIEW_UPDATE: u32 = 47;
/// Handler id for `base_view_delete`. Args:
/// `{ "path": String, "name": String }`. Removes the named view.
/// Missing names are a no-op. Returns `{}`.
pub const HANDLER_BASE_VIEW_DELETE: u32 = 48;
/// Handler id for `base_create`. Args:
/// `{ "path": String, "schema": BaseSchema, "seed_records"?: Vec<BaseRecord> }`.
/// Creates a new `.bases` directory at `path` with `schema` (and optional
/// seed records), then indexes it. Rejects if `path` already exists.
/// Returns the freshly-created [`nexus_types::bases::Base`].
pub const HANDLER_BASE_CREATE: u32 = 49;
/// Handler id for `base_property_rename`. Args:
/// `{ "path": String, "old_name": String, "new_name": String }`.
/// Renames a schema column and updates every record's field map in
/// place. Rejects when `old_name` is missing or `new_name` already
/// exists. Returns `{}`.
pub const HANDLER_BASE_PROPERTY_RENAME: u32 = 50;
/// Handler id for `base_record_soft_delete`. Args:
/// `{ "path": String, "record_id": String }`. Sets `deleted_at` on
/// the record but keeps it in `records.json`. Missing ids are a
/// no-op. Returns `{}`.
pub const HANDLER_BASE_RECORD_SOFT_DELETE: u32 = 51;
/// Handler id for `base_record_restore`. Args:
/// `{ "path": String, "record_id": String }`. Clears `deleted_at` on
/// a soft-deleted record. Missing ids or records with no
/// `deleted_at` are a no-op. Returns `{}`.
pub const HANDLER_BASE_RECORD_RESTORE: u32 = 52;
/// Handler id for `obsidian_base_query`. Args:
/// `{ "path": String }`. Reads the Obsidian single-file `.base` at
/// `path`, walks the index, evaluates the filter expression against
/// every markdown note, and projects the configured properties as
/// rows. Read-only — see ADR 0019.
/// Returns [`crate::obsidian_base::ObsidianBaseQueryResult`] as JSON.
pub const HANDLER_OBSIDIAN_BASE_QUERY: u32 = 53;
/// Handler id for `note_append`. Args:
/// `{ "path": String, "snippet": String }`. Reads the existing file
/// at `path` (treating a missing file as empty), then writes back the
/// concatenation `{existing}\n\n{snippet}` (with a trailing newline)
/// through the same atomic + indexing pipeline as
/// [`HANDLER_WRITE_FILE`]. Forge-relative paths only — absolute paths
/// or `..` traversal are rejected at the engine boundary, identical
/// to `write_file`. Returns [`crate::FileMetadata`].
///
/// Use case: BL-043 quick-capture hotkey appends timestamped snippets
/// to a configurable `Inbox.md` without the shell having to read +
/// concatenate + write (which would race against the file watcher).
pub const HANDLER_NOTE_APPEND: u32 = 54;
/// Handler id for `backlinks_to_block`. Args: `{ "path": String, "block_id": String }`.
/// Returns `Vec<BacklinkResult>` filtered to inbound links whose fragment is
/// the BL-049 block-anchored form `^<block_id>` (case-insensitive on the UUID).
/// Powers the backlinks pane's per-block filter — see BL-049 phase 4.
pub const HANDLER_BACKLINKS_TO_BLOCK: u32 = 55;
/// Handler id for `import_forge` (BL-083). Args:
/// `{ "source": "<absolute-path>", "dry_run": bool,
///    "on_conflict": "skip"|"overwrite"|"rename" }`. Returns the
/// [`crate::import::ImportPlan`] when `dry_run = true`, or an
/// [`crate::import::ImportReport`] after applying. The destination
/// is the engine's own forge root (no `--into` at the IPC layer —
/// callers spin up a destination engine and call this on it). Source
/// is an absolute host path operating outside the sandbox; the
/// caller is the trust boundary.
pub const HANDLER_IMPORT_FORGE: u32 = 56;

/// BL-078 — `find_in_files` handler. Args: [`crate::FindInFilesArgs`].
/// Returns `Vec<crate::FileMatches>` ordered by forge-relative path
/// ascending. Walks every non-ignored UTF-8 file under the forge
/// root and applies the matcher line-by-line; binary / non-UTF-8
/// files are silently skipped. See [`crate::find_in_files`] for
/// scope and trade-offs.
pub const HANDLER_FIND_IN_FILES: u32 = 57;

/// BL-078 — `replace_in_files` handler. Args: [`crate::ReplaceInFilesArgs`].
/// Returns a [`crate::ReplaceReport`] tallying the files changed,
/// total replacements applied, and per-file errors that didn't
/// abort the batch. See [`crate::replace_in_files`].
pub const HANDLER_REPLACE_IN_FILES: u32 = 58;

/// BL-053 Phase 4 — `read_frontmatter`. Args:
/// `{ "path": String }`; Returns
/// [`crate::ipc::ReadFrontmatterResult`] — `{ status, fields }`
/// where `fields` is a flat string-valued map of the file's parsed
/// frontmatter (lists collapsed to comma-separated joins; nested
/// objects rendered via debug). Returns `{ status: null, fields: {} }`
/// for paths that don't exist or aren't markdown so the shell can
/// distinguish "no status" from a real error without a separate
/// existence check.
///
/// Read-only — does not touch the search index or emit events. The
/// status pill / file-tree-dot consumer reads through this; the
/// engine's full parser is not exposed because most consumers only
/// need a few well-known scalar keys.
pub const HANDLER_READ_FRONTMATTER: u32 = 59;

/// BL-007 — `write_default_gitignore`. No args. Returns
/// `{ "wrote": bool }` — `true` if a fresh `.forge/.gitignore` was
/// written, `false` if the file already existed (idempotent re-runs
/// are a no-op). Forge-root operation; doesn't need the engine.
///
/// `nexus crdt enable-transport` calls this to bootstrap the
/// gitignore policy on forges created before BL-007 shipped, so the
/// CRDT state files at `.forge/.editor/crdt/*.json` ride through to
/// peers via git while rebuildable / per-machine state stays
/// excluded.
pub const HANDLER_WRITE_DEFAULT_GITIGNORE: u32 = 60;
/// Handler id for `settings_read`. Args: `{}`. Returns the
/// `[settings]` table from `.forge/app.toml` as a JSON object
/// keyed by `pluginId.field`. Missing file / missing block both
/// return `{}` — the shell treats those as "use defaults."
pub const HANDLER_SETTINGS_READ: u32 = 61;
/// Handler id for `settings_write`. Args: `{ "key": String, "value": Value }`.
/// Atomic read-modify-write of one entry in the `[settings]` table.
/// Other sections of `app.toml` are preserved. Passing `null` as
/// the value removes the key, restoring the schema-declared default.
/// Returns `{}`.
pub const HANDLER_SETTINGS_WRITE: u32 = 62;

/// BL-114 — `query_symbol`. Args: [`crate::code_index::SymbolFilter`]
/// (`{ name?: string, path?: string, limit?: u32 }`). Returns
/// `{ symbols: Vec<SymbolRecord> }`. Read-only — never touches the
/// write connection. Powers BL-115's `nexus_context` / `nexus_impact`
/// MCP tools and the BL-116 doc generator.
pub const HANDLER_QUERY_SYMBOL: u32 = 63;

/// BL-128 thin slice — `entity_search`. Args: [`crate::ipc::EntitySearchArgs`].
/// Returns [`crate::ipc::EntitySearchResult`]. Reads `<forge>/entities/*.md`
/// at call time (no SQLite involvement). Empty / missing `entities/`
/// directory returns an empty result.
pub const HANDLER_ENTITY_SEARCH: u32 = 64;
/// BL-128 thin slice — `entity_get`. Args: [`crate::ipc::EntityGetArgs`].
/// Returns [`crate::ipc::EntityGetResult`] with `entity: null` when
/// neither the canonical id nor any alias resolves.
pub const HANDLER_ENTITY_GET: u32 = 65;
/// BL-128 thin slice — `entity_relations`. Args:
/// [`crate::ipc::EntityRelationsArgs`]. Returns
/// [`crate::ipc::EntityRelationsResult`]. Direction defaults to `"both"`.
pub const HANDLER_ENTITY_RELATIONS: u32 = 66;
/// BL-128 close — `entity_upsert`. Args: [`crate::ipc::EntityUpsertArgs`].
/// Returns [`crate::ipc::EntityUpsertResult`]. Atomically writes the
/// entity markdown file under `<forge>/entities/<id>.md` (temp-fsync-
/// rename via `crate::atomic_write`). Relation kinds are normalised
/// through `crate::entity_index::normalize_relation_type` before
/// persistence so on-disk vocabulary stays canonical.
pub const HANDLER_ENTITY_UPSERT: u32 = 67;
/// BL-128 close — `entity_find_duplicates`. Args:
/// [`crate::ipc::EntityFindDuplicatesArgs`]. Returns
/// [`crate::ipc::EntityFindDuplicatesResult`]. Jaccard token
/// similarity over `id + aliases + description`; only same-type
/// pairs are reported. Threshold defaults to `0.92`.
pub const HANDLER_ENTITY_FIND_DUPLICATES: u32 = 68;
/// BL-129 close — `entity_merge`. Args: [`crate::ipc::EntityMergeArgs`].
/// Returns [`crate::ipc::EntityMergeResult`]. Merges `drop` into `keep`
/// (union aliases + relations, longer description, `drop`'s id added as
/// an alias on `keep` so dangling outgoing references still resolve)
/// and deletes `drop`'s file. The atomic-write path is used for the
/// `keep` rewrite; the delete runs after the rewrite succeeds.
pub const HANDLER_ENTITY_MERGE: u32 = 70;
/// BL-129 follow-up — `list_draft_relations`. Args:
/// [`crate::ipc::ListDraftRelationsArgs`]. Returns
/// [`crate::ipc::ListDraftRelationsResult`]. Read-only enumeration of
/// every outgoing relation at-or-below the confidence threshold
/// (default `0.5` — Dream-Cycle proposal value). Drives the shell
/// inbox; approve/skip flow through `entity_get` + `entity_upsert`.
pub const HANDLER_LIST_DRAFT_RELATIONS: u32 = 71;
/// P4-07 — `write_frontmatter`. Args:
/// `{ "path": String, "key": String, "value": String | null }`.
/// Setting `value` to a non-null string inserts/replaces a top-level
/// scalar frontmatter field; setting it to `null` removes the field
/// (no-op when absent). Files without an existing `---` block gain a
/// freshly-prepended one. The handler delegates to `write_file` so
/// the FTS index, knowledge graph, and watcher all stay in sync.
/// Returns `{ "ok": true }`.
pub const HANDLER_WRITE_FRONTMATTER: u32 = 72;

/// BL-129 thin slice — `entity_decay_relations`. Args:
/// [`crate::ipc::EntityDecayRelationsArgs`]. Returns
/// [`crate::ipc::EntityDecayRelationsResult`]. Walks `entities/*.md`,
/// multiplies each relation's confidence by `factor`, clamps to
/// `floor`, and atomically rewrites any file that changed. Already-
/// at-floor relations are skipped (idempotent across cycles). When
/// `dry_run` is true the counts are computed but no file is touched.
pub const HANDLER_ENTITY_DECAY_RELATIONS: u32 = 69;

/// Core plugin that owns a forge watcher and bridges file-system events onto
/// the kernel event bus.
///
/// # Lifecycle
///
/// | Hook | Action |
/// |------|--------|
/// | `on_init` | Verifies the forge directory exists |
/// | `on_start` | Starts a [`Watcher`], spawns the bridge thread |
/// | `on_stop` | Signals the bridge thread and joins it |
///
/// Construct with [`StorageCorePlugin::new`], then either register via
/// [`nexus_plugins::PluginManager::register_core`] or drive the lifecycle
/// hooks directly from the CLI's `App`.
pub struct StorageCorePlugin {
    forge_root: PathBuf,
    config: StorageConfig,
    event_bus: Arc<EventBus>,
    /// Opened by `on_init`; used by the IPC dispatch handlers.
    ///
    /// Wrapped in [`Arc`] for cheap clone into background threads
    /// (the bridge loop, parallel index workers, …). `StorageEngine`
    /// is `Send + Sync` post-#80 — its methods all take `&self` and
    /// it no longer owns a non-`Sync` `mpsc::Receiver` — so concurrent
    /// IPC dispatch needs no per-call locking.
    engine: Option<Arc<StorageEngine>>,
    stop_tx: Option<mpsc::SyncSender<()>>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
    /// BL-114: thread that watches `com.nexus.git.commit` and runs
    /// an incremental reconcile when an external commit lands. The
    /// thread signals stop through `commit_stop_tx`.
    commit_stop_tx: Option<mpsc::SyncSender<()>>,
    commit_thread: Option<std::thread::JoinHandle<()>>,
}

impl StorageCorePlugin {
    /// Create a new (unstarted) plugin for the forge at `forge_root`.
    ///
    /// `debounce_ms` controls how long the watcher waits before flushing a
    /// burst of filesystem notifications.  [`StorageConfig::debounce_ms`] is a
    /// good default to pass here.
    #[must_use]
    pub fn new(forge_root: PathBuf, config: &StorageConfig, event_bus: Arc<EventBus>) -> Self {
        Self {
            forge_root,
            config: config.clone(),
            event_bus,
            engine: None,
            stop_tx: None,
            bridge_thread: None,
            commit_stop_tx: None,
            commit_thread: None,
        }
    }

    /// Direct access to the underlying engine for the bootstrap/CLI during
    /// migration. Returns `None` before `on_init` has run successfully.
    ///
    /// `StorageEngine` is `Send + Sync`; callers can clone the
    /// returned `Arc` cheaply and dispatch concurrently without
    /// locking. See issue #80.
    #[must_use]
    pub fn engine(&self) -> Option<&Arc<StorageEngine>> {
        self.engine.as_ref()
    }
}

impl CorePlugin for StorageCorePlugin {
    /// Verify that the forge exists and open the storage engine.
    fn on_init(&mut self) -> Result<(), PluginError> {
        let forge_dir = self.forge_root.join(".forge");
        if !forge_dir.exists() {
            return Err(PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_init".to_string(),
                reason: format!(
                    "forge directory not found at '{}'; run `nexus forge init` first",
                    forge_dir.display()
                ),
            });
        }

        // Open the storage engine. IPC handlers read from this handle.
        let engine = StorageEngine::open(&self.forge_root, &self.config).map_err(|e| {
            PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_init".to_string(),
                reason: format!("failed to open storage engine: {e}"),
            }
        })?;
        self.engine = Some(Arc::new(engine));
        Ok(())
    }

    /// Start the forge watcher and the bridge thread that translates
    /// [`StorageEvent`]s into [`NexusEvent`]s on the kernel bus.
    fn on_start(&mut self) -> Result<(), PluginError> {
        let watcher = Watcher::start(&self.forge_root, self.config.debounce_ms)
            .map_err(|e| PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_start".to_string(),
                reason: format!("watcher failed to start: {e}"),
            })?;

        let bus = Arc::clone(&self.event_bus);
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
        self.stop_tx = Some(stop_tx);

        let handle = std::thread::Builder::new()
            .name("nexus-storage-bridge".to_string())
            .spawn(move || bridge_loop(watcher, bus, stop_rx))
            .map_err(|e| PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_start".to_string(),
                reason: format!("failed to spawn bridge thread: {e}"),
            })?;

        self.bridge_thread = Some(handle);

        // BL-114: subscribe to git.commit. Subscribe on the *parent*
        // thread before spawning so an event emitted between spawn
        // and subscribe isn't lost — same pattern as the BL-007
        // pull-landing subscriber in `nexus-bootstrap::crdt_publisher`.
        let commit_sub = self
            .event_bus
            .subscribe(EventFilter::CustomExact(GIT_COMMIT_TOPIC.to_string()));
        let engine_for_commit = self
            .engine
            .as_ref()
            .map(Arc::clone);
        let (commit_stop_tx, commit_stop_rx) = mpsc::sync_channel::<()>(1);
        self.commit_stop_tx = Some(commit_stop_tx);
        let commit_handle = std::thread::Builder::new()
            .name("nexus-storage-git-commit".to_string())
            .spawn(move || git_commit_loop(engine_for_commit, commit_sub, commit_stop_rx))
            .map_err(|e| PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_start".to_string(),
                reason: format!("failed to spawn git-commit thread: {e}"),
            })?;
        self.commit_thread = Some(commit_handle);
        Ok(())
    }

    /// Stop the bridge thread gracefully.
    fn on_stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.try_send(());
        }
        if let Some(handle) = self.bridge_thread.take() {
            let _ = handle.join();
        }
        if let Some(tx) = self.commit_stop_tx.take() {
            let _ = tx.try_send(());
        }
        if let Some(handle) = self.commit_thread.take() {
            let _ = handle.join();
        }
    }

    /// Route an IPC call to the corresponding storage operation.
    ///
    /// Handler ids are defined as `HANDLER_*` constants at the top of this
    /// module; the [`nexus_plugins::PluginManifest`] registered by the
    /// bootstrap maps each command id to one of those numbers.
    #[allow(clippy::too_many_lines)]
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // Handlers that operate on on-disk forge files (not the SQLite index)
        // don't need the engine lock — serve them before acquiring it.
        // SD-03 Phase A: the bodies live in `crate::handlers::*`.
        let root = self.forge_root.as_path();
        match handler_id {
            HANDLER_CONFIG_READ => return crate::handlers::config::read(root, args),
            HANDLER_CONFIG_RESET => return crate::handlers::config::reset(root, args),
            HANDLER_SETTINGS_READ => return crate::handlers::config::settings_read(root),
            HANDLER_SETTINGS_WRITE => return crate::handlers::config::settings_write(root, args),
            HANDLER_WRITE_DEFAULT_GITIGNORE => {
                let forge = crate::Forge::new(&self.forge_root);
                let wrote = forge
                    .write_default_gitignore()
                    .map_err(|e| exec_err(format!("write_default_gitignore: {e}")))?;
                return Ok(serde_json::json!({ "wrote": wrote }));
            }
            HANDLER_ENTITY_SEARCH => return crate::handlers::entity::search(root, args),
            HANDLER_ENTITY_GET => return crate::handlers::entity::get(root, args),
            HANDLER_ENTITY_RELATIONS => return crate::handlers::entity::relations(root, args),
            HANDLER_ENTITY_UPSERT => return crate::handlers::entity::upsert(root, args),
            HANDLER_ENTITY_FIND_DUPLICATES => {
                return crate::handlers::entity::find_duplicates(root, args)
            }
            HANDLER_ENTITY_DECAY_RELATIONS => {
                return crate::handlers::entity::decay_relations(root, args)
            }
            HANDLER_ENTITY_MERGE => return crate::handlers::entity::merge(root, args),
            HANDLER_LIST_DRAFT_RELATIONS => {
                return crate::handlers::entity::list_draft_relations(root, args)
            }
            _ => {}
        }

        // Engine is `Arc<StorageEngine>`; no per-call locking. Methods
        // all take `&self`, internal write paths use a fine-grained
        // mutex on the write connection where needed. See issue #80.
        let engine = self.engine.as_ref().ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "storage engine not initialised (on_init did not run)".to_string(),
        })?;

        match handler_id {
            HANDLER_QUERY_FILES => crate::handlers::files::query_files(engine, args),
            HANDLER_READ_FILE => crate::handlers::files::read_file(engine, args),
            HANDLER_BACKLINKS => crate::handlers::graph::backlinks(engine, args),
            HANDLER_BACKLINKS_TO_BLOCK => crate::handlers::graph::backlinks_to_block(engine, args),
            HANDLER_QUERY_TASKS => crate::handlers::tasks::query_tasks(engine, args),
            HANDLER_GRAPH_STATS => crate::handlers::graph::graph_stats(engine),
            HANDLER_REBUILD_INDEX => crate::handlers::index::rebuild_index(engine),
            HANDLER_SEARCH => crate::handlers::search::search(engine, args),
            HANDLER_QUERY_SYMBOL => crate::handlers::search::query_symbol(engine, args),
            HANDLER_WRITE_FILE => crate::handlers::files::write_file(engine, args),
            HANDLER_NOTE_APPEND => crate::handlers::notes::note_append(engine, args),
            HANDLER_WRITE_VAULT_FILE => crate::handlers::files::write_vault_file(engine, args),
            HANDLER_DELETE_FILE => crate::handlers::files::delete_file(engine, args),
            HANDLER_FILE_EXISTS => crate::handlers::files::file_exists(engine, args),
            HANDLER_REBUILD_SEARCH_INDEX => crate::handlers::index::rebuild_search_index(engine),
            HANDLER_TOGGLE_TASK => crate::handlers::tasks::toggle_task(engine, args),
            HANDLER_OUTGOING_LINKS => crate::handlers::graph::outgoing_links(engine, args),
            HANDLER_UNRESOLVED_LINKS => crate::handlers::graph::unresolved_links(engine),
            HANDLER_LIST_ALL_LINKS => crate::handlers::graph::list_all_links(engine),
            HANDLER_CANVAS_READ => crate::handlers::canvas::read(engine, args),
            HANDLER_CANVAS_WRITE => crate::handlers::canvas::write(engine, args),
            HANDLER_CANVAS_PATCH => crate::handlers::canvas::patch(engine, args),
            HANDLER_CANVAS_NODES => crate::handlers::canvas::nodes(engine, args),
            HANDLER_CANVAS_EDGES => crate::handlers::canvas::edges(engine, args),
            HANDLER_BASE_RECORD_CREATE => crate::handlers::bases::record_create(engine, args),
            HANDLER_BASE_RECORD_UPDATE => crate::handlers::bases::record_update(engine, args),
            HANDLER_BASE_RECORD_DELETE => crate::handlers::bases::record_delete(engine, args),
            HANDLER_BASE_PROPERTY_CREATE => crate::handlers::bases::property_create(engine, args),
            HANDLER_BASE_PROPERTY_UPDATE => crate::handlers::bases::property_update(engine, args),
            HANDLER_BASE_RECORD_SOFT_DELETE => {
                crate::handlers::bases::record_soft_delete(engine, args)
            }
            HANDLER_BASE_RECORD_RESTORE => crate::handlers::bases::record_restore(engine, args),
            HANDLER_BASE_PROPERTY_RENAME => crate::handlers::bases::property_rename(engine, args),
            HANDLER_BASE_CREATE => crate::handlers::bases::create(engine, args),
            HANDLER_BASE_PROPERTY_DELETE => crate::handlers::bases::property_delete(engine, args),
            HANDLER_BASE_VIEW_CREATE => crate::handlers::bases::view_create(engine, args),
            HANDLER_BASE_VIEW_UPDATE => crate::handlers::bases::view_update(engine, args),
            HANDLER_BASE_VIEW_DELETE => crate::handlers::bases::view_delete(engine, args),
            HANDLER_GRAPH_NEIGHBORS => crate::handlers::graph::graph_neighbors(engine, args),
            HANDLER_QUERY_TAGS => crate::handlers::search::query_tags(engine, args),
            HANDLER_VECTOR_INSERT => crate::handlers::vector::insert(engine, args),
            HANDLER_VECTOR_QUERY => crate::handlers::vector::query(engine, args),
            HANDLER_VECTOR_DELETE_BY_FILE => crate::handlers::vector::delete_by_file(engine, args),
            HANDLER_VECTORSTORE_COUNT => crate::handlers::vector::count(engine),
            HANDLER_QUERY_BLOCKS => crate::handlers::tasks::query_blocks(engine, args),
            HANDLER_BASE_INDEX => crate::handlers::bases::index(engine, &self.forge_root, args),
            HANDLER_BASE_LOAD => crate::handlers::bases::load(&self.forge_root, args),
            HANDLER_BASE_LIST => crate::handlers::bases::list(engine),
            HANDLER_LIST_DIR => crate::handlers::tree::list_dir(engine, args),
            HANDLER_CREATE_FILE => crate::handlers::tree::create_file(engine, args),
            HANDLER_CREATE_DIR => crate::handlers::tree::create_dir(engine, args),
            HANDLER_RENAME_ENTRY => crate::handlers::tree::rename_entry(engine, args),
            HANDLER_DELETE_ENTRY => crate::handlers::tree::delete_entry(engine, args),
            HANDLER_BASE_QUERY => crate::handlers::bases::query(engine, args),
            HANDLER_OBSIDIAN_BASE_QUERY => {
                crate::handlers::index::obsidian_base_query(engine, args)
            }
            HANDLER_IMPORT_FORGE => crate::handlers::index::import_forge(engine, args),
            HANDLER_FIND_IN_FILES => {
                crate::handlers::search::find_in_files(&self.forge_root, args)
            }
            HANDLER_REPLACE_IN_FILES => {
                crate::handlers::search::replace_in_files(engine, &self.forge_root, args)
            }
            HANDLER_READ_FRONTMATTER => {
                crate::handlers::notes::read_frontmatter(&self.forge_root, args)
            }
            HANDLER_WRITE_FRONTMATTER => {
                crate::handlers::notes::write_frontmatter(engine, &self.forge_root, args)
            }
            _ => Err(exec_err(format!("unknown handler id {handler_id}"))),
        }
    }
}

// ── Dispatch helpers — SD-01: emitted by the shared macro ───────────────────

nexus_plugins::define_dispatch_helpers!();

// SD-03 Phase B complete: all helpers (is_forge_metadata_path,
// path_arg, relpath_arg, name_arg, build_appended,
// read_frontmatter_for_path) and every inline-arm body now live under
// `crate::handlers::*`. Only `apply_frontmatter_edit` remains below
// because its test cluster lives in this file's `#[cfg(test)]` block.

/// P4-07 — splice a top-level scalar frontmatter field into a markdown
/// source. `value = Some(s)` inserts or replaces the field's line in
/// the YAML frontmatter; `value = None` removes the field if present.
/// Files without a leading `---` block gain a freshly-prepended one
/// when `value` is `Some` (and are returned unchanged on `None`).
///
/// Only top-level scalar keys are supported — nested objects, lists,
/// and multi-line block scalars are left intact, but if the caller
/// names a list key the write replaces it with a scalar string.
/// Power users wanting structured edits should reach for a dedicated
/// YAML editor; the user-facing add-property prompt is the only
/// caller today and accepts a single string value.
#[must_use]
pub fn apply_frontmatter_edit(content: &str, key: &str, value: Option<&str>) -> String {
    // Locate the closing `---` of the existing frontmatter, if any.
    let (open_len, fm_body_start, fm_close_start) = match locate_frontmatter(content) {
        Some(triple) => triple,
        None => {
            // No frontmatter block.
            let Some(v) = value else {
                // Remove on a file that has no frontmatter — no-op.
                return content.to_string();
            };
            let opener = if content.starts_with("---\r\n") || content.contains("\r\n") {
                "---\r\n"
            } else {
                "---\n"
            };
            let closer = if opener.ends_with("\r\n") {
                "---\r\n\r\n"
            } else {
                "---\n\n"
            };
            let line_end = if opener.ends_with("\r\n") { "\r\n" } else { "\n" };
            return format!("{opener}{key}: {v}{line_end}{closer}{content}");
        }
    };
    let yaml_src = &content[open_len..fm_close_start];
    let line_end = if content[..open_len].ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = format!("{key}:");
    let mut found = false;
    let mut rebuilt = String::with_capacity(yaml_src.len() + 32);
    for line in yaml_src.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        if !found && trimmed.starts_with(&prefix) {
            found = true;
            if let Some(v) = value {
                rebuilt.push_str(&format!("{key}: {v}{line_end}"));
            }
            // value == None ⇒ drop this line entirely.
            continue;
        }
        rebuilt.push_str(line);
    }
    if !found {
        if let Some(v) = value {
            // Append the new key just before the closing `---`. Ensure
            // the body ends with a newline so we don't glue keys.
            if !rebuilt.ends_with('\n') {
                rebuilt.push_str(line_end);
            }
            rebuilt.push_str(&format!("{key}: {v}{line_end}"));
        }
        // value == None ⇒ key absent, nothing to do.
    }
    let mut out = String::with_capacity(content.len() + 32);
    out.push_str(&content[..open_len]);
    out.push_str(&rebuilt);
    out.push_str(&content[fm_close_start..fm_body_start]);
    out.push_str(&content[fm_body_start..]);
    out
}

/// Locate the byte ranges of an existing frontmatter block. Returns
/// `(open_len, body_start, close_start)` where `open_len` is the
/// length of the opening `---` line (so `content[..open_len]` is the
/// opener), `close_start` is the index of the closing `---` line, and
/// `body_start` is the index just after the closing `---\n` (or
/// `---\r\n`). Returns `None` for files without a well-formed block.
fn locate_frontmatter(content: &str) -> Option<(usize, usize, usize)> {
    let open_len = if content.starts_with("---\r\n") {
        5
    } else if content.starts_with("---\n") {
        4
    } else {
        return None;
    };
    // Find `\n---` followed by `\n` or `\r\n` or EOF.
    let rest = &content[open_len..];
    let close_offset = rest.find("\n---")?;
    let close_start = open_len + close_offset + 1; // index of the `---` itself
    let after = close_start + 3; // index past `---`
    let body_start = if content[after..].starts_with("\r\n") {
        after + 2
    } else if content[after..].starts_with('\n') {
        after + 1
    } else {
        after
    };
    Some((open_len, body_start, close_start))
}

// ── Config handlers ──────────────────────────────────────────────────────────

// SD-03 Phase A: config_kind, dispatch_entity_*, dispatch_config_*,
// dispatch_settings_*, dispatch_list_draft_relations moved to
// `crate::handlers::{config, entity}`.

// ── Bridge thread ──────────────────────────────────────────────────────────────

/// Polls the watcher until the stop signal arrives, translating each
/// [`StorageEvent`] into a [`NexusEvent`] published on the kernel bus.
///
/// The bridge only handles event translation and publication.  Index updates
/// (`write_file`, `delete_file`, etc.) remain the caller's responsibility.
#[allow(clippy::needless_pass_by_value)]
/// BL-114: drain `com.nexus.git.commit` events and run an incremental
/// reconcile so the FTS / knowledge graph / code-symbol indices catch
/// up after an external commit (pull, rebase, checkout). Many commits
/// inside a single tick collapse to one reconcile — the index is
/// idempotent under repeated passes. Falls out when `stop_rx` fires.
fn git_commit_loop(
    engine: Option<Arc<StorageEngine>>,
    mut sub: nexus_kernel::EventSubscription,
    stop_rx: mpsc::Receiver<()>,
) {
    loop {
        match stop_rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        // Coalesce every pending commit into one reconcile per tick.
        let mut had_event = false;
        loop {
            match sub.try_recv() {
                Ok(Some(_)) => had_event = true,
                Ok(None) => break,
                Err(err) => {
                    tracing::debug!(%err, "BL-114 git-commit subscriber recv error");
                    break;
                }
            }
        }

        if had_event {
            if let Some(engine) = engine.as_ref() {
                match engine.reconcile_index() {
                    Ok(delta) => tracing::debug!(
                        ?delta,
                        "BL-114: reconcile after git.commit",
                    ),
                    Err(err) => tracing::warn!(
                        %err,
                        "BL-114: reconcile after git.commit failed",
                    ),
                }
            }
        } else {
            std::thread::sleep(GIT_COMMIT_POLL_INTERVAL);
        }
    }
}

fn bridge_loop(
    watcher: Watcher,
    bus: Arc<EventBus>,
    stop_rx: mpsc::Receiver<()>,
) {
    let rx = watcher.events();

    loop {
        match stop_rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let storage_event = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(e) => e,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        publish_event(&storage_event, &bus);
    }
}

/// Translate one [`StorageEvent`] into a `com.nexus.storage.*` custom event
/// and publish on the bus. BL-052 — also fans out to the universal
/// `com.nexus.activity.appended` topic so the timeline pane sees file
/// writes alongside AI / git / terminal activity.
fn publish_event(event: &StorageEvent, bus: &EventBus) {
    match event {
        StorageEvent::FileCreated { path, content_hash } => {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.file_created",
                serde_json::json!({
                    "path": path,
                    "content_hash": content_hash,
                }),
            );
            publish_file_activity(bus, "created", path, None);
        }

        StorageEvent::FileModified { path, content_hash } => {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.file_modified",
                serde_json::json!({
                    "path": path,
                    "content_hash": content_hash,
                }),
            );
            publish_file_activity(bus, "modified", path, None);
        }

        StorageEvent::ReconcileRequested => {
            // Watcher recommends a re-walk of the forge — typically
            // emitted after a git batch (`.git/index.lock` came + went).
            // Bracket the indexing window with started/completed events
            // so subscribers can debounce UI refreshes. The actual
            // reconcile is the consumer's responsibility (#84).
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.indexing.started",
                serde_json::json!({}),
            );
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.indexing.completed",
                serde_json::json!({ "triggered_by": "git-batch-mode" }),
            );
        }

        StorageEvent::FileDeleted { path } => {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.file_deleted",
                serde_json::json!({ "path": path }),
            );
            publish_file_activity(bus, "deleted", path, None);
        }

        StorageEvent::FileRenamed { from, to, content_hash } => {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.file_renamed",
                serde_json::json!({
                    "from": from,
                    "to": to,
                    "content_hash": content_hash,
                }),
            );
            publish_file_activity(bus, "renamed", to, Some(from));
        }
    }
}

/// BL-052 — fan a storage file event out to the universal activity
/// topic. `kind` is one of `created` / `modified` / `deleted` /
/// `renamed`; `path` is the affected file (for renames, the new
/// destination). `extra` carries the rename source when applicable.
/// Best-effort: a bus failure logs at debug level and is swallowed —
/// missing one activity entry is preferable to interrupting the
/// storage event pipeline.
fn publish_file_activity(
    bus: &EventBus,
    kind: &str,
    path: &str,
    extra_path: Option<&str>,
) {
    use nexus_types::activity::{
        ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface,
        ACTIVITY_APPENDED_TOPIC,
    };

    let mut entry = ActivityEntry::now(
        // session_id is the path so the timeline can collapse
        // many edits to the same file under one row if it wants to.
        path.to_string(),
        ActivitySurface::File,
        ActivityOrigin::Storage,
    );
    entry.outcome = ActivityOutcome::Ok;
    entry.prompt = match (kind, extra_path) {
        ("renamed", Some(from)) => format!("renamed {from} → {path}"),
        _ => format!("{kind} {path}"),
    };
    entry.files = match extra_path {
        Some(from) => vec![from.to_string(), path.to_string()],
        None => vec![path.to_string()],
    };
    if let Ok(payload) = serde_json::to_value(&entry) {
        if let Err(err) = bus.publish_plugin(PLUGIN_ID, ACTIVITY_APPENDED_TOPIC, payload) {
            tracing::debug!(
                plugin = PLUGIN_ID,
                %err,
                "failed to publish storage activity entry",
            );
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::notes::build_appended;
    use crate::StorageEngine;

    /// Issue #84. Handler ids are hand-allocated `u32` constants —
    /// the convention is "append; never reuse a retired id." That's
    /// only a comment, so this test catches the case where two
    /// `HANDLER_*` constants are accidentally given the same id.
    /// Add the constant's name to the table when a new handler is
    /// declared (the table is the source of truth the test checks).
    #[test]
    fn handler_ids_are_unique() {
        let mut handlers: Vec<(&str, u32)> = vec![
            ("HANDLER_QUERY_FILES", HANDLER_QUERY_FILES),
            ("HANDLER_READ_FILE", HANDLER_READ_FILE),
            ("HANDLER_BACKLINKS", HANDLER_BACKLINKS),
            ("HANDLER_QUERY_TASKS", HANDLER_QUERY_TASKS),
            ("HANDLER_GRAPH_STATS", HANDLER_GRAPH_STATS),
            ("HANDLER_REBUILD_INDEX", HANDLER_REBUILD_INDEX),
            ("HANDLER_SEARCH", HANDLER_SEARCH),
            ("HANDLER_WRITE_FILE", HANDLER_WRITE_FILE),
            ("HANDLER_DELETE_FILE", HANDLER_DELETE_FILE),
            ("HANDLER_FILE_EXISTS", HANDLER_FILE_EXISTS),
            ("HANDLER_REBUILD_SEARCH_INDEX", HANDLER_REBUILD_SEARCH_INDEX),
            ("HANDLER_TOGGLE_TASK", HANDLER_TOGGLE_TASK),
            ("HANDLER_OUTGOING_LINKS", HANDLER_OUTGOING_LINKS),
            ("HANDLER_UNRESOLVED_LINKS", HANDLER_UNRESOLVED_LINKS),
            ("HANDLER_GRAPH_NEIGHBORS", HANDLER_GRAPH_NEIGHBORS),
            ("HANDLER_QUERY_TAGS", HANDLER_QUERY_TAGS),
            ("HANDLER_VECTOR_INSERT", HANDLER_VECTOR_INSERT),
            ("HANDLER_VECTOR_QUERY", HANDLER_VECTOR_QUERY),
            ("HANDLER_VECTOR_DELETE_BY_FILE", HANDLER_VECTOR_DELETE_BY_FILE),
            ("HANDLER_VECTORSTORE_COUNT", HANDLER_VECTORSTORE_COUNT),
            ("HANDLER_QUERY_BLOCKS", HANDLER_QUERY_BLOCKS),
            ("HANDLER_CONFIG_READ", HANDLER_CONFIG_READ),
            ("HANDLER_CONFIG_RESET", HANDLER_CONFIG_RESET),
            ("HANDLER_BASE_INDEX", HANDLER_BASE_INDEX),
            ("HANDLER_BASE_LIST", HANDLER_BASE_LIST),
            ("HANDLER_BASE_QUERY", HANDLER_BASE_QUERY),
            ("HANDLER_LIST_DIR", HANDLER_LIST_DIR),
            ("HANDLER_CREATE_FILE", HANDLER_CREATE_FILE),
            ("HANDLER_CREATE_DIR", HANDLER_CREATE_DIR),
            ("HANDLER_RENAME_ENTRY", HANDLER_RENAME_ENTRY),
            ("HANDLER_DELETE_ENTRY", HANDLER_DELETE_ENTRY),
            ("HANDLER_BASE_LOAD", HANDLER_BASE_LOAD),
            ("HANDLER_WRITE_VAULT_FILE", HANDLER_WRITE_VAULT_FILE),
            ("HANDLER_LIST_ALL_LINKS", HANDLER_LIST_ALL_LINKS),
            ("HANDLER_CANVAS_READ", HANDLER_CANVAS_READ),
            ("HANDLER_CANVAS_WRITE", HANDLER_CANVAS_WRITE),
            ("HANDLER_CANVAS_PATCH", HANDLER_CANVAS_PATCH),
            ("HANDLER_CANVAS_NODES", HANDLER_CANVAS_NODES),
            ("HANDLER_CANVAS_EDGES", HANDLER_CANVAS_EDGES),
            ("HANDLER_BASE_RECORD_CREATE", HANDLER_BASE_RECORD_CREATE),
            ("HANDLER_BASE_RECORD_UPDATE", HANDLER_BASE_RECORD_UPDATE),
            ("HANDLER_BASE_RECORD_DELETE", HANDLER_BASE_RECORD_DELETE),
            ("HANDLER_BASE_PROPERTY_CREATE", HANDLER_BASE_PROPERTY_CREATE),
            ("HANDLER_BASE_PROPERTY_UPDATE", HANDLER_BASE_PROPERTY_UPDATE),
            ("HANDLER_BASE_PROPERTY_DELETE", HANDLER_BASE_PROPERTY_DELETE),
            ("HANDLER_BASE_VIEW_CREATE", HANDLER_BASE_VIEW_CREATE),
            ("HANDLER_BASE_VIEW_UPDATE", HANDLER_BASE_VIEW_UPDATE),
            ("HANDLER_BASE_VIEW_DELETE", HANDLER_BASE_VIEW_DELETE),
            ("HANDLER_BASE_CREATE", HANDLER_BASE_CREATE),
            ("HANDLER_BASE_PROPERTY_RENAME", HANDLER_BASE_PROPERTY_RENAME),
            ("HANDLER_BASE_RECORD_SOFT_DELETE", HANDLER_BASE_RECORD_SOFT_DELETE),
            ("HANDLER_BASE_RECORD_RESTORE", HANDLER_BASE_RECORD_RESTORE),
            ("HANDLER_OBSIDIAN_BASE_QUERY", HANDLER_OBSIDIAN_BASE_QUERY),
            ("HANDLER_NOTE_APPEND", HANDLER_NOTE_APPEND),
            ("HANDLER_BACKLINKS_TO_BLOCK", HANDLER_BACKLINKS_TO_BLOCK),
            ("HANDLER_WRITE_DEFAULT_GITIGNORE", HANDLER_WRITE_DEFAULT_GITIGNORE),
            ("HANDLER_SETTINGS_READ", HANDLER_SETTINGS_READ),
            ("HANDLER_SETTINGS_WRITE", HANDLER_SETTINGS_WRITE),
            ("HANDLER_QUERY_SYMBOL", HANDLER_QUERY_SYMBOL),
            ("HANDLER_ENTITY_SEARCH", HANDLER_ENTITY_SEARCH),
            ("HANDLER_ENTITY_GET", HANDLER_ENTITY_GET),
            ("HANDLER_ENTITY_RELATIONS", HANDLER_ENTITY_RELATIONS),
            ("HANDLER_ENTITY_UPSERT", HANDLER_ENTITY_UPSERT),
            ("HANDLER_ENTITY_FIND_DUPLICATES", HANDLER_ENTITY_FIND_DUPLICATES),
            ("HANDLER_WRITE_FRONTMATTER", HANDLER_WRITE_FRONTMATTER),
        ];
        handlers.sort_by_key(|(_, id)| *id);
        for window in handlers.windows(2) {
            let (a_name, a_id) = window[0];
            let (b_name, b_id) = window[1];
            assert_ne!(
                a_id, b_id,
                "duplicate handler id {a_id}: {a_name} and {b_name} share the same value. \
                 Append a fresh id rather than reusing a retired one (see core_plugin.rs)"
            );
        }
    }

    fn boot_plugin(forge: &std::path::Path) -> StorageCorePlugin {
        // StorageCorePlugin::on_init opens its own engine handle and therefore
        // its own lockfile; drop the initialising engine before handing over.
        drop(StorageEngine::init(forge).expect("init forge"));
        let bus = Arc::new(EventBus::new(16));
        let mut plugin =
            StorageCorePlugin::new(forge.to_path_buf(), &StorageConfig::default(), bus);
        plugin.on_init().expect("on_init");
        plugin
    }

    #[test]
    fn note_append_creates_missing_file_with_snippet_and_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        let args = serde_json::json!({
            "path": "Inbox.md",
            "snippet": "## Captured\n\nfirst note",
        });
        let resp = plugin
            .dispatch(HANDLER_NOTE_APPEND, &args)
            .expect("note_append on missing file should create it");

        // Returns FileMetadata-shaped JSON.
        assert_eq!(resp.get("path").and_then(|v| v.as_str()), Some("Inbox.md"));
        assert!(resp.get("size_bytes").and_then(serde_json::Value::as_u64).is_some());

        let on_disk = std::fs::read_to_string(dir.path().join("Inbox.md")).expect("read back");
        assert_eq!(on_disk, "## Captured\n\nfirst note\n");
    }

    #[test]
    fn note_append_appends_to_existing_with_blank_line_separator_and_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        // Seed the file via the same handler so the on-disk layout is
        // exactly what the production hotkey would produce.
        plugin
            .dispatch(
                HANDLER_NOTE_APPEND,
                &serde_json::json!({ "path": "Inbox.md", "snippet": "first" }),
            )
            .expect("seed first append");

        plugin
            .dispatch(
                HANDLER_NOTE_APPEND,
                &serde_json::json!({ "path": "Inbox.md", "snippet": "second" }),
            )
            .expect("second append");

        let on_disk = std::fs::read_to_string(dir.path().join("Inbox.md")).expect("read back");
        // Exactly one blank line between snippets, exactly one trailing
        // newline at the end. No double-blank-line drift across appends.
        assert_eq!(on_disk, "first\n\nsecond\n");
    }

    #[test]
    fn note_append_rejects_absolute_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        // Use a path that is unambiguously absolute on every platform
        // we run CI on. On Windows tempfile's tempdir() also produces an
        // absolute path, but we keep the assertion portable by using a
        // shape `is_absolute()` recognises everywhere.
        let abs = if cfg!(windows) {
            "C:\\evil\\path.md".to_string()
        } else {
            "/etc/evil.md".to_string()
        };
        let args = serde_json::json!({ "path": abs, "snippet": "x" });
        let err = plugin
            .dispatch(HANDLER_NOTE_APPEND, &args)
            .expect_err("absolute paths must be rejected");
        // Rejection now flows from the engine's `resolve_within`
        // path-confinement check via `read_file` (issue #72), surfaced
        // by note_append as a `read:` failure containing the offending
        // relpath.
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("invalid relpath") && reason.contains(&abs),
                    "expected invalid-relpath rejection, got: {reason}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn note_append_round_trips_through_dispatch_with_documented_arg_shape() {
        // Mirror of the StorageNoteAppendArgs contract — keys must be
        // `path` (string) + `snippet` (string), return shape must match
        // FileMetadata. The other tests cover the on-disk semantics; this
        // one pins the IPC contract a frontend would consume.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        let resp = plugin
            .dispatch(
                HANDLER_NOTE_APPEND,
                &serde_json::json!({
                    "path": "notes/quick.md",
                    "snippet": "hello world",
                }),
            )
            .expect("dispatch succeeds with documented args");

        assert!(resp.is_object(), "response must be a JSON object");
        for key in ["path", "size_bytes", "modified_at", "content_hash"] {
            assert!(
                resp.get(key).is_some(),
                "FileMetadata key '{key}' missing from response: {resp}"
            );
        }
        assert_eq!(
            resp.get("path").and_then(|v| v.as_str()),
            Some("notes/quick.md"),
        );
    }

    #[test]
    fn backlinks_to_block_dispatch_requires_block_id_arg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        // Missing block_id surfaces as ExecutionFailed with a clear reason
        // rather than silently filtering on an empty needle.
        let err = plugin
            .dispatch(
                HANDLER_BACKLINKS_TO_BLOCK,
                &serde_json::json!({ "path": "target.md" }),
            )
            .expect_err("missing block_id must reject");
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("block_id"),
                    "expected block_id rejection, got: {reason}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn backlinks_to_block_dispatch_returns_empty_for_unknown_path() {
        // Empty graph — handler should return [] rather than error so
        // shells can render the empty-state without special-casing.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());

        let resp = plugin
            .dispatch(
                HANDLER_BACKLINKS_TO_BLOCK,
                &serde_json::json!({
                    "path": "no-such.md",
                    "block_id": "11111111-1111-4111-8111-111111111111",
                }),
            )
            .expect("dispatch succeeds with documented args");
        assert!(resp.is_array(), "response must be a JSON array");
        assert_eq!(resp.as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn write_default_gitignore_dispatches_and_is_idempotent() {
        // BL-007 — the IPC dispatch path must produce the same on-disk
        // outcome as `Forge::write_default_gitignore`. This test pins
        // the JSON shape (`{ "wrote": bool }`) the bootstrap helper
        // and the CLI's `enable_transport` rely on, plus the
        // idempotent re-run contract.
        //
        // `boot_plugin` calls `StorageEngine::init` which already runs
        // `Forge::init` (and therefore the gitignore write) — that's
        // the post-BL-007 behaviour for fresh forges. To exercise the
        // "old forge bootstrapped via enable-transport" path, delete
        // the file before dispatching so the first call reports
        // `wrote: true`.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());
        let path = dir.path().join(".forge").join(".gitignore");
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists(), "test setup: file must be absent before dispatch");

        let resp = plugin
            .dispatch(HANDLER_WRITE_DEFAULT_GITIGNORE, &serde_json::json!({}))
            .expect("write_default_gitignore dispatch");
        assert_eq!(resp.get("wrote").and_then(serde_json::Value::as_bool), Some(true));
        assert!(path.exists(), "fresh write must create the file");

        let resp_again = plugin
            .dispatch(HANDLER_WRITE_DEFAULT_GITIGNORE, &serde_json::json!({}))
            .expect("write_default_gitignore second dispatch");
        assert_eq!(
            resp_again.get("wrote").and_then(serde_json::Value::as_bool),
            Some(false),
            "re-run must report no-op"
        );
    }

    #[test]
    fn build_appended_handles_existing_trailing_newlines_idempotently() {
        // No matter how many trailing newlines the existing buffer has,
        // we collapse to exactly one blank-line separator + trailing nl.
        assert_eq!(build_appended("", "a"), "a\n");
        assert_eq!(build_appended("a", "b"), "a\n\nb\n");
        assert_eq!(build_appended("a\n", "b"), "a\n\nb\n");
        assert_eq!(build_appended("a\n\n", "b"), "a\n\nb\n");
        assert_eq!(build_appended("a\n\n\n", "b"), "a\n\nb\n");
        // Snippet trailing newlines are normalised too.
        assert_eq!(build_appended("a", "b\n"), "a\n\nb\n");
        assert_eq!(build_appended("a", "b\n\n"), "a\n\nb\n");
    }

    // ── BL-128 thin slice — entity dispatch arms ─────────────────────────────

    fn seed_entity(forge: &std::path::Path, stem: &str, frontmatter: &str, body: &str) {
        let dir = forge.join(crate::entity_index::ENTITIES_DIR);
        std::fs::create_dir_all(&dir).expect("mkdir entities");
        std::fs::write(
            dir.join(format!("{stem}.md")),
            format!("---\n{frontmatter}---\n{body}"),
        )
        .expect("write entity");
    }

    #[test]
    fn entity_search_returns_typed_hits_and_filters_by_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());
        seed_entity(
            dir.path(),
            "alice",
            "entity_type: person\ndescription: Engineer working on nexus.\n",
            "",
        );
        seed_entity(
            dir.path(),
            "nexus",
            "entity_type: project\ndescription: A microkernel forge.\n",
            "",
        );

        let resp = plugin
            .dispatch(
                HANDLER_ENTITY_SEARCH,
                &serde_json::json!({ "query": "nexus" }),
            )
            .expect("entity_search ok");
        let results = resp
            .get("results")
            .and_then(serde_json::Value::as_array)
            .expect("results array");
        // alice's description mentions "nexus" so it matches alongside
        // the canonical "nexus" project entity — but nexus ranks higher.
        assert!(results.len() >= 1);
        assert_eq!(results[0].get("id").and_then(|v| v.as_str()), Some("nexus"));

        let typed = plugin
            .dispatch(
                HANDLER_ENTITY_SEARCH,
                &serde_json::json!({ "query": "", "entity_type": "person" }),
            )
            .expect("entity_search with filter");
        let typed_results = typed.get("results").and_then(serde_json::Value::as_array).unwrap();
        assert_eq!(typed_results.len(), 1);
        assert_eq!(typed_results[0].get("id").and_then(|v| v.as_str()), Some("alice"));
    }

    #[test]
    fn entity_get_returns_null_for_missing_and_record_for_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());
        seed_entity(
            dir.path(),
            "alice",
            "entity_type: person\naliases: [Al]\nrelations:\n  - target: nexus\n    type: works_on\n",
            "",
        );

        let missing = plugin
            .dispatch(HANDLER_ENTITY_GET, &serde_json::json!({ "id": "ghost" }))
            .expect("get ghost ok");
        assert!(missing.get("entity").is_some_and(serde_json::Value::is_null));

        let present = plugin
            .dispatch(HANDLER_ENTITY_GET, &serde_json::json!({ "id": "Al" }))
            .expect("get by alias ok");
        let entity = present.get("entity").expect("entity field");
        assert_eq!(entity.get("id").and_then(|v| v.as_str()), Some("alice"));
        assert_eq!(
            entity.get("entity_type").and_then(|v| v.as_str()),
            Some("person"),
        );
        let relations = entity
            .get("relations")
            .and_then(serde_json::Value::as_array)
            .expect("relations array");
        assert_eq!(relations.len(), 1);
        assert_eq!(
            relations[0].get("target").and_then(|v| v.as_str()),
            Some("nexus"),
        );
    }

    #[test]
    fn entity_relations_default_both_with_alias_resolution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin = boot_plugin(dir.path());
        seed_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n",
            "",
        );
        seed_entity(
            dir.path(),
            "bob",
            "entity_type: person\nrelations:\n  - target: alice\n    type: knows\n",
            "",
        );
        seed_entity(dir.path(), "nexus", "entity_type: project\n", "");

        let both = plugin
            .dispatch(
                HANDLER_ENTITY_RELATIONS,
                &serde_json::json!({ "id": "alice" }),
            )
            .expect("relations both");
        let rows = both
            .get("relations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        // alice has 1 outgoing (alice→nexus) + 1 incoming (bob→alice) = 2.
        assert_eq!(rows.len(), 2);

        let outgoing_only = plugin
            .dispatch(
                HANDLER_ENTITY_RELATIONS,
                &serde_json::json!({ "id": "alice", "direction": "outgoing" }),
            )
            .expect("relations outgoing");
        let rows = outgoing_only
            .get("relations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("to").and_then(|v| v.as_str()), Some("nexus"));
    }

    // ── apply_frontmatter_edit (P4-07) ───────────────────────────────────────

    #[test]
    fn frontmatter_edit_inserts_new_key_when_block_absent() {
        let out = apply_frontmatter_edit("body line\n", "title", Some("Hello"));
        assert_eq!(out, "---\ntitle: Hello\n---\n\nbody line\n");
    }

    #[test]
    fn frontmatter_edit_appends_into_existing_block() {
        let src = "---\ntitle: Old\n---\n\nbody\n";
        let out = apply_frontmatter_edit(src, "tags", Some("a, b"));
        assert_eq!(out, "---\ntitle: Old\ntags: a, b\n---\n\nbody\n");
    }

    #[test]
    fn frontmatter_edit_replaces_existing_key() {
        let src = "---\ntitle: Old\ntags: a\n---\n\nbody\n";
        let out = apply_frontmatter_edit(src, "title", Some("New"));
        assert_eq!(out, "---\ntitle: New\ntags: a\n---\n\nbody\n");
    }

    #[test]
    fn frontmatter_edit_removes_key_when_value_none() {
        let src = "---\ntitle: T\ntags: a\n---\n\nbody\n";
        let out = apply_frontmatter_edit(src, "tags", None);
        assert_eq!(out, "---\ntitle: T\n---\n\nbody\n");
    }

    #[test]
    fn frontmatter_edit_remove_missing_key_is_noop() {
        let src = "---\ntitle: T\n---\n\nbody\n";
        let out = apply_frontmatter_edit(src, "tags", None);
        assert_eq!(out, src);
    }

    #[test]
    fn frontmatter_edit_remove_when_block_absent_is_noop() {
        let out = apply_frontmatter_edit("body\n", "title", None);
        assert_eq!(out, "body\n");
    }
}
