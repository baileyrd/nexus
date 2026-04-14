//! Nexus plugin system: manifest parsing, WASM sandbox, host functions,
//! plugin loader, settings validation, and hot-reload.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-04-plugins-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod host_fns;
pub mod manifest;
mod loader;
mod sandbox;
mod settings;
mod hot_reload;
mod scaffold;

pub use error::PluginError;
pub use scaffold::{scaffold, PluginTemplate, ScaffoldConfig};
pub use loader::{CorePlugin, CorePluginFuture, PluginLoader, SharedPluginLoader};
pub use manifest::{
    CliSubcommandReg, EventSubscriberReg, IpcCommandReg, LifecycleConfig, ManifestCapabilities,
    PluginManifest, Registrations, SettingsConfig, WasmConfig,
};
pub use manifest::{load_manifest, parse_manifest, validate};
pub use sandbox::{PluginData, WasmSandbox};
pub use settings::SettingsManager;
pub use hot_reload::{HotReloader, ReloadEvent};

// ─── PluginManagerConfig ──────────────────────────────────────────────────────

/// Configuration for [`PluginManager`].
#[derive(Debug, Clone)]
pub struct PluginManagerConfig {
    /// Whether to watch the plugins directory for WASM changes and
    /// automatically reload affected plugins. Default: `true`.
    pub hot_reload: bool,
    /// Debounce delay in milliseconds used by the file watcher.
    /// Default: `500`.
    pub debounce_ms: u64,
}

impl Default for PluginManagerConfig {
    fn default() -> Self {
        Self {
            hot_reload: true,
            debounce_ms: 500,
        }
    }
}

// ─── PluginManager ────────────────────────────────────────────────────────────

/// High-level facade that combines [`PluginLoader`] with optional
/// [`HotReloader`] support.
///
/// Use [`PluginManager::new`] to create an instance, then call
/// [`load_all`](Self::load_all) to scan and load all plugins in the configured
/// directory.
pub struct PluginManager {
    loader: loader::PluginLoader,
    reloader: Option<hot_reload::HotReloader>,
}

impl PluginManager {
    /// Create a new [`PluginManager`] rooted at `plugins_dir`.
    ///
    /// If `config.hot_reload` is `true`, a [`HotReloader`] watcher is started
    /// for the directory.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the hot-reload watcher cannot be started.
    pub fn new(plugins_dir: &std::path::Path, config: &PluginManagerConfig) -> Result<Self, PluginError> {
        let loader = loader::PluginLoader::new(plugins_dir);
        let reloader = if config.hot_reload {
            Some(hot_reload::HotReloader::start(plugins_dir, config.debounce_ms)?)
        } else {
            None
        };
        Ok(Self { loader, reloader })
    }

    /// Register a native Rust **core** plugin.
    ///
    /// Core plugins are compiled into the binary and bypass the WASM sandbox.
    /// `manifest` must have `trust_level = "core"` and no `[wasm]` section.
    /// `plugin_dir` is where `plugin.toml` and optional `settings.json` live.
    ///
    /// See [`PluginLoader::register_core`] for the full contract.
    ///
    /// # Errors
    /// Propagates errors from the underlying loader.
    pub fn register_core(
        &mut self,
        manifest: PluginManifest,
        plugin_dir: &std::path::Path,
        plugin: Box<dyn CorePlugin>,
    ) -> Result<nexus_kernel::PluginInfo, PluginError> {
        self.loader.register_core(manifest, plugin_dir, plugin)
    }

    /// Scan the plugins directory and load every subdirectory that contains a
    /// `manifest.toml`.
    ///
    /// Individual load failures are logged as warnings and skipped; the
    /// successful [`nexus_kernel::PluginInfo`]s are returned.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the directory scan itself fails.
    pub fn load_all(&mut self) -> Result<Vec<nexus_kernel::PluginInfo>, PluginError> {
        let dirs = self.loader.scan()?;
        let mut infos = Vec::new();
        for dir in dirs {
            match self.loader.load(&dir) {
                Ok(info) => infos.push(info),
                Err(e) => {
                    tracing::warn!("failed to load plugin at {}: {e}", dir.display());
                }
            }
        }
        Ok(infos)
    }

    /// Load a single plugin from `plugin_dir`.
    ///
    /// # Errors
    /// Propagates errors from [`PluginLoader::load`].
    pub fn load(&mut self, plugin_dir: &std::path::Path) -> Result<nexus_kernel::PluginInfo, PluginError> {
        self.loader.load(plugin_dir)
    }

