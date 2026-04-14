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
/// Handler id for `read_file`. Args: `{ "path": String }`; Returns: `{ "bytes": Vec<u8> }`.
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
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
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
                let bytes = engine
                    .read_file(&path)
                    .map_err(|e| exec_err(format!("read_file: {e}")))?;
                Ok(serde_json::json!({ "bytes": bytes }))
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

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize failed: {e}")))
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
