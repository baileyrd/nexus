use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};
use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginManager, PluginManagerConfig};
use nexus_storage::{StorageCorePlugin, StorageConfig, StorageEngine};
use tokio::runtime::Runtime as TokioRuntime;

use crate::output::OutputFormat;

/// Central application state, owning all subsystems with lazy initialisation.
pub struct App {
    forge_root: PathBuf,
    format: OutputFormat,
    /// Kernel event bus — shared with the storage bridge thread.
    event_bus: Arc<EventBus>,
    /// The storage engine (lazy). Retained for subcommands that have not yet
    /// migrated to `ipc_call` (canvas, bases, ai, mcp, watch).
    storage: Option<StorageEngine>,
    /// Storage core plugin that bridges watcher events onto the kernel bus
    /// (lazy, started alongside `storage`).
    storage_plugin: Option<StorageCorePlugin>,
    /// Community-plugin manager (lazy).
    plugins: Option<PluginManager>,
    /// Bootstrap-assembled runtime (lazy). Used by subcommands that have
    /// migrated to the plugin-IPC boundary.
    runtime: Option<Runtime>,
    /// Tokio runtime used to block on async `ipc_call`s.
    rt: Option<TokioRuntime>,
}

impl App {
    /// Create a new `App` with the given forge root and output format.
    ///
    /// Subsystems are not opened until first use.
    pub fn new(forge_root: PathBuf, format: OutputFormat) -> Self {
        Self {
            forge_root,
            format,
            event_bus: Arc::new(EventBus::new(256)),
            storage: None,
            storage_plugin: None,
            plugins: None,
            runtime: None,
            rt: None,
        }
    }

    /// Lazily build the Nexus runtime (kernel + all core plugins + CLI as a
    /// Core plugin) and return a reference plus a Tokio runtime for blocking
    /// on async `ipc_call`s.
    ///
    /// First-use opens the storage engine inside the plugin, so the forge
    /// directory must already exist. Subcommands that run *before* forge
    /// init (e.g. `forge init`) must not call this — they use
    /// [`nexus_bootstrap::init_forge`] first.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime or Tokio runtime cannot be built.
    pub fn runtime(&mut self) -> Result<(&Runtime, &TokioRuntime)> {
        if self.runtime.is_none() {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .context("failed to start tokio runtime")?;
            let runtime = build_cli_runtime(self.forge_root.clone())
                .with_context(|| format!("failed to build runtime at {}", self.forge_root.display()))?;
            self.runtime = Some(runtime);
            self.rt = Some(rt);
        }
        Ok((
            self.runtime.as_ref().expect("just initialised"),
            self.rt.as_ref().expect("just initialised"),
        ))
    }

    /// Return the forge root directory.
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Return the configured output format.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Return the kernel event bus handle.
    #[allow(dead_code)]
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    /// Open the storage engine lazily (creates on first call, reuses after).
    ///
    /// Also starts the storage core plugin on first call, wiring the forge
    /// watcher to the kernel event bus so `com.nexus.storage.file_created` /
    /// `file_modified` / `file_deleted` / `file_renamed` events flow to subscribers.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge directory does not exist or the engine
    /// cannot be opened.
    pub fn storage(&mut self) -> Result<&StorageEngine> {
        if self.storage.is_none() {
            let config = StorageConfig::default();
            let engine = StorageEngine::open(&self.forge_root, &config)
                .with_context(|| {
                    format!(
                        "failed to open forge at '{}'",
                        self.forge_root.display()
                    )
                })?;
            self.storage = Some(engine);

            // Start the storage core plugin (watcher → kernel bus bridge).
            let mut plugin = StorageCorePlugin::new(
                self.forge_root.clone(),
                &config,
                Arc::clone(&self.event_bus),
            );
            if let Err(e) = plugin.on_init().and_then(|()| plugin.on_start()) {
                tracing::warn!(
                    error = %e,
                    "storage core plugin failed to start; file events will not \
                     be published to the kernel bus"
                );
            }
            self.storage_plugin = Some(plugin);
        }
        Ok(self.storage.as_ref().expect("just initialised"))
    }

    /// Open the storage engine lazily and return a shared reference.
    ///
    /// All [`StorageEngine`] mutation methods use interior mutability (`&self`),
    /// so a mutable reference is not required.  This method is an alias for
    /// [`storage`](Self::storage) kept for call-site compatibility.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge directory does not exist or the engine
    /// cannot be opened.
    pub fn storage_mut(&mut self) -> Result<&StorageEngine> {
        self.storage()
    }

    /// Create the plugin manager lazily (creates on first call, reuses after).
    ///
    /// The plugins directory is `.forge/plugins/` relative to the forge root.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin manager cannot be created.
    pub fn plugins(&mut self) -> Result<&mut PluginManager> {
        if self.plugins.is_none() {
            let plugins_dir = self.forge_root.join(".forge").join("plugins");
            let config = PluginManagerConfig {
                hot_reload: false,
                ..Default::default()
            };
            let mut manager =
                PluginManager::new(&plugins_dir, &config).with_context(|| {
                    format!(
                        "failed to create plugin manager at '{}'",
                        plugins_dir.display()
                    )
                })?;
            // Wire the kernel bus so community plugins can subscribe to events.
            manager.set_event_bus(Arc::clone(&self.event_bus));
            self.plugins = Some(manager);
        }
        Ok(self.plugins.as_mut().expect("just initialised"))
    }

}