    /// Unload the plugin identified by `plugin_id`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if no such plugin is loaded.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.loader.unload(plugin_id)
    }

    /// Return a snapshot of all currently-loaded plugins.
    #[must_use]
    pub fn list(&self) -> Vec<nexus_kernel::PluginInfo> {
        self.loader.list()
    }

    /// Look up a single plugin by ID.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<nexus_kernel::PluginInfo> {
        self.loader.get(plugin_id)
    }

    /// Dispatch a CLI subcommand call.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the subcommand is unknown.
    /// Propagates sandbox dispatch errors.
    pub fn dispatch_cli(&mut self, subcommand: &str, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_cli(subcommand, args)
    }

    /// Dispatch an IPC command call.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or command is
    /// unknown. Propagates sandbox dispatch errors.
    pub fn dispatch_ipc(&mut self, plugin_id: &str, command_id: &str, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_ipc(plugin_id, command_id, args)
    }

    /// Dispatch an IPC command call with capability verification.
    ///
    /// # Errors
    /// Returns [`PluginError::CapabilityDenied`] if `caller_plugin_id` lacks
    /// `IpcCall`, or [`PluginError::PluginNotFound`] if either plugin is unknown.
    pub fn dispatch_ipc_checked(
        &mut self,
        caller_plugin_id: &str,
        plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_ipc_checked(caller_plugin_id, plugin_id, command_id, args)
    }

    /// Load the settings for the plugin identified by `plugin_id`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded, or
    /// propagates settings I/O / validation errors.
    pub fn get_settings(&self, plugin_id: &str) -> Result<serde_json::Value, PluginError> {
        let plugin_dir = self
            .loader
            .plugin_dir(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?
            .to_path_buf();
        self.loader.settings().load_settings(plugin_id, &plugin_dir)
    }

    /// Persist `settings` for the plugin identified by `plugin_id` and notify
    /// it via `on_settings_changed` if declared.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded, or
    /// propagates settings validation / I/O errors.
    pub fn set_settings(&mut self, plugin_id: &str, settings: &serde_json::Value) -> Result<(), PluginError> {
        self.loader.update_settings(plugin_id, settings)
    }

    /// Enable the plugin identified by `plugin_id`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded.
    /// Propagates `on_enable` errors.
    pub fn enable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.loader.enable(plugin_id)
    }

    /// Disable the plugin identified by `plugin_id`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded.
    /// Propagates `on_disable` errors.
    pub fn disable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.loader.disable(plugin_id)
    }

    /// Inject the kernel event bus so that plugins can receive events.
    ///
    /// Must be called before loading plugins with event subscriptions.
    pub fn set_event_bus(&mut self, bus: std::sync::Arc<nexus_kernel::EventBus>) {
        self.loader.set_event_bus(bus);
    }

    /// Drain pending kernel events and dispatch them to subscribing plugins.
    ///
    /// Call this in your event loop (e.g. every tick or on each user interaction).
    ///
    /// # Errors
    /// Returns the first dispatch error encountered.
    pub fn poll_events(&mut self) -> Result<(), PluginError> {
        self.loader.poll_events()
    }

    /// Drain pending hot-reload events and reload the affected plugins.
    ///
    /// Returns the IDs of plugins that were successfully reloaded. Plugins that
    /// fail to reload are marked [`nexus_kernel::PluginStatus::Crashed`] and
    /// their IDs are **not** included in the returned list.
    ///
    /// If hot-reload is disabled this is a no-op that returns an empty `Vec`.
    ///
    /// # Errors
    /// This method does not currently propagate errors; reload failures are
    /// recorded on the plugin status and logged.
    pub fn poll_reloads(&mut self) -> Result<Vec<String>, PluginError> {
        let Some(ref reloader) = self.reloader else {
            return Ok(Vec::new());
        };

        let events = reloader.drain();
        let mut completed = Vec::new();

        for event in events {
            match self.reload_plugin(&event.plugin_id, &event.wasm_path) {
                Ok(()) => completed.push(event.plugin_id),
                Err(e) => {
                    tracing::warn!("hot-reload failed for {}: {e}", event.plugin_id);
                    self.loader.set_status(&event.plugin_id, nexus_kernel::PluginStatus::Crashed);
                }
            }
        }

        Ok(completed)
    }

    /// Unload all currently-loaded plugins in an orderly fashion.
    ///
    /// # Errors
    /// Returns the first error encountered, if any.
    pub fn shutdown(&mut self) -> Result<(), PluginError> {
        let ids: Vec<String> = self.loader.list().into_iter().map(|i| i.id).collect();
        for id in ids {
            if let Err(e) = self.loader.unload(&id) {
                tracing::warn!("shutdown: failed to unload {id}: {e}");
            }
        }
        Ok(())
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    fn reload_plugin(&mut self, plugin_id: &str, wasm_path: &std::path::Path) -> Result<(), PluginError> {
        // Call on_stop on the old sandbox (best-effort).
        if let Some(sandbox) = self.loader.sandbox_mut(plugin_id) {
            let _ = sandbox.call_on_stop();
        }

        // Read new WASM bytes.
        let wasm_bytes = std::fs::read(wasm_path)?;

        // Retrieve the manifest and plugin_dir from the loader.
        // Hot-reload is only triggered for community (WASM) plugins; core plugins
        // are never reloaded this way.
        let (wasm_config, lifecycle, plugin_data) = {
            let m = self
                .loader
                .manifest(plugin_id)
                .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
            let wasm_config = m.wasm.clone().ok_or_else(|| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: "hot-reload attempted on a core plugin — this should never happen"
                    .to_string(),
            })?;
            let lifecycle = m.lifecycle.clone();
            let pd = PluginData {
                plugin_id: plugin_id.to_string(),
                capabilities: self
                    .loader
                    .get(plugin_id)
                    .map_or_else(nexus_kernel::CapabilitySet::empty, |i| i.capabilities),
                ..Default::default()
            };
            (wasm_config, lifecycle, pd)
        };

        // Create new sandbox.
        let mut new_sandbox = WasmSandbox::new(&wasm_bytes, &wasm_config, plugin_data)
            .map_err(|e| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: e.to_string(),
            })?;

        // Call lifecycle hooks on new sandbox.
        if lifecycle.on_init {
            new_sandbox.call_on_init().map_err(|e| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: e.to_string(),
            })?;
        }
        if lifecycle.on_start {
            new_sandbox.call_on_start().map_err(|e| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: e.to_string(),
            })?;
        }

        // Replace sandbox and reset status.
        self.loader.replace_sandbox(plugin_id, new_sandbox);
        self.loader.set_status(plugin_id, nexus_kernel::PluginStatus::Running);

        Ok(())
    }
}

