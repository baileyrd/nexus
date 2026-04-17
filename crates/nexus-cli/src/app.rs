use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{Runtime, build_cli_runtime};
use nexus_kernel::EventBus;
use nexus_plugins::{PluginManager, PluginManagerConfig};
use tokio::runtime::Runtime as TokioRuntime;

use crate::output::OutputFormat;

/// Central application state, owning all subsystems with lazy initialisation.
pub struct App {
    forge_root: PathBuf,
    format: OutputFormat,
    /// Safe mode — skip every community plugin at load (F-8.2.2).
    safe_mode: bool,
    /// Kernel event bus handle retained for the community plugin manager.
    event_bus: Arc<EventBus>,
    /// Community-plugin manager (lazy).
    plugins: Option<PluginManager>,
    /// Bootstrap-assembled runtime (lazy).
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
            safe_mode: false,
            event_bus: Arc::new(EventBus::new(256)),
            plugins: None,
            runtime: None,
            rt: None,
        }
    }

    /// Enable safe mode so the plugin manager skips community plugins
    /// at load time (F-8.2.2). Must be set before `plugins()` is called.
    pub fn set_safe_mode(&mut self, enabled: bool) {
        self.safe_mode = enabled;
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
                safe_mode: self.safe_mode,
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
