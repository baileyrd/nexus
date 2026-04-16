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

use std::sync::{Arc, Mutex};

pub use error::PluginError;
pub use scaffold::{scaffold, PluginTemplate, ScaffoldConfig};
pub use loader::{CorePlugin, CorePluginFuture, PluginBackend, PluginLoader, SharedPluginLoader};
pub use manifest::{
    CliSubcommandReg, EventSubscriberReg, IpcCommandReg, LifecycleConfig, ManifestCapabilities,
    PanelSide, PluginManifest, Registrations, SettingsConfig, UiCommandReg, UiPanelReg,
    UiRibbonItemReg, UiSettingsTabReg, UiStatusItemReg, WasmConfig,
};
pub use manifest::{load_manifest, parse_manifest, validate};
pub use sandbox::{PluginData, PluginEventForwarder, WasmSandbox};
pub use settings::SettingsManager;
pub use hot_reload::{HotReloader, ReloadEvent};
pub use nexus_kernel::{PluginInfo, PluginStatus, TrustLevel};

// ─── UiContribution ───────────────────────────────────────────────────────────

/// A single plugin-contributed command palette entry, materialised for the
/// frontend.
///
/// Aggregated by [`PluginManager::ui_contributions`] across every loaded
/// plugin's `[[registrations.ui_command]]` entries.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiContribution {
    /// The plugin that owns this contribution. Used for dispatch routing.
    pub plugin_id: String,
    /// The `id` declared in the manifest's `ui_command` entry. Passed back to
    /// [`PluginManager::dispatch_ipc`] when the command is invoked.
    pub command_id: String,
    /// Primary label shown in the command palette.
    pub title: String,
    /// Optional category badge.
    pub category: Option<String>,
    /// Optional Lucide icon name.
    pub icon: Option<String>,
    /// Optional default keybinding — a `+`-separated chord parsed and
    /// dispatched on the frontend (e.g. `"Mod+Shift+H"`).
    pub keybinding: Option<String>,
}

/// A single plugin-contributed side panel, materialised for the frontend.
///
/// Aggregated by [`PluginManager::ui_panels`] across every loaded
/// plugin's `[[registrations.ui_panel]]` entries. The frontend merges
/// these into the active workspace layout at render time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiPanelContribution {
    /// The plugin that owns this panel.
    pub plugin_id: String,
    /// The `id` declared in the manifest's `ui_panel` entry.
    pub panel_id: String,
    /// Panel title shown in the side-panel selector.
    pub title: String,
    /// Lucide icon name.
    pub icon: String,
    /// `"left"` or `"right"` — which side panel to dock into.
    pub side: String,
}

/// A single plugin-contributed Settings-modal tab, materialised for
/// the frontend.
///
/// Aggregated by [`PluginManager::ui_settings_tabs`] across every
/// loaded plugin's `[[registrations.ui_settings_tab]]` entries. The
/// frontend renders one entry per row under the Settings modal's
/// "Plugins" rail group.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiSettingsTabContribution {
    /// The plugin that owns this tab.
    pub plugin_id: String,
    /// Human-readable plugin name (pulled from the manifest for the
    /// auto-generated tab header).
    pub plugin_name: String,
    /// Plugin version string, used in the auto-generated header.
    pub plugin_version: String,
    /// The `id` declared in the manifest's `ui_settings_tab` entry.
    pub tab_id: String,
    /// Title shown in the rail entry.
    pub title: String,
    /// Lucide icon name.
    pub icon: String,
}

/// A single plugin-contributed workspace-ribbon icon, materialised
/// for the frontend.
///
/// Aggregated by [`PluginManager::ui_ribbon_items`] across every
/// loaded plugin's `[[registrations.ui_ribbon_item]]` entries. The
/// frontend merges these into the active layout's ribbon at render
/// time; clicking dispatches the referenced plugin command via the
/// contribution registry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiRibbonItemContribution {
    /// The plugin that owns this ribbon entry.
    pub plugin_id: String,
    /// The `id` declared in the manifest.
    pub ribbon_id: String,
    /// Lucide icon name for the ribbon button.
    pub icon: String,
    /// Hover tooltip and accessible label.
    pub tooltip: String,
    /// Fully-qualified command id to dispatch on click
    /// (`plugin:<plugin_id>:<command_id>`). Pre-resolved server-side
    /// so the frontend doesn't reconstruct it.
    pub command_id: String,
}