// ─── PluginManager integration tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_plugin(plugin_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(plugin_id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        let manifest = format!(
            r#"
[plugin]
id = "{plugin_id}"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "{plugin_id}.echo"
handler_id = 100
description = "Echo"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#
        );
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
        (tmp, plugin_dir)
    }

    fn no_reload_config() -> PluginManagerConfig {
        PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        }
    }

    #[test]
    fn manager_load_and_list() {
        let (tmp, plugin_dir) = setup_plugin("com.test.mgr.load");
        let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
        let info = mgr.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.mgr.load");

        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "com.test.mgr.load");
    }

    #[test]
    fn manager_dispatch_cli() {
        let (tmp, plugin_dir) = setup_plugin("com.test.mgr.dispatch");
        let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
        mgr.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"key": "value"});
        let result = mgr
            .dispatch_cli("com.test.mgr.dispatch.echo", &args)
            .unwrap();
        assert_eq!(result, args, "echo handler should return args unchanged");
    }

    #[test]
    fn manager_unload_and_shutdown() {
        let (tmp, plugin_dir) = setup_plugin("com.test.mgr.unload");
        let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
        mgr.load(&plugin_dir).unwrap();
        assert_eq!(mgr.list().len(), 1);

        mgr.unload("com.test.mgr.unload").unwrap();
        assert!(mgr.list().is_empty());

        // shutdown on already-empty manager should succeed
        mgr.shutdown().unwrap();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn manager_get_returns_info() {
        let (tmp, plugin_dir) = setup_plugin("com.test.mgr.get");
        let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
        mgr.load(&plugin_dir).unwrap();

        let info = mgr.get("com.test.mgr.get");
        assert!(info.is_some());
        assert_eq!(info.unwrap().id, "com.test.mgr.get");

        assert!(mgr.get("com.nonexistent").is_none());
    }
}
