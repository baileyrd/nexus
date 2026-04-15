//! Plugin loader: scans a directory for plugins, loads WASM sandboxes for
//! community plugins, registers native Rust handlers for core plugins,
//! manages plugin lifecycle, and dispatches kernel events to subscriber plugins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::sync::Mutex;

use std::future::Future;
use std::pin::Pin;

use nexus_kernel::{
    Capability, CapabilitySet, EventBus, EventFilter, EventSubscription, IpcDispatcher, IpcError,
    IpcFuture, KernelPluginContext, PluginInfo, PluginStatus, TrustLevel,
};

use crate::manifest::{self, PluginManifest};
use crate::sandbox::{PluginData, WasmSandbox};
use crate::settings::SettingsManager;
use crate::PluginError;

// ─── CorePlugin trait ─────────────────────────────────────────────────────────

/// Native Rust interface for core plugins.
///
/// Core plugins are compiled into the Nexus binary (or linked as a dylib) and
/// have unrestricted access to kernel internals. They implement this trait
/// directly in Rust rather than compiling to WASM.
///
/// Register an implementation with [`PluginLoader::register_core`].
pub trait CorePlugin: Send + Sync {
    /// Called when the plugin is initialised (after dependencies are ready).
    ///
    /// # Errors
    /// Return [`PluginError`] to abort plugin startup.
    fn on_init(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    /// Called when the plugin transitions to the Started state.
    ///
    /// # Errors
    /// Return [`PluginError`] to abort plugin startup.
    fn on_start(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    /// Called on graceful shutdown.
    fn on_stop(&mut self) {}
    /// Called when the plugin is enabled after being disabled.
    ///
    /// # Errors
    /// Return [`PluginError`] on failure.
    fn on_enable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    /// Called when the plugin is disabled.
    fn on_disable(&mut self) {}
    /// Called after the user updates this plugin's settings.
    ///
    /// # Errors
    /// Return [`PluginError`] on failure.
    fn on_settings_changed(&mut self, _settings: &serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }
    /// Dispatch a handler call identified by `handler_id` with JSON `args`.
    ///
    /// `handler_id` values correspond to those declared in the plugin manifest's
    /// `[registrations]` sections (same numbering as the WASM ABI).
    ///
    /// # Errors
    /// Return [`PluginError`] on dispatch failure.
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError>;

    /// Async dispatch path for handlers that perform HTTP calls, nested
    /// `ipc_call`s, or other `.await`-bound work.
    ///
    /// Returns `Some(future)` when this plugin has an async handler for
    /// `handler_id`; returns `None` (the default) when it is sync-only and
    /// the caller should fall back to [`dispatch`](CorePlugin::dispatch).
    ///
    /// Implementors must capture any state the future needs by value — the
    /// returned future outlives the borrow on `self`.
    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        let _ = (handler_id, args);
        None
    }

    /// Called once by the bootstrap after all core plugins are registered and
    /// the shared [`IpcDispatcher`] is assembled, handing the plugin its own
    /// [`KernelPluginContext`]. Plugins that need to issue nested `ipc_call`s
    /// (e.g. an AI plugin calling storage for vector search) should capture
    /// the context here so async handlers can clone it into their futures.
    ///
    /// Default impl is a no-op — plugins that never initiate IPC can ignore
    /// this hook entirely. Registration / `on_init` happens BEFORE this call,
    /// so anything that uses the context must defer to `dispatch_async`.
    fn wire_context(&mut self, _ctx: Arc<KernelPluginContext>) {}
}

/// Boxed future returned by [`CorePlugin::dispatch_async`].
///
/// Mirrors [`nexus_kernel::IpcFuture`] but carries the crate-native
/// [`PluginError`]; the loader converts to [`IpcError`] before handing the
/// future back to the kernel.
pub type CorePluginFuture = Pin<Box<dyn Future<Output = Result<serde_json::Value, PluginError>> + Send>>;

// ─── PluginBackend ────────────────────────────────────────────────────────────

/// The runtime backend for a loaded plugin.
///
/// - `Core` — native Rust; no sandbox overhead, unrestricted kernel access.
/// - `Community` — WASM-sandboxed; capability-gated, fuel-metered.
enum PluginBackend {
    Core(Box<dyn CorePlugin>),
    Community(WasmSandbox),
}

impl PluginBackend {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match self {
            Self::Core(p) => p.dispatch(handler_id, args),
            Self::Community(s) => s.dispatch(handler_id, args),
        }
    }

