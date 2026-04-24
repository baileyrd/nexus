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
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};

use crate::{FileFilter, StorageConfig, StorageEngine, TaskFilter};
use crate::watcher::{StorageEvent, Watcher};

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
    /// Wrapped in [`Mutex`] purely so the plugin stays `Sync` — `StorageEngine`
    /// itself owns a `Watcher` whose `mpsc::Receiver` is `Send` but not `Sync`.
    /// Storage methods all take `&self` so no fine-grained locking is needed.
    engine: Option<Mutex<StorageEngine>>,
    stop_tx: Option<mpsc::SyncSender<()>>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
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
        }
    }

    /// Direct access to the underlying engine for the bootstrap/CLI during
    /// migration. Returns `None` before `on_init` has run successfully.
    ///
    /// Callers must lock the returned [`Mutex`]; in practice the lock is
    /// always held briefly because [`StorageEngine`] methods all take `&self`.
    #[must_use]
    pub fn engine(&self) -> Option<&Mutex<StorageEngine>> {
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
        self.engine = Some(Mutex::new(engine));
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
        match handler_id {
            HANDLER_CONFIG_READ => return dispatch_config_read(&self.forge_root, args),
            HANDLER_CONFIG_RESET => return dispatch_config_reset(&self.forge_root, args),
            _ => {}
        }

        let engine_mutex = self.engine.as_ref().ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "storage engine not initialised (on_init did not run)".to_string(),
        })?;
        let engine = engine_mutex.lock().map_err(|_| exec_err("engine lock poisoned".to_string()))?;

        match handler_id {
            HANDLER_QUERY_FILES => {
                let filter: FileFilter = parse_args(args, "query_files")?;
                let records = engine
                    .query_files(&filter)
                    .map_err(|e| exec_err(format!("query_files: {e}")))?;
                to_value(&records, "query_files")
            }
            HANDLER_READ_FILE => {
                let path = path_arg(args, "read_file")?;
                match engine.read_file(&path) {
                    Ok(bytes) => Ok(serde_json::json!({ "bytes": bytes })),
                    // Missing files are an expected outcome for callers probing
                    // `.forge/workspace.json` on first boot, etc. Return a typed
                    // null rather than an error so the IPC bridge doesn't
                    // surface it as `PluginCrashedDuringCall`.
                    Err(crate::StorageError::FileNotFound(_)) => {
                        Ok(serde_json::json!({ "bytes": null }))
                    }
                    Err(e) => Err(exec_err(format!("read_file: {e}"))),
                }
            }
            HANDLER_BACKLINKS => {
                let path = path_arg(args, "backlinks")?;
                let results = engine
                    .backlinks(&path)
                    .map_err(|e| exec_err(format!("backlinks: {e}")))?;
                to_value(&results, "backlinks")
            }
            HANDLER_QUERY_TASKS => {
                let filter: TaskFilter = parse_args(args, "query_tasks")?;
                let records = engine
                    .query_tasks(&filter)
                    .map_err(|e| exec_err(format!("query_tasks: {e}")))?;
                to_value(&records, "query_tasks")
            }
            HANDLER_GRAPH_STATS => {
                let stats = engine
                    .graph_stats()
                    .map_err(|e| exec_err(format!("graph_stats: {e}")))?;
                to_value(&stats, "graph_stats")
            }
            HANDLER_REBUILD_INDEX => {
                let stats = engine
                    .rebuild_index()
                    .map_err(|e| exec_err(format!("rebuild_index: {e}")))?;
                to_value(&stats, "rebuild_index")
            }
            HANDLER_SEARCH => {
                let query = args
                    .get("query")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| exec_err("search: missing 'query' string".to_string()))?;
                let limit = args
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(50);
                let results = engine
                    .search(query, limit)
                    .map_err(|e| exec_err(format!("search: {e}")))?;
                to_value(&results, "search")
            }
            HANDLER_WRITE_FILE => {
                let path = path_arg(args, "write_file")?;
                let bytes: Vec<u8> = args
                    .get("bytes")
                    .ok_or_else(|| exec_err("write_file: missing 'bytes'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("write_file: bytes decode: {e}")))
                    })?;
                let meta = engine
                    .write_file(&path, &bytes)
                    .map_err(|e| exec_err(format!("write_file: {e}")))?;
                to_value(&meta, "write_file")
            }
            HANDLER_WRITE_VAULT_FILE => {
                let path = path_arg(args, "write_vault_file")?;
                let bytes: Vec<u8> = args
                    .get("bytes")
                    .ok_or_else(|| exec_err("write_vault_file: missing 'bytes'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            exec_err(format!("write_vault_file: bytes decode: {e}"))
                        })
                    })?;
                engine
                    .write_raw(&path, &bytes)
                    .map_err(|e| exec_err(format!("write_vault_file: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_DELETE_FILE => {
                let path = path_arg(args, "delete_file")?;
                engine
                    .delete_file(&path)
                    .map_err(|e| exec_err(format!("delete_file: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_FILE_EXISTS => {
                let path = path_arg(args, "file_exists")?;
                let exists = engine
                    .file_exists(&path)
                    .map_err(|e| exec_err(format!("file_exists: {e}")))?;
                Ok(serde_json::json!({ "exists": exists }))
            }
            HANDLER_REBUILD_SEARCH_INDEX => {
                engine
                    .rebuild_search_index()
                    .map_err(|e| exec_err(format!("rebuild_search_index: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_TOGGLE_TASK => {
                let task_id = args
                    .get("task_id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| exec_err("toggle_task: missing 'task_id' (u64)".to_string()))?;
                let record = engine
                    .toggle_task(task_id)
                    .map_err(|e| exec_err(format!("toggle_task: {e}")))?;
                to_value(&record, "toggle_task")
            }
            HANDLER_OUTGOING_LINKS => {
                let path = path_arg(args, "outgoing_links")?;
                let links = engine
                    .outgoing_links(&path)
                    .map_err(|e| exec_err(format!("outgoing_links: {e}")))?;
                to_value(&links, "outgoing_links")
            }
            HANDLER_UNRESOLVED_LINKS => {
                let links = engine
                    .unresolved_links()
                    .map_err(|e| exec_err(format!("unresolved_links: {e}")))?;
                to_value(&links, "unresolved_links")
            }
            HANDLER_LIST_ALL_LINKS => {
                let snapshot = engine
                    .list_all_links()
                    .map_err(|e| exec_err(format!("list_all_links: {e}")))?;
                to_value(&snapshot, "list_all_links")
            }
            HANDLER_CANVAS_READ => {
                let path = path_arg(args, "canvas_read")?;
                let canvas_file = engine
                    .read_canvas(&path)
                    .map_err(|e| exec_err(format!("canvas_read: {e}")))?;
                to_value(&canvas_file, "canvas_read")
            }
            HANDLER_CANVAS_WRITE => {
                let path = path_arg(args, "canvas_write")?;
                let canvas_file: crate::CanvasFile = args
                    .get("canvas")
                    .ok_or_else(|| exec_err("canvas_write: missing 'canvas'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("canvas_write: canvas decode: {e}")))
                    })?;
                let meta = engine
                    .write_canvas(&path, &canvas_file)
                    .map_err(|e| exec_err(format!("canvas_write: {e}")))?;
                to_value(&meta, "canvas_write")
            }
            HANDLER_CANVAS_PATCH => {
                let path = path_arg(args, "canvas_patch")?;
                let ops: Vec<crate::CanvasPatchOp> = args
                    .get("ops")
                    .ok_or_else(|| exec_err("canvas_patch: missing 'ops'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("canvas_patch: ops decode: {e}")))
                    })?;
                let meta = engine
                    .patch_canvas(&path, &ops)
                    .map_err(|e| exec_err(format!("canvas_patch: {e}")))?;
                to_value(&meta, "canvas_patch")
            }
            HANDLER_CANVAS_NODES => {
                let path = path_arg(args, "canvas_nodes")?;
                let nodes = engine
                    .canvas_nodes_by_path(&path)
                    .map_err(|e| exec_err(format!("canvas_nodes: {e}")))?;
                to_value(&nodes, "canvas_nodes")
            }
            HANDLER_CANVAS_EDGES => {
                let path = path_arg(args, "canvas_edges")?;
                let edges = engine
                    .canvas_edges_by_path(&path)
                    .map_err(|e| exec_err(format!("canvas_edges: {e}")))?;
                to_value(&edges, "canvas_edges")
            }
            HANDLER_BASE_RECORD_CREATE => {
                let path = path_arg(args, "base_record_create")?;
                let record: nexus_types::bases::BaseRecord = args
                    .get("record")
                    .ok_or_else(|| exec_err("base_record_create: missing 'record'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            exec_err(format!("base_record_create: record decode: {e}"))
                        })
                    })?;
                let stored = engine
                    .base_record_create(&path, record)
                    .map_err(|e| exec_err(format!("base_record_create: {e}")))?;
                to_value(&stored, "base_record_create")
            }
            HANDLER_BASE_RECORD_UPDATE => {
                let path = path_arg(args, "base_record_update")?;
                let record_id = args
                    .get("record_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_record_update: missing 'record_id' string".to_string())
                    })?;
                let fields = args
                    .get("fields")
                    .and_then(serde_json::Value::as_object)
                    .cloned()
                    .ok_or_else(|| {
                        exec_err("base_record_update: missing 'fields' object".to_string())
                    })?;
                let updated = engine
                    .base_record_update(&path, record_id, &fields)
                    .map_err(|e| exec_err(format!("base_record_update: {e}")))?;
                to_value(&updated, "base_record_update")
            }
            HANDLER_BASE_RECORD_DELETE => {
                let path = path_arg(args, "base_record_delete")?;
                let record_id = args
                    .get("record_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_record_delete: missing 'record_id' string".to_string())
                    })?;
                engine
                    .base_record_delete(&path, record_id)
                    .map_err(|e| exec_err(format!("base_record_delete: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_PROPERTY_CREATE => {
                let path = path_arg(args, "base_property_create")?;
                let name = name_arg(args, "base_property_create")?;
                let definition = args
                    .get("definition")
                    .cloned()
                    .ok_or_else(|| {
                        exec_err("base_property_create: missing 'definition'".to_string())
                    })?;
                engine
                    .base_property_create(&path, &name, definition)
                    .map_err(|e| exec_err(format!("base_property_create: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_PROPERTY_UPDATE => {
                let path = path_arg(args, "base_property_update")?;
                let name = name_arg(args, "base_property_update")?;
                let definition = args
                    .get("definition")
                    .cloned()
                    .ok_or_else(|| {
                        exec_err("base_property_update: missing 'definition'".to_string())
                    })?;
                let migrate_values = args
                    .get("migrate_values")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                engine
                    .base_property_update(&path, &name, &definition, migrate_values)
                    .map_err(|e| exec_err(format!("base_property_update: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_RECORD_SOFT_DELETE => {
                let path = path_arg(args, "base_record_soft_delete")?;
                let record_id = args
                    .get("record_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_record_soft_delete: missing 'record_id' string".to_string())
                    })?;
                engine
                    .base_record_soft_delete(&path, record_id)
                    .map_err(|e| exec_err(format!("base_record_soft_delete: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_RECORD_RESTORE => {
                let path = path_arg(args, "base_record_restore")?;
                let record_id = args
                    .get("record_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_record_restore: missing 'record_id' string".to_string())
                    })?;
                engine
                    .base_record_restore(&path, record_id)
                    .map_err(|e| exec_err(format!("base_record_restore: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_PROPERTY_RENAME => {
                let path = path_arg(args, "base_property_rename")?;
                let old_name = args
                    .get("old_name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_property_rename: missing 'old_name' string".to_string())
                    })?
                    .to_string();
                let new_name = args
                    .get("new_name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        exec_err("base_property_rename: missing 'new_name' string".to_string())
                    })?
                    .to_string();
                engine
                    .base_property_rename(&path, &old_name, &new_name)
                    .map_err(|e| exec_err(format!("base_property_rename: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_CREATE => {
                let path = path_arg(args, "base_create")?;
                let schema: nexus_types::bases::BaseSchema = args
                    .get("schema")
                    .ok_or_else(|| exec_err("base_create: missing 'schema'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("base_create: schema decode: {e}")))
                    })?;
                let seed_records: Vec<nexus_types::bases::BaseRecord> = args
                    .get("seed_records")
                    .cloned()
                    .map(|v| {
                        serde_json::from_value(v)
                            .map_err(|e| exec_err(format!("base_create: seed_records decode: {e}")))
                    })
                    .transpose()?
                    .unwrap_or_default();
                let base = engine
                    .base_create(&path, &schema, seed_records)
                    .map_err(|e| exec_err(format!("base_create: {e}")))?;
                to_value(&base, "base_create")
            }
            HANDLER_BASE_PROPERTY_DELETE => {
                let path = path_arg(args, "base_property_delete")?;
                let name = name_arg(args, "base_property_delete")?;
                engine
                    .base_property_delete(&path, &name)
                    .map_err(|e| exec_err(format!("base_property_delete: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_VIEW_CREATE => {
                let path = path_arg(args, "base_view_create")?;
                let view: nexus_types::bases::BaseView = args
                    .get("view")
                    .ok_or_else(|| exec_err("base_view_create: missing 'view'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            exec_err(format!("base_view_create: view decode: {e}"))
                        })
                    })?;
                engine
                    .base_view_create(&path, view)
                    .map_err(|e| exec_err(format!("base_view_create: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_VIEW_UPDATE => {
                let path = path_arg(args, "base_view_update")?;
                let view: nexus_types::bases::BaseView = args
                    .get("view")
                    .ok_or_else(|| exec_err("base_view_update: missing 'view'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            exec_err(format!("base_view_update: view decode: {e}"))
                        })
                    })?;
                engine
                    .base_view_update(&path, view)
                    .map_err(|e| exec_err(format!("base_view_update: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_VIEW_DELETE => {
                let path = path_arg(args, "base_view_delete")?;
                let name = name_arg(args, "base_view_delete")?;
                engine
                    .base_view_delete(&path, &name)
                    .map_err(|e| exec_err(format!("base_view_delete: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_GRAPH_NEIGHBORS => {
                let path = path_arg(args, "graph_neighbors")?;
                let depth = args
                    .get("depth")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| exec_err("graph_neighbors: missing 'depth' (u64)".to_string()))?;
                let paths = engine
                    .graph_neighbors(&path, depth)
                    .map_err(|e| exec_err(format!("graph_neighbors: {e}")))?;
                to_value(&paths, "graph_neighbors")
            }
            HANDLER_QUERY_TAGS => {
                let name = args
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| exec_err("query_tags: missing 'name' string".to_string()))?;
                let tags = engine
                    .query_tags(name)
                    .map_err(|e| exec_err(format!("query_tags: {e}")))?;
                to_value(&tags, "query_tags")
            }
            HANDLER_VECTOR_INSERT => {
                let file_path = args
                    .get("file_path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| exec_err("vector_insert: missing 'file_path' string".to_string()))?
                    .to_string();
                let chunks: Vec<crate::vectorstore::ChunkEmbedding> = args
                    .get("chunks")
                    .ok_or_else(|| exec_err("vector_insert: missing 'chunks'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("vector_insert: chunks decode: {e}")))
                    })?;
                engine
                    .vector_insert(&file_path, &chunks)
                    .map_err(|e| exec_err(format!("vector_insert: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_VECTOR_QUERY => {
                let embedding: Vec<f32> = args
                    .get("embedding")
                    .ok_or_else(|| exec_err("vector_query: missing 'embedding'".to_string()))
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| exec_err(format!("vector_query: embedding decode: {e}")))
                    })?;
                let limit = args
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(5);
                let matches = engine
                    .vector_query(&embedding, limit)
                    .map_err(|e| exec_err(format!("vector_query: {e}")))?;
                to_value(&matches, "vector_query")
            }
            HANDLER_VECTOR_DELETE_BY_FILE => {
                let path = path_arg(args, "vector_delete_by_file")?;
                engine
                    .vector_delete_by_file(&path)
                    .map_err(|e| exec_err(format!("vector_delete_by_file: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_VECTORSTORE_COUNT => {
                let count = engine
                    .vectorstore_count()
                    .map_err(|e| exec_err(format!("vectorstore_count: {e}")))?;
                Ok(serde_json::json!({ "count": count }))
            }
            HANDLER_QUERY_BLOCKS => {
                let path = path_arg(args, "query_blocks")?;
                let blocks = engine
                    .query_blocks_by_path(&path)
                    .map_err(|e| exec_err(format!("query_blocks: {e}")))?;
                to_value(&blocks, "query_blocks")
            }
            HANDLER_BASE_INDEX => {
                let path = path_arg(args, "base_index")?;
                let abs_dir = self.forge_root.join(&path);
                let base = nexus_types::bases::load_base(&abs_dir)
                    .map_err(|e| exec_err(format!("base_index: load: {e}")))?;
                let base_id = engine
                    .index_base(&path, &base)
                    .map_err(|e| exec_err(format!("base_index: {e}")))?;
                Ok(serde_json::json!({ "base_id": base_id }))
            }
            HANDLER_BASE_LOAD => {
                let path = path_arg(args, "base_load")?;
                let abs_dir = self.forge_root.join(&path);
                let base = nexus_types::bases::load_base(&abs_dir)
                    .map_err(|e| exec_err(format!("base_load: {e}")))?;
                to_value(&base, "base_load")
            }
            HANDLER_BASE_LIST => {
                let bases = engine
                    .list_bases()
                    .map_err(|e| exec_err(format!("base_list: {e}")))?;
                to_value(&bases, "base_list")
            }
            HANDLER_LIST_DIR => {
                let relpath = args
                    .get("relpath")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let entries = engine
                    .list_dir(&relpath)
                    .map_err(|e| exec_err(format!("list_dir: {e}")))?;
                to_value(&entries, "list_dir")
            }
            HANDLER_CREATE_FILE => {
                let relpath = relpath_arg(args, "create_file")?;
                engine
                    .create_file(&relpath)
                    .map_err(|e| exec_err(format!("create_file: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_CREATE_DIR => {
                let relpath = relpath_arg(args, "create_dir")?;
                engine
                    .create_dir(&relpath)
                    .map_err(|e| exec_err(format!("create_dir: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_RENAME_ENTRY => {
                let from = args
                    .get("from")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| exec_err("rename_entry: missing 'from' string".to_string()))?;
                let to = args
                    .get("to")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| exec_err("rename_entry: missing 'to' string".to_string()))?;
                engine
                    .rename_entry(from, to)
                    .map_err(|e| exec_err(format!("rename_entry: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_DELETE_ENTRY => {
                let relpath = relpath_arg(args, "delete_entry")?;
                engine
                    .delete_entry(&relpath)
                    .map_err(|e| exec_err(format!("delete_entry: {e}")))?;
                Ok(serde_json::json!({}))
            }
            HANDLER_BASE_QUERY => {
                let path = path_arg(args, "base_query")?;
                let filters: Vec<String> = args
                    .get("filters")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sorts: Vec<String> = args
                    .get("sorts")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let limit = args
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let offset = args
                    .get("offset")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok());

                let bases = engine
                    .list_bases()
                    .map_err(|e| exec_err(format!("base_query: list_bases: {e}")))?;
                let base_summary = bases
                    .iter()
                    .find(|b| b.path == path)
                    .ok_or_else(|| exec_err(format!("base_query: base not found: {path}")))?;

                let mut db_query = crate::bases::query::Query {
                    base_id: base_summary.id,
                    ..Default::default()
                };
                for f in &filters {
                    db_query.filters.push(
                        crate::bases::query::parse_filter(f)
                            .map_err(|e| exec_err(format!("base_query: parse filter '{f}': {e}")))?,
                    );
                }
                for s in &sorts {
                    db_query.sorts.push(
                        crate::bases::query::parse_sort(s)
                            .map_err(|e| exec_err(format!("base_query: parse sort '{s}': {e}")))?,
                    );
                }
                db_query.limit = limit;
                db_query.offset = offset;

                let conn = engine
                    .pool_connection()
                    .map_err(|e| exec_err(format!("base_query: pool: {e}")))?;
                let result = crate::bases::query::execute(&conn, &db_query)
                    .map_err(|e| exec_err(format!("base_query: {e}")))?;
                to_value(&result, "base_query")
            }
            _ => Err(exec_err(format!("unknown handler id {handler_id}"))),
        }
    }
}

// ── Dispatch helpers ─────────────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn parse_args<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    // Empty object and null both mean "no args" — accept both.
    if value.is_null() || matches!(value.as_object(), Some(o) if o.is_empty()) {
        return serde_json::from_value(serde_json::json!({}))
            .map_err(|e| exec_err(format!("{command}: default args invalid: {e}")));
    }
    serde_json::from_value(value.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn path_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    value
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("{command}: missing 'path' string argument")))
}

fn relpath_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    value
        .get("relpath")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("{command}: missing 'relpath' string argument")))
}

fn name_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("{command}: missing 'name' string argument")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize failed: {e}")))
}

// ── Config handlers ──────────────────────────────────────────────────────────

fn config_kind(args: &serde_json::Value) -> Result<&str, PluginError> {
    args.get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("config: missing 'kind' string argument".to_string()))
}

fn dispatch_config_read(
    forge_root: &std::path::Path,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let kind = config_kind(args)?;
    let (format, content) = match kind {
        "app" => {
            let cfg = crate::config::load_app_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            ("toml", toml::to_string_pretty(&cfg)
                .map_err(|e| exec_err(format!("config_read: serialize app: {e}")))?)
        }
        "workspace" => {
            let state = crate::config::load_workspace_state(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            ("json", serde_json::to_string_pretty(&state)
                .map_err(|e| exec_err(format!("config_read: serialize workspace: {e}")))?)
        }
        "mcp" => {
            let cfg = crate::config::load_mcp_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            ("toml", toml::to_string_pretty(&cfg)
                .map_err(|e| exec_err(format!("config_read: serialize mcp: {e}")))?)
        }
        "ai" => {
            let cfg = crate::config::load_ai_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            ("toml", toml::to_string_pretty(&cfg)
                .map_err(|e| exec_err(format!("config_read: serialize ai: {e}")))?)
        }
        other => return Err(exec_err(format!(
            "config_read: unknown kind '{other}' (expected app|workspace|mcp|ai)"
        ))),
    };
    Ok(serde_json::json!({ "format": format, "content": content }))
}

fn dispatch_config_reset(
    forge_root: &std::path::Path,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let kind = config_kind(args)?;
    match kind {
        "app" => crate::config::save_app_config(forge_root, &crate::config::AppConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "workspace" => crate::config::save_workspace_state(
            forge_root,
            &crate::config::WorkspaceState::default(),
        )
        .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "mcp" => crate::config::save_mcp_config(forge_root, &crate::config::McpConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "ai" => crate::config::save_ai_config(forge_root, &crate::config::AiConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        other => return Err(exec_err(format!(
            "config_reset: unknown kind '{other}' (expected app|workspace|mcp|ai)"
        ))),
    }
    Ok(serde_json::json!({}))
}

// ── Bridge thread ──────────────────────────────────────────────────────────────

/// Polls the watcher until the stop signal arrives, translating each
/// [`StorageEvent`] into a [`NexusEvent`] published on the kernel bus.
///
/// The bridge only handles event translation and publication.  Index updates
/// (`write_file`, `delete_file`, etc.) remain the caller's responsibility.
#[allow(clippy::needless_pass_by_value)]
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
/// and publish on the bus.
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
        }

        StorageEvent::FileModified { path, content_hash } => {
            if path.is_empty() {
                // Empty path is the reconcile signal emitted after a git batch
                // burst.  Emit a custom indexing event so subscribers know a
                // reconcile pass is warranted.
                let _ = bus.publish_plugin(
                    PLUGIN_ID,
                    "com.nexus.storage.indexing.started",
                    serde_json::json!({}),
                );
                // Note: actual reconcile is the caller's responsibility.
                // Emit completed immediately so subscribers aren't left waiting.
                let _ = bus.publish_plugin(
                    PLUGIN_ID,
                    "com.nexus.storage.indexing.completed",
                    serde_json::json!({ "triggered_by": "git-batch-mode" }),
                );
            } else {
                let _ = bus.publish_plugin(
                    PLUGIN_ID,
                    "com.nexus.storage.file_modified",
                    serde_json::json!({
                        "path": path,
                        "content_hash": content_hash,
                    }),
                );
            }
        }

        StorageEvent::FileDeleted { path } => {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.storage.file_deleted",
                serde_json::json!({ "path": path }),
            );
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
        }
    }
}
