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

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};

use crate::StorageConfig;
use crate::watcher::{StorageEvent, Watcher};

/// Reverse-DNS identifier for this plugin.
const PLUGIN_ID: &str = "com.nexus.storage";

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
    debounce_ms: u64,
    event_bus: Arc<EventBus>,
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
            debounce_ms: config.debounce_ms,
            event_bus,
            stop_tx: None,
            bridge_thread: None,
        }
    }
}

impl CorePlugin for StorageCorePlugin {
    /// Verify that the forge directory exists.
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
        Ok(())
    }

    /// Start the forge watcher and the bridge thread that translates
    /// [`StorageEvent`]s into [`NexusEvent`]s on the kernel bus.
    fn on_start(&mut self) -> Result<(), PluginError> {
        let watcher = Watcher::start(&self.forge_root, self.debounce_ms)
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

    /// IPC dispatch — no commands registered yet; returns an error for all IDs.
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "unknown handler id {handler_id}; IPC commands not yet registered"
            ),
        })
    }
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