    /// Async dispatch: delegates to the core plugin's `dispatch_async`. WASM
    /// sandboxes never produce a future today, so community plugins always
    /// return `None` and fall back to sync dispatch.
    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        match self {
            Self::Core(p) => p.dispatch_async(handler_id, args),
            Self::Community(_) => None,
        }
    }

    fn call_on_init(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_init(),
            Self::Community(s) => s.call_on_init(),
        }
    }

    fn call_on_start(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_start(),
            Self::Community(s) => s.call_on_start(),
        }
    }

    fn call_on_stop(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => {
                p.on_stop();
                Ok(())
            }
            Self::Community(s) => s.call_on_stop(),
        }
    }

    fn call_on_load(&mut self) -> Result<(), PluginError> {
        match self {
            // Core plugins have no separate on_load; on_init serves that role.
            Self::Core(_) => Ok(()),
            Self::Community(s) => s.call_on_load(),
        }
    }

    fn call_on_unload(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(_) => Ok(()),
            Self::Community(s) => s.call_on_unload(),
        }
    }

    /// Hand the plugin its [`KernelPluginContext`]. Core-only; WASM sandboxes
    /// receive their runtime state through a different mechanism.
    fn call_wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        if let Self::Core(p) = self {
            p.wire_context(ctx);
        }
    }

    fn call_on_enable(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_enable(),
            Self::Community(s) => s.call_on_enable(),
        }
    }

    fn call_on_disable(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => {
                p.on_disable();
                Ok(())
            }
            Self::Community(s) => s.call_on_disable(),
        }
    }

    fn call_on_settings_changed(
        &mut self,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_settings_changed(settings),
            Self::Community(s) => s.call_on_settings_changed(settings),
        }
    }

    /// Returns a mutable reference to the inner `WasmSandbox`, if community.
    fn as_wasm_sandbox_mut(&mut self) -> Option<&mut WasmSandbox> {
        match self {
            Self::Community(s) => Some(s),
            Self::Core(_) => None,
        }
    }
}

// ─── Internal structs ─────────────────────────────────────────────────────────

struct PluginRegistrations {
    cli_subcommands: Vec<String>,
    #[allow(dead_code)]
    ipc_commands: Vec<String>,
}

/// A live event subscription wired to a plugin handler.
struct PluginEventSub {
    /// Handler invoked when a matching event arrives.
    handler_id: u32,
    /// Live subscription handle (dropped → auto-unsubscribe).
    subscription: EventSubscription,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    backend: PluginBackend,
    capabilities: CapabilitySet,
    status: PluginStatus,
    plugin_dir: PathBuf,
    registrations: PluginRegistrations,
    /// Active event subscriptions for this plugin.
    event_subs: Vec<PluginEventSub>,
}

// ─── PluginLoader ─────────────────────────────────────────────────────────────

/// Manages loading, unloading, and dispatching to WASM plugins.
///
/// Maintains a registry of loaded plugins keyed by their manifest ID, and a
/// separate CLI-dispatch table mapping subcommand IDs to plugin IDs.
pub struct PluginLoader {
    plugins_dir: PathBuf,
    loaded: HashMap<String, LoadedPlugin>,
    /// Maps `subcommand_id` → `plugin_id`
    cli_registry: HashMap<String, String>,
    settings: SettingsManager,
    /// Optional reference to the kernel event bus; set via [`set_event_bus`].
    ///
    /// [`set_event_bus`]: PluginLoader::set_event_bus
    event_bus: Option<Arc<EventBus>>,
}

impl PluginLoader {
    /// Create a new, empty `PluginLoader` rooted at `plugins_dir`.
    ///
    /// No plugins are loaded at construction time; call [`scan`](Self::scan)
    /// and [`load`](Self::load) to populate the loader.
    #[must_use]
    pub fn new(plugins_dir: &Path) -> Self {
        Self {
            plugins_dir: plugins_dir.to_path_buf(),
            loaded: HashMap::new(),
            cli_registry: HashMap::new(),
            settings: SettingsManager::new(),
            event_bus: None,
        }
    }

    /// Inject the kernel event bus.
    ///
    /// Must be called before loading plugins that declare event subscriptions.
    /// Already-loaded plugins will not retroactively subscribe; reload them to
    /// pick up the new bus.
    pub fn set_event_bus(&mut self, bus: Arc<EventBus>) {
        self.event_bus = Some(bus);
    }

    /// Walk `plugins_dir` and return the paths of subdirectories that contain
    /// a `manifest.toml` file.
    ///
    /// # Errors
    /// Returns [`PluginError::Io`] if the plugins directory cannot be read.
    pub fn scan(&self) -> Result<Vec<PathBuf>, PluginError> {
        let mut found = Vec::new();

        let read_dir = match std::fs::read_dir(&self.plugins_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
            Err(e) => return Err(PluginError::Io(e)),
        };

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("manifest.toml").exists() {
                found.push(path);
            }
        }

