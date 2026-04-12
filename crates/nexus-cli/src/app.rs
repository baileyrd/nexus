use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nexus_plugins::{PluginManager, PluginManagerConfig};
use nexus_storage::{StorageConfig, StorageEngine};

use crate::output::OutputFormat;

/// Central application state, owning all subsystems with lazy initialisation.
pub struct App {
    forge_root: PathBuf,
    storage: Option<StorageEngine>,
    plugins: Option<PluginManager>,
    format: OutputFormat,
}

impl App {
    /// Create a new `App` with the given forge root and output format.
    ///
    /// Subsystems are not opened until first use.
    pub fn new(forge_root: PathBuf, format: OutputFormat) -> Self {
        Self {
            forge_root,
            storage: None,
            plugins: None,
            format,
        }
    }

    /// Return the forge root directory.
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Return the configured output format.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Open the storage engine lazily (creates on first call, reuses after).
    ///
    /// # Errors
    ///
    /// Returns an error if the forge directory does not exist or the engine
    /// cannot be opened.
    pub fn storage(&mut self) -> Result<&StorageEngine> {
        if self.storage.is_none() {
            let engine = StorageEngine::open(&self.forge_root, &StorageConfig::default())
                .with_context(|| {
                    format!(
                        "failed to open forge at '{}'",
                        self.forge_root.display()
                    )
                })?;
            self.storage = Some(engine);
        }
        Ok(self.storage.as_ref().expect("just initialised"))
    }

    /// Open the storage engine lazily and return a mutable reference.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge directory does not exist or the engine
    /// cannot be opened.
    pub fn storage_mut(&mut self) -> Result<&mut StorageEngine> {
        if self.storage.is_none() {
            let engine = StorageEngine::open(&self.forge_root, &StorageConfig::default())
                .with_context(|| {
                    format!(
                        "failed to open forge at '{}'",
                        self.forge_root.display()
                    )
                })?;
            self.storage = Some(engine);
        }
        Ok(self.storage.as_mut().expect("just initialised"))
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
            let manager = PluginManager::new(&plugins_dir, &config).with_context(|| {
                format!(
                    "failed to create plugin manager at '{}'",
                    plugins_dir.display()
                )
            })?;
            self.plugins = Some(manager);
        }
        Ok(self.plugins.as_mut().expect("just initialised"))
    }

    /// Initialise a new forge at `forge_root` without opening it.
    ///
    /// Creates the directory structure expected by the storage engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge already exists or directory creation fails.
    pub fn init_forge(&self) -> Result<()> {
        StorageEngine::init(&self.forge_root)
            .with_context(|| {
                format!(
                    "failed to initialise forge at '{}'",
                    self.forge_root.display()
                )
            })?;
        Ok(())
    }
}