/// A single plugin-contributed status-bar entry, materialised for the
/// frontend. Aggregated by [`PluginManager::ui_status_items`]; the
/// frontend merges these into the active layout's status-bar array.
/// `command_id` is `Some(fully-qualified)` for interactive entries,
/// `None` for plain counters.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UiStatusItemContribution {
    /// The plugin that owns this status-bar entry.
    pub plugin_id: String,
    /// The `id` declared in the manifest.
    pub status_id: String,
    /// Text shown alongside the icon. `None` for icon-only.
    pub text: Option<String>,
    /// Lucide icon name. `None` for text-only.
    pub icon: Option<String>,
    /// Hover tooltip; falls back to `text` frontend-side if unset.
    pub tooltip: Option<String>,
    /// Fully-qualified command id to dispatch on click
    /// (`plugin:<plugin_id>:<command>`), or `None` for non-interactive.
    pub command_id: Option<String>,
}

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

    /// Aggregate UI command contributions across all currently-loaded plugins.
    ///
    /// Returns one [`UiContribution`] per `[[registrations.ui_command]]` entry
    /// declared in each plugin's manifest. The frontend consumes this list to
    /// populate the command palette with plugin-contributed entries.
    #[must_use]
    pub fn ui_contributions(&self) -> Vec<UiContribution> {
        self.loader
            .list()
            .into_iter()
            .flat_map(|info| {
                let plugin_id = info.id.clone();
                self.loader
                    .manifest(&info.id)
                    .map(|m| m.registrations.ui_commands.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |r| UiContribution {
                        plugin_id: plugin_id.clone(),
                        command_id: r.id,
                        title: r.title,
                        category: r.category,
                        icon: r.icon,
                        keybinding: r.keybinding,
                    })
            })
            .collect()
    }

    /// Aggregate UI side-panel contributions across all currently-loaded
    /// plugins. Returns one [`UiPanelContribution`] per
    /// `[[registrations.ui_panel]]` entry; the frontend merges these
    /// into the active workspace layout at render time.
    #[must_use]
    pub fn ui_panels(&self) -> Vec<UiPanelContribution> {
        self.loader
            .list()
            .into_iter()
            .flat_map(|info| {
                let plugin_id = info.id.clone();
                self.loader
                    .manifest(&info.id)
                    .map(|m| m.registrations.ui_panels.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |r| UiPanelContribution {
                        plugin_id: plugin_id.clone(),
                        panel_id: r.id,
                        title: r.title,
                        icon: r.icon,
                        side: match r.side {
                            PanelSide::Left => "left".to_string(),
                            PanelSide::Right => "right".to_string(),
                        },
                    })
            })
            .collect()
    }

    /// Aggregate Settings-modal tab contributions across all
    /// currently-loaded plugins. Returns one
    /// [`UiSettingsTabContribution`] per `[[registrations.ui_settings_tab]]`
    /// entry; the frontend renders one row per tab in the Settings
    /// modal's "Plugins" rail group.
    #[must_use]
    pub fn ui_settings_tabs(&self) -> Vec<UiSettingsTabContribution> {
        self.loader
            .list()
            .into_iter()
            .flat_map(|info| {
                let plugin_id = info.id.clone();
                let plugin_name = info.name.clone();
                let plugin_version = info.version.clone();
                self.loader
                    .manifest(&info.id)
                    .map(|m| m.registrations.ui_settings_tabs.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |r| UiSettingsTabContribution {
                        plugin_id: plugin_id.clone(),
                        plugin_name: plugin_name.clone(),
                        plugin_version: plugin_version.clone(),
                        tab_id: r.id,
                        title: r.title,
                        icon: r.icon,
                    })
            })
            .collect()
    }

    /// Aggregate workspace-ribbon icon contributions across all
    /// currently-loaded plugins. `command_id` is pre-qualified with the
    /// owning plugin so the frontend can pass it straight to
    /// `contributions.invokeCommand`.
    #[must_use]
    pub fn ui_ribbon_items(&self) -> Vec<UiRibbonItemContribution> {
        self.loader
            .list()
            .into_iter()
            .flat_map(|info| {
                let plugin_id = info.id.clone();
                self.loader
                    .manifest(&info.id)
                    .map(|m| m.registrations.ui_ribbon_items.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |r| UiRibbonItemContribution {
                        plugin_id: plugin_id.clone(),
                        ribbon_id: r.id,
                        icon: r.icon,
                        tooltip: r.tooltip,
                        command_id: format!("plugin:{plugin_id}:{}", r.command),
                    })
            })
            .collect()
    }

    /// Aggregate status-bar entry contributions across all
    /// currently-loaded plugins. `command_id` is pre-qualified
    /// (`plugin:<plugin_id>:<command>`) when set, `None` for
    /// non-interactive counters.
    #[must_use]
    pub fn ui_status_items(&self) -> Vec<UiStatusItemContribution> {
        self.loader
            .list()
            .into_iter()
            .flat_map(|info| {
                let plugin_id = info.id.clone();
                self.loader
                    .manifest(&info.id)
                    .map(|m| m.registrations.ui_status_items.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |r| UiStatusItemContribution {
                        plugin_id: plugin_id.clone(),
                        status_id: r.id,
                        text: r.text,
                        icon: r.icon,
                        tooltip: r.tooltip,
                        command_id: r
                            .command
                            .map(|c| format!("plugin:{plugin_id}:{c}")),
                    })
            })
            .collect()
    }

    /// Dispatch a CLI subcommand call.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the subcommand is unknown.
    /// Propagates sandbox dispatch errors.
    pub fn dispatch_cli(&self, subcommand: &str, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_cli(subcommand, args)
    }

    /// Dispatch an IPC command call.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or command is
    /// unknown. Propagates sandbox dispatch errors.
    pub fn dispatch_ipc(&self, plugin_id: &str, command_id: &str, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_ipc(plugin_id, command_id, args)
    }

    /// Dispatch an IPC command call with capability verification.
    ///
    /// # Errors
    /// Returns [`PluginError::CapabilityDenied`] if `caller_plugin_id` lacks
    /// `IpcCall`, or [`PluginError::PluginNotFound`] if either plugin is unknown.
    pub fn dispatch_ipc_checked(
        &self,
        caller_plugin_id: &str,
        plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_ipc_checked(caller_plugin_id, plugin_id, command_id, args)
    }

    /// Resolve a plugin IPC target without dispatching. Returns the
    /// backend handle and handler_id so callers can release locks before
    /// executing.
    pub fn resolve_ipc(
        &self,
        plugin_id: &str,
        command_id: &str,
    ) -> Result<(Arc<Mutex<loader::PluginBackend>>, u32), PluginError> {
        self.loader.resolve_ipc(plugin_id, command_id)
    }

    /// Inject an [`IpcDispatcher`] into all loaded community plugins.
    pub fn inject_ipc_dispatcher(&mut self, dispatcher: Arc<dyn nexus_kernel::IpcDispatcher>) {
        self.loader.inject_ipc_dispatcher(dispatcher);
    }

    /// Inject a [`PluginEventForwarder`] into all loaded community
    /// plugins so `host::emit_event` calls are surfaced to the
    /// application layer.
    pub fn inject_event_forwarder(&mut self, forwarder: Arc<dyn sandbox::PluginEventForwarder>) {
        self.loader.inject_event_forwarder(forwarder);
    }

    /// Return the event subscriptions for `plugin_id`.
    #[must_use]
    pub fn event_subscriptions(&self, plugin_id: &str) -> Vec<(String, String, bool)> {
        self.loader.event_subscriptions(plugin_id)
    }

    /// Enable or disable an event subscription.
    ///
    /// # Errors
    /// See [`PluginLoader::toggle_event_subscription`].
    pub fn toggle_event_subscription(
        &mut self,
        plugin_id: &str,
        subscription_id: &str,
        enabled: bool,
    ) -> Result<(), PluginError> {
        self.loader.toggle_event_subscription(plugin_id, subscription_id, enabled)
    }

    /// Return the raw JSON Schema declared by `plugin_id`, or `None`
    /// if the plugin either isn't loaded or doesn't declare a
    /// `[settings]` block. The frontend uses this to render a form.
    #[must_use]
    pub fn get_settings_schema(&self, plugin_id: &str) -> Option<serde_json::Value> {
        self.loader.settings().schema(plugin_id).cloned()
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
    /// Returns a `Vec<(plugin_id, response)>` of every handler response produced
    /// by this drain so the caller can surface any `{events: [...]}` side
    /// channels back to the frontend.
    ///
    /// # Errors
    /// Returns the first dispatch error encountered.
    pub fn poll_events(&mut self) -> Result<Vec<(String, serde_json::Value)>, PluginError> {
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
        if let Some(backend) = self.loader.backend_arc(plugin_id) {
            if let Ok(mut guard) = backend.lock() {
                let _ = guard.call_on_stop();
            }
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
                settings_json: self.loader.settings_cache(plugin_id),
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