        Ok(found)
    }

    /// Load a **community** plugin from `plugin_dir`.
    ///
    /// Community plugins are WASM-sandboxed and distributed as `.wasm` files.
    /// The plugin directory must contain `manifest.toml` with
    /// `trust_level = "community"` and a `[wasm]` section.
    ///
    /// For **core** plugins (native Rust), use [`register_core`] instead.
    ///
    /// [`register_core`]: PluginLoader::register_core
    ///
    /// # Steps
    /// 1. Parse `manifest.toml` via [`manifest::load_manifest`].
    /// 2. Validate the manifest via [`manifest::validate`].
    /// 3. Reject non-community manifests (core plugins must use `register_core`).
    /// 4. Reject duplicate plugin IDs.
    /// 5. Register settings schema with [`SettingsManager`] if declared.
    /// 6. Read the WASM bytes and create a [`WasmSandbox`].
    /// 7. Call `on_load` / `on_init` / `on_start` lifecycle hooks if declared.
    /// 8. Register CLI subcommands, rejecting conflicts.
    /// 9. Set status to [`PluginStatus::Running`] and return [`PluginInfo`].
    ///
    /// # Errors
    /// Returns [`PluginError`] on any failure.
    pub fn load(&mut self, plugin_dir: &Path) -> Result<PluginInfo, PluginError> {
        // Step 1: Parse manifest
        let manifest_path = plugin_dir.join("manifest.toml");
        let manifest = manifest::load_manifest(&manifest_path)?;

        // Step 2: Validate
        manifest::validate(&manifest, plugin_dir)?;

        // Step 3: Reject core plugins — they must use register_core()
        if manifest.trust_level == TrustLevel::Core {
            return Err(PluginError::ManifestInvalid {
                path: manifest_path.display().to_string(),
                reason: "core plugins must be registered via PluginLoader::register_core(), \
                         not loaded from a WASM directory"
                    .to_string(),
            });
        }

        // Step 4: Reject duplicate plugin ID
        let plugin_id = manifest.id.clone();
        if self.loaded.contains_key(&plugin_id) {
            return Err(PluginError::DuplicatePlugin(plugin_id));
        }

        // Step 5: Register settings schema if declared
        if let Some(ref settings_cfg) = manifest.settings {
            let schema_path = plugin_dir.join(&settings_cfg.schema);
            let schema_json = std::fs::read_to_string(&schema_path)?;
            self.settings.register_schema(&plugin_id, &schema_json)?;
        }

        // Step 6: Read WASM bytes and build sandbox.
        // wasm is guaranteed Some for community plugins (validated in step 2).
        let wasm_cfg = manifest.wasm.as_ref().ok_or_else(|| PluginError::ManifestInvalid {
            path: manifest_path.display().to_string(),
            reason: "internal: community plugin passed validation without a [wasm] section"
                .to_string(),
        })?;
        let wasm_path = plugin_dir.join(&wasm_cfg.module);
        let wasm_bytes = std::fs::read(&wasm_path)?;

        let capabilities = build_capabilities(&manifest);
        let plugin_data = PluginData {
            plugin_id: plugin_id.clone(),
            capabilities: capabilities.clone(),
            forge_root: plugin_dir.to_path_buf(),
            ..Default::default()
        };
        let mut backend = PluginBackend::Community(
            WasmSandbox::new(&wasm_bytes, wasm_cfg, plugin_data)?,
        );

        // Step 7: Call lifecycle hooks
        if manifest.lifecycle.on_load {
            backend.call_on_load()?;
        }
        if manifest.lifecycle.on_init {
            backend.call_on_init()?;
        }
        if manifest.lifecycle.on_start {
            backend.call_on_start()?;
        }

        self.finish_loading(manifest, backend, capabilities, plugin_dir)
    }

    /// Register a **core** plugin backed by a native Rust implementation.
    ///
    /// Core plugins are compiled into the Nexus binary and have unrestricted
    /// kernel access. They are not sandboxed and do not run through the WASM
    /// runtime.
    ///
    /// The `manifest` must have `trust_level = "core"` and no `[wasm]` section.
    /// Pass the plugin directory (where `plugin.toml` and optional
    /// `settings.json` live) so that settings schema loading and error messages
    /// work correctly.
    ///
    /// `on_init` and `on_start` are called on `plugin` during registration if
    /// declared in the manifest lifecycle flags.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the manifest is invalid, the plugin ID is
    /// already registered, or a lifecycle hook returns an error.
    pub fn register_core(
        &mut self,
        manifest: PluginManifest,
        plugin_dir: &Path,
        mut plugin: Box<dyn CorePlugin>,
    ) -> Result<PluginInfo, PluginError> {
        // Validate the manifest (ensures trust_level=core, no [wasm] section, etc.).
        manifest::validate(&manifest, plugin_dir)?;

        if manifest.trust_level != TrustLevel::Core {
            return Err(PluginError::ManifestInvalid {
                path: plugin_dir.join("plugin.toml").display().to_string(),
                reason: "register_core requires trust_level = 'core'".to_string(),
            });
        }

        let plugin_id = manifest.id.clone();
        if self.loaded.contains_key(&plugin_id) {
            return Err(PluginError::DuplicatePlugin(plugin_id));
        }

        // Register settings schema if declared
        if let Some(ref settings_cfg) = manifest.settings {
            let schema_path = plugin_dir.join(&settings_cfg.schema);
            let schema_json = std::fs::read_to_string(&schema_path)?;
            self.settings.register_schema(&plugin_id, &schema_json)?;
        }

        // Call lifecycle hooks directly on the native implementation.
        if manifest.lifecycle.on_init {
            plugin.on_init()?;
        }
        if manifest.lifecycle.on_start {
            plugin.on_start()?;
        }

        let capabilities = build_capabilities(&manifest);
        let backend = PluginBackend::Core(plugin);
        self.finish_loading(manifest, backend, capabilities, plugin_dir)
    }

    /// Shared final step: register CLI/IPC, wire event subscriptions, insert.
    fn finish_loading(
        &mut self,
        manifest: PluginManifest,
        backend: PluginBackend,
        capabilities: CapabilitySet,
        plugin_dir: &Path,
    ) -> Result<PluginInfo, PluginError> {
        let plugin_id = manifest.id.clone();

        // Register CLI subcommands (reject duplicates)
        for sub in &manifest.registrations.cli_subcommands {
            if let Some(existing_plugin) = self.cli_registry.get(&sub.id) {
                return Err(PluginError::DuplicateCliSubcommand {
                    plugin_id: plugin_id.clone(),
                    subcommand: format!(
                        "{} (already registered by {})",
                        sub.id, existing_plugin
                    ),
                });
            }
        }
        let mut registered_cli: Vec<String> = Vec::new();
        for sub in &manifest.registrations.cli_subcommands {
            self.cli_registry.insert(sub.id.clone(), plugin_id.clone());
            registered_cli.push(sub.id.clone());
        }

        let registered_ipc: Vec<String> = manifest
            .registrations
            .ipc_commands
            .iter()
            .map(|r| r.id.clone())
            .collect();

        // Wire event subscriptions to the kernel bus (if available).
        let event_subs: Vec<PluginEventSub> = if let Some(ref bus) = self.event_bus {
            manifest
                .registrations
                .event_subscribers
                .iter()
                .map(|reg| PluginEventSub {
                    handler_id: reg.handler_id,
                    subscription: bus.subscribe(parse_event_filter(&reg.filter)),
                })
                .collect()
        } else {
            Vec::new()
        };

        let info = plugin_info_from(&manifest, PluginStatus::Running, &capabilities);

        self.loaded.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                backend,
                capabilities,
                status: PluginStatus::Running,
                plugin_dir: plugin_dir.to_path_buf(),
                registrations: PluginRegistrations {
                    cli_subcommands: registered_cli,
                    ipc_commands: registered_ipc,
                },
                event_subs,
            },
        );

        Ok(info)
    }

    /// Unload the plugin with the given `plugin_id`.
    ///
    /// If the plugin declared `on_stop`, it is called best-effort (errors are
    /// ignored so the plugin is always removed).
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if no plugin with `plugin_id`
    /// is loaded.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let mut loaded = self
            .loaded
            .remove(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        // Best-effort on_stop then on_unload — plugin is removed regardless of errors.
        if loaded.manifest.lifecycle.on_stop {
            let _ = loaded.backend.call_on_stop();
        }
        if loaded.manifest.lifecycle.on_unload {
            let _ = loaded.backend.call_on_unload();
        }

        // Deregister CLI subcommands
        for sub_id in &loaded.registrations.cli_subcommands {
            self.cli_registry.remove(sub_id);
        }

        Ok(())
    }

    /// Return a snapshot of all currently-loaded plugins.
    #[must_use]
    pub fn list(&self) -> Vec<PluginInfo> {
        self.loaded
            .values()
            .map(|lp| plugin_info_from(&lp.manifest, lp.status, &lp.capabilities))
            .collect()
    }

    /// Look up a single plugin by ID, returning a [`PluginInfo`] snapshot.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.loaded
            .get(plugin_id)
            .map(|lp| plugin_info_from(&lp.manifest, lp.status, &lp.capabilities))
    }

    /// Dispatch a CLI subcommand call.
    ///
    /// Looks up the plugin from the CLI registry by `subcommand`, finds the
    /// handler ID from the manifest, and calls `sandbox.dispatch`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the subcommand is unknown.
    /// Propagates sandbox dispatch errors.
    pub fn dispatch_cli(
        &mut self,
        subcommand: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_id = self
            .cli_registry
            .get(subcommand)
            .cloned()
            .ok_or_else(|| PluginError::PluginNotFound(subcommand.to_string()))?;

        let lp = self
            .loaded
            .get_mut(&plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.clone()))?;

        let handler_id = lp
            .manifest
            .registrations
            .cli_subcommands
            .iter()
            .find(|r| r.id == subcommand)
            .map(|r| r.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(subcommand.to_string()))?;

        lp.backend.dispatch(handler_id, args)
    }

    /// Dispatch an IPC command call with capability verification.
    ///
    /// Like [`dispatch_ipc`](Self::dispatch_ipc) but first checks that
    /// `caller_plugin_id` holds the [`Capability::IpcCall`] capability.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the caller or target is
    /// unknown, or [`PluginError::CapabilityDenied`] if the caller lacks
    /// `IpcCall`.
    pub fn dispatch_ipc_checked(
        &mut self,
        caller_plugin_id: &str,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // Verify calling plugin exists and has IpcCall capability.
        let caller_has_cap = self
            .loaded
            .get(caller_plugin_id)
            .map(|lp| lp.capabilities.contains(Capability::IpcCall))
            .ok_or_else(|| PluginError::PluginNotFound(caller_plugin_id.to_string()))?;

        if !caller_has_cap {
            return Err(PluginError::CapabilityDenied {
                plugin_id: caller_plugin_id.to_string(),
                capability: "ipc.call".to_string(),
            });
        }

        self.dispatch_ipc(target_plugin_id, command_id, args)
    }

    /// Dispatch an IPC command call.
    ///
    /// Looks up the plugin by `plugin_id`, finds the handler ID for
    /// `command_id` in the manifest, and calls `sandbox.dispatch`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or command is
    /// unknown. Propagates sandbox dispatch errors.
    pub fn dispatch_ipc(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        let handler_id = lp
            .manifest
            .registrations
            .ipc_commands
            .iter()
            .find(|r| r.id == command_id)
            .map(|r| r.handler_id)
            .or_else(|| {
                lp.manifest
                    .registrations
                    .ui_commands
                    .iter()
                    .find(|r| r.id == command_id)
                    .map(|r| r.handler_id)
            })
            .or_else(|| {
                lp.manifest
                    .registrations
                    .ui_panels
                    .iter()
                    .find(|r| r.id == command_id)
                    .map(|r| r.handler_id)
            })
            .or_else(|| {
                lp.manifest
                    .registrations
                    .ui_settings_tabs
                    .iter()
                    .find(|r| r.id == command_id)
                    .map(|r| r.handler_id)
            })
            .ok_or_else(|| PluginError::PluginNotFound(command_id.to_string()))?;

        lp.backend.dispatch(handler_id, args)
    }

    /// Enable the plugin with `plugin_id`.
    ///
    /// Sets its status to [`PluginStatus::Running`] and calls `on_enable` if
    /// declared.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded.
    /// Propagates `on_enable` errors.
    pub fn enable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
        lp.status = PluginStatus::Running;
        if lp.manifest.lifecycle.on_enable {
            lp.backend.call_on_enable()?;
        }
        Ok(())
    }

    /// Disable the plugin with `plugin_id`.
    ///
    /// Sets its status to [`PluginStatus::Stopped`] and calls `on_disable` if
    /// declared.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded.
    /// Propagates `on_disable` errors.
    pub fn disable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
        lp.status = PluginStatus::Stopped;
        if lp.manifest.lifecycle.on_disable {
            lp.backend.call_on_disable()?;
        }
        Ok(())
    }

    /// Persist `settings` and notify the plugin via `on_settings_changed` if
    /// declared.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin is not loaded.
    /// Propagates settings I/O / validation errors and `on_settings_changed`
    /// errors.
    pub fn update_settings(
        &mut self,
        plugin_id: &str,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let plugin_dir = self
            .plugin_dir(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?
            .to_path_buf();
        self.settings.save_settings(plugin_id, &plugin_dir, settings)?;
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
        if lp.manifest.lifecycle.on_settings_changed {
            lp.backend.call_on_settings_changed(settings)?;
        }
        Ok(())
    }

    /// Hand a registered core plugin its [`KernelPluginContext`] so it can
    /// issue nested `ipc_call`s through the canonical plugin-facing surface.
    ///
    /// Typically invoked by bootstrap after all core plugins are registered
    /// and the shared dispatcher is constructed.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if `plugin_id` is not loaded.
    pub fn wire_context(
        &mut self,
        plugin_id: &str,
        ctx: Arc<KernelPluginContext>,
    ) -> Result<(), PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
        lp.backend.call_wire_context(ctx);
        Ok(())
    }

    /// Drain pending events from all plugin subscriptions and dispatch them to
    /// the appropriate plugin handlers (WASM or native).
    ///
    /// Call this in your event loop (e.g. every tick). Each call is synchronous
    /// and non-blocking; events that have not yet arrived are silently skipped.
    ///
    /// Returns a `Vec<(plugin_id, response)>` of every handler response that
    /// was produced during the drain. Callers (e.g. the Tauri host) can walk
    /// the responses for `{events: [...]}` side-channel arrays and re-emit
    /// them to the frontend, keeping host-initiated events symmetric with
    /// handler-initiated events.
    ///
    /// # Errors
    /// Returns the first dispatch error encountered; subscriptions that lag or
    /// are closed are silently skipped so they don't block other plugins.
    pub fn poll_events(
        &mut self,
    ) -> Result<Vec<(String, serde_json::Value)>, PluginError> {
        // Collect (plugin_id, handler_id, event_json) tuples to dispatch.
        let mut pending: Vec<(String, u32, serde_json::Value)> = Vec::new();

        for (plugin_id, lp) in &mut self.loaded {
            for sub in &mut lp.event_subs {
                // Drain until no more events or the subscription is lagged/closed.
                while let Ok(Some(evt)) = sub.subscription.try_recv() {
                    let payload = serde_json::to_value(&*evt)
                        .unwrap_or(serde_json::Value::Null);
                    pending.push((plugin_id.clone(), sub.handler_id, payload));
                }
            }
        }

        let mut responses = Vec::with_capacity(pending.len());
        for (plugin_id, handler_id, payload) in pending {
            if let Some(lp) = self.loaded.get_mut(&plugin_id) {
                let response = lp.backend.dispatch(handler_id, &payload).map_err(|e| {
                    tracing::warn!(plugin_id = %plugin_id, "event dispatch failed: {e}");
                    e
                })?;
                responses.push((plugin_id, response));
            }
        }

        Ok(responses)
    }

    /// Return a reference to the [`SettingsManager`].
    #[must_use]
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// Return the plugin directory for `plugin_id`, if it is loaded.
    #[must_use]
    pub fn plugin_dir(&self, plugin_id: &str) -> Option<&Path> {
        self.loaded
            .get(plugin_id)
            .map(|lp| lp.plugin_dir.as_path())
    }

    // ─── Internal helpers for hot-reload ──────────────────────────────────────

    /// Return a mutable reference to the [`WasmSandbox`] for `plugin_id`.
    ///
    /// Returns `None` if the plugin is not loaded or is a core (native) plugin.
    /// Hot-reload only applies to community WASM plugins.
    #[allow(dead_code)]
    pub(crate) fn sandbox_mut(&mut self, plugin_id: &str) -> Option<&mut WasmSandbox> {
        self.loaded
            .get_mut(plugin_id)
            .and_then(|lp| lp.backend.as_wasm_sandbox_mut())
    }

    /// Return a reference to the [`PluginManifest`] for `plugin_id`.
    #[allow(dead_code)]
    pub(crate) fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.loaded.get(plugin_id).map(|lp| &lp.manifest)
    }

    /// Update the [`PluginStatus`] for `plugin_id`.
    #[allow(dead_code)]
    pub(crate) fn set_status(&mut self, plugin_id: &str, status: PluginStatus) {
        if let Some(lp) = self.loaded.get_mut(plugin_id) {
            lp.status = status;
        }
    }

    /// Replace the [`WasmSandbox`] for `plugin_id` during hot-reload.
    ///
    /// Only valid for community WASM plugins. Returns `None` if the plugin
    /// is not loaded or is a core plugin.
    #[allow(dead_code)]
    pub(crate) fn replace_sandbox(
        &mut self,
        plugin_id: &str,
        sandbox: WasmSandbox,
    ) -> Option<WasmSandbox> {
        self.loaded.get_mut(plugin_id).and_then(|lp| {
            if let PluginBackend::Community(_) = &lp.backend {
                let old_backend =
                    std::mem::replace(&mut lp.backend, PluginBackend::Community(sandbox));
                // Extract the old WasmSandbox from the replaced backend.
                if let PluginBackend::Community(old) = old_backend {
                    Some(old)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a manifest filter string into an [`EventFilter`].
///
/// Rules:
/// - `"*"` or `""` → [`EventFilter::All`]
/// - ends with `".*"` → [`EventFilter::CustomPrefix`] (the prefix before `*`)
/// - otherwise → [`EventFilter::CustomExact`]
///
/// Note: [`EventFilter::Variant`] requires a `&'static str` so it cannot be
/// used for dynamically-loaded manifest filter strings.
fn parse_event_filter(filter: &str) -> EventFilter {
    match filter {
        "" | "*" => EventFilter::All,
        f if f.ends_with(".*") => {
            EventFilter::CustomPrefix(f[..f.len() - 1].to_string()) // strip "*", keep "."
        }
        f => EventFilter::CustomExact(f.to_string()),
    }
}

fn build_capabilities(manifest: &PluginManifest) -> CapabilitySet {
    match manifest.trust_level {
        TrustLevel::Core => CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        TrustLevel::Community => {
            let caps: Vec<Capability> = manifest
                .capabilities
                .required
                .iter()
                .chain(manifest.capabilities.optional.iter())
                .filter_map(|s| Capability::from_str(s).ok())
                .collect();
            CapabilitySet::from_iter(caps)
        }
    }
}

fn plugin_info_from(
    manifest: &PluginManifest,
    status: PluginStatus,
    capabilities: &CapabilitySet,
) -> PluginInfo {
    PluginInfo {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        trust_level: manifest.trust_level,
        status,
        capabilities: capabilities.clone(),
    }
}

// ─── IpcDispatcher impl ──────────────────────────────────────────────────────

/// Shared handle that lets a [`nexus_kernel::KernelPluginContext`] dispatch
/// IPC calls into a [`PluginLoader`].
///
/// `PluginLoader::dispatch_ipc` requires `&mut self`, so the loader has to
/// live behind interior mutability; this newtype wraps a [`Mutex`] and
/// implements [`IpcDispatcher`] on top.
///
/// Typical usage:
/// ```ignore
/// let loader = Arc::new(SharedPluginLoader::new(PluginLoader::new(dir)));
/// let dispatcher: Arc<dyn IpcDispatcher> = loader.clone();
/// // ... pass `dispatcher` into KernelPluginContext::new
/// ```
pub struct SharedPluginLoader(Mutex<PluginLoader>);

impl SharedPluginLoader {
    /// Wrap a loader for shared kernel access.
    #[must_use]
    pub fn new(loader: PluginLoader) -> Self {
        Self(Mutex::new(loader))
    }

    /// Acquire the loader lock; panics on poison.
    ///
    /// # Panics
    /// Panics if the inner mutex is poisoned by a previous panic.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, PluginLoader> {
        self.0.lock().expect("plugin loader mutex poisoned")
    }

    /// Convenience wrapper over [`PluginLoader::wire_context`] that handles
    /// the internal mutex.
    ///
    /// # Errors
    /// See [`PluginLoader::wire_context`].
    ///
    /// # Panics
    /// Panics if the inner mutex is poisoned.
    pub fn wire_context(
        &self,
        plugin_id: &str,
        ctx: Arc<KernelPluginContext>,
    ) -> Result<(), PluginError> {
        self.0
            .lock()
            .expect("plugin loader mutex poisoned")
            .wire_context(plugin_id, ctx)
    }
}

impl IpcDispatcher for SharedPluginLoader {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        let mut loader = self
            .0
            .lock()
            .map_err(|_| IpcError::PluginCrashedDuringCall {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })?;

        let lp = loader
            .loaded
            .get_mut(target_plugin_id)
            .ok_or_else(|| IpcError::PluginNotFound {
                plugin_id: target_plugin_id.to_string(),
            })?;

        let handler_id = lp
            .manifest
            .registrations
            .ipc_commands
            .iter()
            .find(|r| r.id == command_id)
            .map(|r| r.handler_id)
            .ok_or_else(|| IpcError::CommandNotFound {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })?;

        lp.backend
            .dispatch(handler_id, args)
            .map_err(|_| IpcError::PluginCrashedDuringCall {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })
    }

    /// Hand back an async future for `command_id`, if the target plugin has
    /// one. The loader mutex is held only long enough to look up the handler
    /// and construct the future — it is released before the future is
    /// awaited, so handlers may issue nested `ipc_call`s without deadlocking.
    fn dispatch_async(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
    ) -> Option<IpcFuture> {
        let target = target_plugin_id.to_string();
        let command = command_id.to_string();

        let inner: CorePluginFuture = {
            let mut loader = self.0.lock().ok()?;
            let lp = loader.loaded.get_mut(&target)?;
            let handler_id = lp
                .manifest
                .registrations
                .ipc_commands
                .iter()
                .find(|r| r.id == command)
                .map(|r| r.handler_id)?;
            lp.backend.dispatch_async(handler_id, &args)?
        };

        Some(Box::pin(async move {
            inner.await.map_err(|_| IpcError::PluginCrashedDuringCall {
                plugin_id: target,
                command,
            })
        }))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn plugins_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn new_loader_has_empty_state() {
        let dir = plugins_dir();
        let loader = PluginLoader::new(dir.path());
        assert!(loader.loaded.is_empty());
        assert!(loader.cli_registry.is_empty());
        assert!(loader.list().is_empty());
    }

    #[test]
    fn scan_empty_directory() {
        let dir = plugins_dir();
        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn scan_finds_plugin_dirs() {
        let dir = plugins_dir();
        let plugin_dir = dir.path().join("my.plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("manifest.toml"), b"").unwrap();

        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], plugin_dir);
    }

    #[test]
    fn scan_skips_dirs_without_manifest() {
        let dir = plugins_dir();
        // Dir without manifest.toml
        std::fs::create_dir_all(dir.path().join("no-manifest")).unwrap();
        // Dir with manifest.toml
        let with_manifest = dir.path().join("has-manifest");
        std::fs::create_dir_all(&with_manifest).unwrap();
        std::fs::write(with_manifest.join("manifest.toml"), b"").unwrap();

        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("has-manifest"));
    }

    #[test]
    fn unload_nonexistent_returns_error() {
        let dir = plugins_dir();
        let mut loader = PluginLoader::new(dir.path());
        let err = loader.unload("com.example.missing").unwrap_err();
        assert!(
            matches!(err, PluginError::PluginNotFound(_)),
            "expected PluginNotFound, got {err:?}"
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn setup_plugin_dir(plugin_id: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(plugin_id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // Copy WASM fixture
        let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        let manifest = format!(
            r#"
[plugin]
id = "{plugin_id}"
name = "Test Plugin"
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
description = "Echo command"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#
        );
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
        tmp
    }

    #[test]
    fn load_plugin_from_dir() {
        let tmp = setup_plugin_dir("com.test.load");
        let plugin_dir = tmp.path().join("com.test.load");
        let mut loader = PluginLoader::new(tmp.path());
        let info = loader.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.load");
        assert_eq!(info.status, PluginStatus::Running);
    }

    #[test]
    fn load_duplicate_plugin_fails() {
        let tmp = setup_plugin_dir("com.test.dupe");
        let plugin_dir = tmp.path().join("com.test.dupe");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let err = loader.load(&plugin_dir).unwrap_err();
        assert!(
            matches!(err, PluginError::DuplicatePlugin(_)),
            "expected DuplicatePlugin, got {err:?}"
        );
    }

    #[test]
    fn list_shows_loaded_plugins() {
        let tmp = setup_plugin_dir("com.test.list");
        let plugin_dir = tmp.path().join("com.test.list");
        let mut loader = PluginLoader::new(tmp.path());
        assert!(loader.list().is_empty());

        loader.load(&plugin_dir).unwrap();
        let list = loader.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "com.test.list");
    }

    #[test]
    fn unload_removes_plugin() {
        let tmp = setup_plugin_dir("com.test.unload");
        let plugin_dir = tmp.path().join("com.test.unload");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();
        assert_eq!(loader.list().len(), 1);

        loader.unload("com.test.unload").unwrap();
        assert!(loader.list().is_empty());
        assert!(loader.get("com.test.unload").is_none());
        // CLI subcommand should also be deregistered
        assert!(!loader.cli_registry.contains_key("com.test.unload.echo"));
    }

    #[test]
    fn dispatch_cli_to_loaded_plugin() {
        let tmp = setup_plugin_dir("com.test.dispatch");
        let plugin_dir = tmp.path().join("com.test.dispatch");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"hello": "world"});
        let result = loader
            .dispatch_cli("com.test.dispatch.echo", &args)
            .unwrap();
        assert_eq!(result, args, "echo handler should return args unchanged");
    }

    #[test]
    fn dispatch_cli_unknown_subcommand_fails() {
        let tmp = setup_plugin_dir("com.test.unknown");
        let plugin_dir = tmp.path().join("com.test.unknown");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let err = loader
            .dispatch_cli("com.test.unknown.nonexistent", &serde_json::json!({}))
            .unwrap_err();
        assert!(
            matches!(err, PluginError::PluginNotFound(_)),
            "expected PluginNotFound, got {err:?}"
        );
    }

    /// Writes a plugin dir whose manifest declares a `ui_command` bound to
    /// the echo handler (id 100) instead of a cli_subcommand.
    fn setup_ui_plugin_dir(plugin_id: &str) -> tempfile::TempDir {
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
name = "Test Plugin"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.ui_command]]
id = "{plugin_id}.hello"
handler_id = 100
title = "Say Hi"
"#
        );
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
        tmp
    }

    #[test]
    fn dispatch_ipc_resolves_ui_command_handler() {
        let tmp = setup_ui_plugin_dir("com.test.ui");
        let plugin_dir = tmp.path().join("com.test.ui");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"name": "nexus"});
        let result = loader
            .dispatch_ipc("com.test.ui", "com.test.ui.hello", &args)
            .unwrap();
        assert_eq!(result, args);
    }
}
