//! Plugin loader: scans a directory for plugins, loads WASM sandboxes for
//! community plugins, registers native Rust handlers for core plugins,
//! manages plugin lifecycle, and dispatches kernel events to subscriber plugins.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, RwLock};

use std::future::Future;
use std::pin::Pin;

// ─── Reentrancy detection ─────────────────────────────────────────────────────
//
// Sync IPC dispatch holds `Arc<Mutex<PluginBackend>>` across the handler call
// (handlers take `&mut self`). Using `Mutex::lock()` is correct for concurrent
// dispatches — they queue naturally — but a *true* reentrant call from within
// a handler on the same thread would deadlock.
//
// We track the set of plugin ids currently being dispatched on *this thread*
// in a thread-local. Entering dispatch for a plugin already in the set returns
// `ReentrantCall` instead of deadlocking. This preserves the reentrancy
// guarantee without conflating it with ordinary contention.
//
// Only the sync path (`spawn_blocking` body / direct `dispatch_ipc` callers)
// needs this guard: async handlers never hold the backend mutex across
// awaits, so nested async ipc_calls are safe by construction.

thread_local! {
    static ACTIVE_DISPATCHES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard that registers `plugin_id` as being dispatched on the current
/// thread. Returns `None` if the plugin is already on the stack for this
/// thread — the caller must treat that as a reentrant call.
struct DispatchGuard {
    plugin_id: String,
}

impl DispatchGuard {
    fn enter(plugin_id: &str) -> Option<Self> {
        ACTIVE_DISPATCHES.with(|s| {
            let mut stack = s.borrow_mut();
            if stack.iter().any(|id| id == plugin_id) {
                return None;
            }
            stack.push(plugin_id.to_string());
            Some(Self {
                plugin_id: plugin_id.to_string(),
            })
        })
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        ACTIVE_DISPATCHES.with(|s| {
            let mut stack = s.borrow_mut();
            if let Some(pos) = stack.iter().rposition(|id| id == &self.plugin_id) {
                stack.remove(pos);
            }
        });
    }
}

use nexus_kernel::{
    audit, Capability, CapabilitySet, EventBus, EventFilter, EventSubscription, IpcDispatcher,
    IpcError, IpcFuture, KernelPluginContext, PluginInfo, PluginStatus, TrustLevel,
};

use crate::manifest::{self, PluginManifest};
use crate::sandbox::{PluginData, PluginEventForwarder, WasmSandbox};
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
pub enum PluginBackend {
    /// Native Rust plugin; no sandbox overhead, unrestricted kernel access.
    Core(Box<dyn CorePlugin>),
    /// WASM-sandboxed plugin; capability-gated, fuel-metered.
    Community(WasmSandbox),
    /// JS plugin executed in the Tauri `WebView`. No backend runtime state —
    /// dispatch is handled entirely by the frontend. Backend only tracks
    /// manifest, settings, and event subscriptions.
    Script,
}

impl PluginBackend {
    /// Dispatch a call to the handler identified by `handler_id`.
    ///
    /// Script plugins return [`PluginError::ScriptDispatchFrontend`] —
    /// their handlers execute in the Tauri `WebView`, not on the backend.
    ///
    /// # Errors
    ///
    /// Propagates whatever [`PluginError`] the inner backend returns, plus
    /// [`PluginError::ScriptDispatchFrontend`] for script plugins.
    pub fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match self {
            Self::Core(p) => p.dispatch(handler_id, args),
            Self::Community(s) => s.dispatch(handler_id, args),
            Self::Script => Err(PluginError::ScriptDispatchFrontend),
        }
    }

    /// Async dispatch: delegates to the core plugin's `dispatch_async`. WASM
    /// sandboxes and script plugins always return `None`.
    pub(crate) fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        match self {
            Self::Core(p) => p.dispatch_async(handler_id, args),
            Self::Community(_) | Self::Script => None,
        }
    }

    pub(crate) fn call_on_init(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_init(),
            Self::Community(s) => s.call_on_init(),
            Self::Script => Ok(()), // Lifecycle runs in frontend JS
        }
    }

    pub(crate) fn call_on_start(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_start(),
            Self::Community(s) => s.call_on_start(),
            Self::Script => Ok(()),
        }
    }

    pub(crate) fn call_on_stop(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => {
                p.on_stop();
                Ok(())
            }
            Self::Community(s) => s.call_on_stop(),
            Self::Script => Ok(()),
        }
    }

    pub(crate) fn call_on_load(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(_) | Self::Script => Ok(()),
            Self::Community(s) => s.call_on_load(),
        }
    }

    pub(crate) fn call_on_unload(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(_) | Self::Script => Ok(()),
            Self::Community(s) => s.call_on_unload(),
        }
    }

    /// Hand the plugin its [`KernelPluginContext`]. Core-only; WASM sandboxes
    /// and script plugins receive their runtime state differently.
    pub(crate) fn call_wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        if let Self::Core(p) = self {
            p.wire_context(ctx);
        }
    }

    pub(crate) fn call_on_enable(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_enable(),
            Self::Community(s) => s.call_on_enable(),
            Self::Script => Ok(()),
        }
    }

    pub(crate) fn call_on_disable(&mut self) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => {
                p.on_disable();
                Ok(())
            }
            Self::Community(s) => s.call_on_disable(),
            Self::Script => Ok(()),
        }
    }

    pub(crate) fn call_on_settings_changed(
        &mut self,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        match self {
            Self::Core(p) => p.on_settings_changed(settings),
            Self::Community(s) => s.call_on_settings_changed(settings),
            Self::Script => Ok(()), // Frontend JS module handles this
        }
    }
}

// ─── Internal structs ─────────────────────────────────────────────────────────

struct PluginRegistrations {
    cli_subcommands: Vec<String>,
}

/// A live event subscription wired to a plugin handler.
struct PluginEventSub {
    /// Subscription identifier from the manifest.
    id: String,
    /// Filter expression from the manifest (e.g. `"nexus.host.*"`).
    filter: String,
    /// Handler invoked when a matching event arrives.
    handler_id: u32,
    /// Live subscription handle (dropped → auto-unsubscribe).
    /// `None` when the subscription has been disabled by the user.
    subscription: Option<EventSubscription>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    /// Per-plugin backend lock. Dispatch methods resolve the target
    /// (read-only) and then lock only the target backend, enabling
    /// plugin-to-plugin IPC without holding the loader lock during
    /// WASM execution.
    backend: Arc<Mutex<PluginBackend>>,
    capabilities: CapabilitySet,
    status: PluginStatus,
    plugin_dir: PathBuf,
    registrations: PluginRegistrations,
    /// Active event subscriptions for this plugin.
    event_subs: Vec<PluginEventSub>,
    /// Shared live cache of the plugin's validated settings JSON. Mirrored
    /// into the sandbox's [`PluginData`] so `host::get_settings` can read
    /// it without acquiring the loader lock. Rewritten in-place by
    /// [`PluginLoader::update_settings`] (and re-seeded during hot-reload)
    /// so plugins see user edits on their next handler call.
    settings_cache: Arc<RwLock<String>>,
    /// True while the plugin is in the middle of a hot-reload. Dispatch
    /// methods check this and return [`PluginError::PluginReloading`]
    /// instead of racing the per-plugin backend mutex.
    reloading: Arc<AtomicBool>,
    /// Number of consecutive [`PluginError::ExecutionTimeout`] results
    /// since the last successful dispatch (PRD-04 §13). When it reaches
    /// [`PluginLoader::max_timeout_streak`], the plugin is quarantined
    /// and further dispatches return [`PluginError::Quarantined`].
    timeout_streak: Arc<AtomicU32>,
    /// Set once the plugin has tripped the consecutive-timeout threshold.
    /// Dispatch methods short-circuit with [`PluginError::Quarantined`]
    /// until the user runs `nexus plugin reset <id>` (or the plugin is
    /// reloaded).
    quarantined: Arc<AtomicBool>,
}

// ─── PluginLoader ─────────────────────────────────────────────────────────────

/// Manages loading, unloading, and dispatching to WASM plugins.
///
/// Maintains a registry of loaded plugins keyed by their manifest ID, and a
/// separate CLI-dispatch table mapping subcommand IDs to plugin IDs.
pub struct PluginLoader {
    /// Directories searched by [`scan`](Self::scan). The first entry is the
    /// default `.forge/plugins` path; additional entries come from
    /// `KernelConfig::plugin_search_paths`.
    search_paths: Vec<PathBuf>,
    /// Consecutive-timeout threshold. When a plugin hits this many
    /// [`PluginError::ExecutionTimeout`] results in a row the loader
    /// auto-quarantines it (PRD-04 §13). `0` disables the watchdog.
    max_timeout_streak: u32,
    loaded: HashMap<String, LoadedPlugin>,
    /// Plugin IDs in registration order. Used by
    /// [`PluginManager::shutdown`] to stop plugins in reverse-registration
    /// order so a plugin that subscribes to another plugin's events is
    /// never stopped before its source.
    registration_order: Vec<String>,
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
            search_paths: vec![plugins_dir.to_path_buf()],
            max_timeout_streak: 3,
            loaded: HashMap::new(),
            registration_order: Vec::new(),
            cli_registry: HashMap::new(),
            settings: SettingsManager::new(),
            event_bus: None,
        }
    }

    /// Override the consecutive-timeout watchdog threshold (PRD-04 §13).
    /// `0` disables auto-quarantine. The default is 3.
    pub fn set_max_timeout_streak(&mut self, max: u32) {
        self.max_timeout_streak = max;
    }

    /// Inspect a dispatch result and update the per-plugin timeout
    /// streak. On [`PluginError::ExecutionTimeout`] the counter is
    /// incremented; any other result (success *or* failure) resets it.
    /// When the streak reaches `max_timeout_streak` the plugin is
    /// quarantined and its persistent crash counter is bumped so the
    /// quarantine survives restart.
    fn record_dispatch_result<T>(
        &self,
        plugin_id: &str,
        result: &Result<T, PluginError>,
    ) {
        let Some(lp) = self.loaded.get(plugin_id) else { return };
        match result {
            Err(PluginError::ExecutionTimeout { .. }) => {
                let next = lp.timeout_streak.fetch_add(1, Ordering::AcqRel) + 1;
                if self.max_timeout_streak > 0 && next >= self.max_timeout_streak {
                    lp.quarantined.store(true, Ordering::Release);
                    let plugin_dir = lp.plugin_dir.clone();
                    let _ = crate::bump_crash_count(&plugin_dir);
                    tracing::warn!(
                        audit = true,
                        plugin_id = %plugin_id,
                        consecutive_timeouts = next,
                        "plugin quarantined after consecutive timeouts; \
                         run `nexus plugin reset <id>` to reactivate"
                    );
                }
            }
            _ => {
                lp.timeout_streak.store(0, Ordering::Release);
            }
        }
    }

    /// Clear any in-memory quarantine state for `plugin_id` and reset the
    /// consecutive-timeout counter. Called by
    /// [`PluginManager::reset_crash_count`] after the user investigates.
    pub(crate) fn clear_quarantine(&self, plugin_id: &str) {
        if let Some(lp) = self.loaded.get(plugin_id) {
            lp.quarantined.store(false, Ordering::Release);
            lp.timeout_streak.store(0, Ordering::Release);
        }
    }

    /// Quarantine check invoked at the top of every dispatch path. Returns
    /// [`PluginError::Quarantined`] when the plugin has tripped the
    /// consecutive-timeout watchdog.
    fn check_quarantine(&self, plugin_id: &str) -> Result<(), PluginError> {
        if let Some(lp) = self.loaded.get(plugin_id) {
            if lp.quarantined.load(Ordering::Acquire) {
                return Err(PluginError::Quarantined {
                    plugin_id: plugin_id.to_string(),
                    consecutive_timeouts: lp.timeout_streak.load(Ordering::Acquire),
                });
            }
        }
        Ok(())
    }

    /// Primary search path (the one passed to `PluginLoader::new`). Used
    /// for crash-counter bookkeeping that needs `<plugins_dir>/<plugin_id>/…`.
    #[must_use]
    pub fn plugins_dir(&self) -> &Path {
        &self.search_paths[0]
    }

    /// Append an additional directory to the search path list.
    ///
    /// [`scan`](Self::scan) will visit all registered paths; plugins found in
    /// later paths are loaded after earlier ones.
    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Inject the kernel event bus.
    ///
    /// Must be called before loading plugins that declare event subscriptions.
    /// Already-loaded plugins will not retroactively subscribe; reload them to
    /// pick up the new bus.
    pub fn set_event_bus(&mut self, bus: Arc<EventBus>) {
        self.event_bus = Some(bus);
    }

    /// Walk all registered search paths and return subdirectories that contain
    /// a `manifest.toml` file.
    ///
    /// # Errors
    /// Returns [`PluginError::Io`] if any search directory cannot be read
    /// (directories that simply don't exist are silently skipped).
    pub fn scan(&self) -> Result<Vec<PathBuf>, PluginError> {
        let mut found = Vec::new();

        for search_dir in &self.search_paths {
            let read_dir = match std::fs::read_dir(search_dir) {
                Ok(rd) => rd,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(PluginError::Io(e)),
            };

            for entry in read_dir {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && path.join("manifest.toml").exists() {
                    found.push(path);
                }
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

        // Step 2a (F-9.2.1): enforce api_version major compatibility.
        check_api_version(&manifest.api_version, &manifest.id)?;

        // Step 2b (PRD-04 §12): ensure every declared dependency is
        // present and its installed version satisfies the requested
        // semver range. Dependencies must be registered before their
        // dependents — bootstrap orders core plugins accordingly.
        self.check_dependencies(&manifest)?;

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

        let capabilities = build_capabilities(&manifest, plugin_dir);
        let settings_cache = load_settings_cache(&self.settings, &plugin_id, plugin_dir);

        // Step 6: Create backend — WASM sandbox or Script marker.
        let mut backend = if manifest.script.is_some() {
            // Script plugins execute in the frontend; no backend runtime.
            PluginBackend::Script
        } else {
            // WASM community plugin — build sandbox.
            let wasm_cfg = manifest.wasm.as_ref().ok_or_else(|| PluginError::ManifestInvalid {
                path: manifest_path.display().to_string(),
                reason: "internal: community plugin passed validation without a [wasm] or [script] section"
                    .to_string(),
            })?;
            let wasm_path = plugin_dir.join(&wasm_cfg.module);
            let wasm_bytes = std::fs::read(&wasm_path)?;
            // Build a ForgePathValidator scoped to the plugin's forge root
            // so the write host function can close the symlink-swap
            // TOCTOU race (MK audit finding F-5.3.1). If construction
            // fails (non-existent or non-canonicalizable root), the
            // validator stays `None` and the host write path falls back
            // to denying every write — safer than silently degrading.
            let path_validator = nexus_types::ForgePathValidator::new(plugin_dir).ok();
            let plugin_data = PluginData {
                plugin_id: plugin_id.clone(),
                capabilities: capabilities.clone(),
                forge_root: plugin_dir.to_path_buf(),
                path_validator,
                settings_json: Some(settings_cache.clone()),
                ..Default::default()
            };
            PluginBackend::Community(
                WasmSandbox::new(&wasm_bytes, wasm_cfg, plugin_data)?,
            )
        };

        // Step 7: Call lifecycle hooks (no-ops for Script plugins)
        if manifest.lifecycle.on_load {
            backend.call_on_load()?;
        }
        if manifest.lifecycle.on_init {
            backend.call_on_init()?;
        }
        if manifest.lifecycle.on_start {
            backend.call_on_start()?;
        }

        self.finish_loading(manifest, backend, capabilities, plugin_dir, settings_cache)
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

        // F-9.2.1: enforce api_version even for core plugins so an out-of-date
        // bundled manifest fails loud rather than silently running against a
        // mismatched host.
        check_api_version(&manifest.api_version, &manifest.id)?;

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

        let capabilities = build_capabilities(&manifest, plugin_dir);
        let backend = PluginBackend::Core(plugin);
        let settings_cache = load_settings_cache(&self.settings, &plugin_id, plugin_dir);
        self.finish_loading(manifest, backend, capabilities, plugin_dir, settings_cache)
    }

    /// Shared final step: register CLI/IPC, wire event subscriptions, insert.
    fn finish_loading(
        &mut self,
        manifest: PluginManifest,
        backend: PluginBackend,
        capabilities: CapabilitySet,
        plugin_dir: &Path,
        settings_cache: Arc<RwLock<String>>,
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

        // Load persisted subscription overrides (disabled IDs).
        let disabled_subs = load_disabled_subscriptions(plugin_dir);

        // Wire event subscriptions to the kernel bus (if available).
        let event_subs: Vec<PluginEventSub> = manifest
            .registrations
            .event_subscribers
            .iter()
            .map(|reg| {
                let enabled = !disabled_subs.contains(&reg.id);
                PluginEventSub {
                    id: reg.id.clone(),
                    filter: reg.filter.clone(),
                    handler_id: reg.handler_id,
                    subscription: if enabled {
                        self.event_bus
                            .as_ref()
                            .map(|bus| bus.subscribe(parse_event_filter(&reg.filter)))
                    } else {
                        None
                    },
                }
            })
            .collect();

        let info = plugin_info_from(&manifest, PluginStatus::Running, &capabilities);

        self.registration_order.push(plugin_id.clone());
        self.loaded.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                backend: Arc::new(Mutex::new(backend)),
                capabilities,
                status: PluginStatus::Running,
                plugin_dir: plugin_dir.to_path_buf(),
                registrations: PluginRegistrations {
                    cli_subcommands: registered_cli,
                },
                event_subs,
                settings_cache,
                reloading: Arc::new(AtomicBool::new(false)),
                timeout_streak: Arc::new(AtomicU32::new(0)),
                quarantined: Arc::new(AtomicBool::new(false)),
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
        let loaded = self
            .loaded
            .remove(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        // Best-effort on_stop then on_unload — plugin is removed regardless of errors.
        if loaded.manifest.lifecycle.on_stop {
            let _ = loaded.backend.lock().map(|mut b| b.call_on_stop());
        }
        if loaded.manifest.lifecycle.on_unload {
            let _ = loaded.backend.lock().map(|mut b| b.call_on_unload());
        }

        // Deregister CLI subcommands
        for sub_id in &loaded.registrations.cli_subcommands {
            self.cli_registry.remove(sub_id);
        }

        // Drop from the registration-order tracker so shutdown won't revisit.
        self.registration_order.retain(|id| id != plugin_id);

        Ok(())
    }

    /// Return all loaded plugin IDs in registration order (earliest first).
    ///
    /// [`PluginManager::shutdown`] iterates this in reverse to stop plugins
    /// in LIFO order, guaranteeing an event source is still alive when its
    /// subscribers run their `on_stop`.
    #[must_use]
    pub fn registration_order(&self) -> Vec<String> {
        self.registration_order.clone()
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

    /// Return all registered CLI subcommands as `(id, description)` pairs.
    #[must_use]
    pub fn list_cli_subcommands(&self) -> Vec<(String, String)> {
        self.loaded
            .values()
            .flat_map(|lp| {
                lp.manifest.registrations.cli_subcommands.iter().map(|r| {
                    (r.id.clone(), r.description.clone())
                })
            })
            .collect()
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
        &self,
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
            .get(&plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.clone()))?;

        let handler_id = lp
            .manifest
            .registrations
            .cli_subcommands
            .iter()
            .find(|r| r.id == subcommand)
            .map(|r| r.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(subcommand.to_string()))?;

        self.check_quarantine(&plugin_id)?;

        let backend = lp.backend.clone();
        let mut guard = backend
            .lock()
            .map_err(|_| PluginError::PluginNotFound(plugin_id.clone()))?;
        let result = guard.dispatch(handler_id, args);
        drop(guard);
        self.record_dispatch_result(&plugin_id, &result);
        result
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
        &self,
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
    /// Resolve a plugin IPC target without dispatching.
    ///
    /// Returns the cloned backend handle and the resolved `handler_id`,
    /// allowing callers to release any outer lock before calling into
    /// the backend. Used by [`dispatch_ipc`](Self::dispatch_ipc) and by
    /// [`SharedPluginLoader`] to avoid holding the loader mutex during
    /// WASM execution.
    pub fn resolve_ipc(
        &self,
        plugin_id: &str,
        command_id: &str,
    ) -> Result<(Arc<Mutex<PluginBackend>>, u32), PluginError> {
        let lp = self
            .loaded
            .get(plugin_id)
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

        Ok((lp.backend.clone(), handler_id))
    }

    /// Dispatch an IPC command call.
    ///
    /// Resolves the target via [`resolve_ipc`](Self::resolve_ipc), locks the
    /// per-plugin backend, and dispatches. A thread-local guard rejects true
    /// reentrant calls (same thread already dispatching the same plugin);
    /// ordinary cross-thread contention queues on the mutex instead of
    /// erroring.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or command is
    /// unknown. Returns [`PluginError::ReentrantCall`] if a recursive call
    /// would deadlock on the per-plugin mutex. Propagates sandbox dispatch
    /// errors.
    pub fn dispatch_ipc(
        &self,
        plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        if let Some(lp) = self.loaded.get(plugin_id) {
            if lp.reloading.load(Ordering::Acquire) {
                return Err(PluginError::PluginReloading(plugin_id.to_string()));
            }
        }
        self.check_quarantine(plugin_id)?;
        let (backend, handler_id) = self.resolve_ipc(plugin_id, command_id)?;
        let _dispatch_guard =
            DispatchGuard::enter(plugin_id).ok_or_else(|| PluginError::ReentrantCall {
                plugin_id: plugin_id.to_string(),
                command: command_id.to_string(),
            })?;
        let mut guard = backend
            .lock()
            .map_err(|_| PluginError::PluginNotFound(plugin_id.to_string()))?;
        let result = guard.dispatch(handler_id, args);
        drop(guard);
        self.record_dispatch_result(plugin_id, &result);
        result
    }

    /// Return the hot-reload flag for `plugin_id`, or `None` if not loaded.
    pub(crate) fn reloading_flag(&self, plugin_id: &str) -> Option<Arc<AtomicBool>> {
        self.loaded.get(plugin_id).map(|lp| lp.reloading.clone())
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
            lp.backend
                .lock()
                .map_err(|_| PluginError::PluginNotFound(plugin_id.to_string()))?
                .call_on_enable()?;
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
            lp.backend
                .lock()
                .map_err(|_| PluginError::PluginNotFound(plugin_id.to_string()))?
                .call_on_disable()?;
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
        // Refresh the shared settings cache so the next `host::get_settings`
        // call from the sandbox reads the new values. Poisoned-lock
        // failures are logged; the file on disk is already authoritative.
        let serialized = serde_json::to_string_pretty(settings)
            .unwrap_or_else(|_| "{}".to_string());
        if let Ok(mut guard) = lp.settings_cache.write() {
            *guard = serialized;
        } else {
            tracing::warn!(
                plugin_id = %plugin_id,
                "update_settings: settings cache lock poisoned; skipping in-memory refresh"
            );
        }
        if lp.manifest.lifecycle.on_settings_changed {
            lp.backend
                .lock()
                .map_err(|_| PluginError::PluginNotFound(plugin_id.to_string()))?
                .call_on_settings_changed(settings)?;
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
        lp.backend
            .lock()
            .map_err(|_| PluginError::PluginNotFound(plugin_id.to_string()))?
            .call_wire_context(ctx);
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
                let Some(ref mut subscription) = sub.subscription else {
                    continue; // disabled subscription
                };
                // Drain until no more events or the subscription is lagged/closed.
                while let Ok(Some(evt)) = subscription.try_recv() {
                    let payload = serde_json::to_value(&*evt)
                        .unwrap_or(serde_json::Value::Null);
                    pending.push((plugin_id.clone(), sub.handler_id, payload));
                }
            }
        }

        let mut responses = Vec::with_capacity(pending.len());
        for (plugin_id, handler_id, payload) in pending {
            if let Some(lp) = self.loaded.get(&plugin_id) {
                let backend = lp.backend.clone();
                let response = backend
                    .lock()
                    .map_err(|_| PluginError::PluginNotFound(plugin_id.clone()))?
                    .dispatch(handler_id, &payload)
                    .map_err(|e| {
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

    /// Persist an install-time user consent for a HIGH-risk capability on
    /// `plugin_id` (F-5.1.1). Writes `<plugin_dir>/granted_caps.json`
    /// pinned to the plugin's currently-loaded version — a subsequent
    /// version bump that re-requests the capability will re-prompt.
    ///
    /// The grant takes effect on the next plugin load (or hot-reload).
    /// Non-HIGH-risk capabilities are auto-granted from the manifest and
    /// calling `grant_capability` on them is a no-op.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if `plugin_id` is not loaded,
    /// or a generic `io`-backed error if the grants file cannot be written.
    pub fn grant_capability(
        &self,
        plugin_id: &str,
        cap: Capability,
    ) -> Result<(), PluginError> {
        let Some(lp) = self.loaded.get(plugin_id) else {
            return Err(PluginError::PluginNotFound(plugin_id.to_string()));
        };
        if !cap.is_high_risk() {
            return Ok(());
        }
        let plugin_dir = lp.plugin_dir.clone();
        let version = lp.manifest.version.clone();
        write_grant(&plugin_dir, &version, cap, true)
    }

    /// Revoke a previously-persisted capability grant for `plugin_id`
    /// (F-5.1.1). The revoke takes effect on the next plugin load.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if `plugin_id` is not loaded,
    /// or an `io`-backed error if the grants file cannot be rewritten.
    pub fn revoke_capability(
        &self,
        plugin_id: &str,
        cap: Capability,
    ) -> Result<(), PluginError> {
        let Some(lp) = self.loaded.get(plugin_id) else {
            return Err(PluginError::PluginNotFound(plugin_id.to_string()));
        };
        if !cap.is_high_risk() {
            return Ok(());
        }
        let plugin_dir = lp.plugin_dir.clone();
        let version = lp.manifest.version.clone();
        write_grant(&plugin_dir, &version, cap, false)
    }

    // ─── Internal helpers for hot-reload ──────────────────────────────────────

    /// Return a mutable reference to the [`WasmSandbox`] for `plugin_id`.
    ///
    /// Returns `None` if the plugin is not loaded or is a core (native) plugin.
    /// Hot-reload only applies to community WASM plugins.
    pub(crate) fn backend_arc(&self, plugin_id: &str) -> Option<Arc<Mutex<PluginBackend>>> {
        self.loaded.get(plugin_id).map(|lp| lp.backend.clone())
    }

    /// Return a reference to the [`PluginManifest`] for `plugin_id`.
    pub(crate) fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.loaded.get(plugin_id).map(|lp| &lp.manifest)
    }

    /// Verify every `[dependencies]` entry in `manifest` is satisfied by
    /// an already-loaded plugin whose installed version matches the
    /// declared semver range (PRD-04 §12). A missing or version-conflict
    /// dependency returns [`PluginError::DependencyUnsatisfied`].
    ///
    /// The manifest's individual `version_req` strings are already
    /// validated by [`manifest::validate`], so `VersionReq::parse` here
    /// cannot fail for reasons other than a parser regression.
    fn check_dependencies(&self, manifest: &PluginManifest) -> Result<(), PluginError> {
        for dep in &manifest.dependencies {
            let Some(loaded) = self.loaded.get(&dep.plugin_id) else {
                return Err(PluginError::DependencyUnsatisfied {
                    plugin_id: manifest.id.clone(),
                    reason: format!(
                        "required plugin '{}' ({}) is not loaded",
                        dep.plugin_id, dep.version_req
                    ),
                });
            };
            let req = semver::VersionReq::parse(&dep.version_req).map_err(|e| {
                PluginError::DependencyUnsatisfied {
                    plugin_id: manifest.id.clone(),
                    reason: format!(
                        "invalid semver range '{}' for dependency '{}': {e}",
                        dep.version_req, dep.plugin_id
                    ),
                }
            })?;
            let installed = semver::Version::parse(&loaded.manifest.version).map_err(|e| {
                PluginError::DependencyUnsatisfied {
                    plugin_id: manifest.id.clone(),
                    reason: format!(
                        "dependency '{}' has invalid installed version '{}': {e}",
                        dep.plugin_id, loaded.manifest.version
                    ),
                }
            })?;
            if !req.matches(&installed) {
                return Err(PluginError::DependencyUnsatisfied {
                    plugin_id: manifest.id.clone(),
                    reason: format!(
                        "dependency '{}' version {} does not satisfy '{}'",
                        dep.plugin_id, installed, dep.version_req
                    ),
                });
            }
        }
        Ok(())
    }

    /// Clone the shared settings cache for `plugin_id`. Used by hot-reload
    /// to hand the new sandbox the same `Arc` the old one saw, so user
    /// edits that happened before the reload remain visible afterwards.
    pub(crate) fn settings_cache(&self, plugin_id: &str) -> Option<Arc<RwLock<String>>> {
        self.loaded.get(plugin_id).map(|lp| lp.settings_cache.clone())
    }

    /// Update the [`PluginStatus`] for `plugin_id`.
    pub(crate) fn set_status(&mut self, plugin_id: &str, status: PluginStatus) {
        if let Some(lp) = self.loaded.get_mut(plugin_id) {
            lp.status = status;
        }
    }

    /// Replace the [`WasmSandbox`] for `plugin_id` during hot-reload.
    ///
    /// Only valid for community WASM plugins. Returns `None` if the plugin
    /// is not loaded or is a core plugin.
    pub(crate) fn replace_sandbox(
        &mut self,
        plugin_id: &str,
        sandbox: WasmSandbox,
        capabilities: CapabilitySet,
    ) -> Option<WasmSandbox> {
        self.loaded.get_mut(plugin_id).and_then(|lp| {
            let mut backend_guard = lp.backend.lock().ok()?;
            if let PluginBackend::Community(_) = &*backend_guard {
                let old_backend =
                    std::mem::replace(&mut *backend_guard, PluginBackend::Community(sandbox));
                // Keep the loader's cached capability set in sync with the
                // sandbox's. `loader.get()` returns this; observers that
                // inspect `PluginInfo.capabilities` post-reload would see
                // a stale set otherwise. See issue #74.
                drop(backend_guard);
                lp.capabilities = capabilities;
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

    /// Re-evaluate capabilities for `plugin_id` from disk: re-reads
    /// `granted_caps.json` and re-runs the manifest's HIGH-risk filtering
    /// via [`build_capabilities`]. Returns `None` if the plugin is not
    /// loaded.
    ///
    /// Hot-reload must call this rather than reusing the cached
    /// [`CapabilitySet`] — otherwise an operator who edits
    /// `granted_caps.json` to revoke a HIGH-risk cap and then triggers
    /// a reload would silently keep the old grant until the next full
    /// process restart. See issue #74.
    pub(crate) fn refresh_capabilities(&self, plugin_id: &str) -> Option<CapabilitySet> {
        let lp = self.loaded.get(plugin_id)?;
        Some(build_capabilities(&lp.manifest, &lp.plugin_dir))
    }

    /// Inject an [`IpcDispatcher`] into every loaded community (WASM) plugin's
    /// [`PluginData`], enabling `host::invoke_command` to dispatch calls to
    /// other plugins.
    ///
    /// Must be called **after** all plugins are loaded and the dispatcher is
    /// constructed. Newly loaded plugins (e.g. via hot-reload) must be
    /// injected individually via [`inject_ipc_dispatcher_for`].
    pub fn inject_ipc_dispatcher(&mut self, dispatcher: &Arc<dyn IpcDispatcher>) {
        for lp in self.loaded.values() {
            if let Ok(mut backend) = lp.backend.lock() {
                if let PluginBackend::Community(sandbox) = &mut *backend {
                    sandbox.set_ipc_dispatcher(dispatcher.clone());
                }
            }
        }
    }

    /// Inject an [`IpcDispatcher`] into a single community plugin's
    /// [`PluginData`]. Used after hot-reload to wire up a freshly
    /// instantiated sandbox.
    pub fn inject_ipc_dispatcher_for(
        &mut self,
        plugin_id: &str,
        dispatcher: Arc<dyn IpcDispatcher>,
    ) {
        if let Some(lp) = self.loaded.get(plugin_id) {
            if let Ok(mut backend) = lp.backend.lock() {
                if let PluginBackend::Community(sandbox) = &mut *backend {
                    sandbox.set_ipc_dispatcher(dispatcher);
                }
            }
        }
    }

    /// Inject a [`PluginEventForwarder`] into every loaded community
    /// plugin so `host::emit_event` calls are also surfaced to the
    /// application layer (e.g. Tauri frontend).
    pub fn inject_event_forwarder(&mut self, forwarder: &Arc<dyn PluginEventForwarder>) {
        for lp in self.loaded.values() {
            if let Ok(mut backend) = lp.backend.lock() {
                if let PluginBackend::Community(sandbox) = &mut *backend {
                    sandbox.set_event_forwarder(forwarder.clone());
                }
            }
        }
    }

    /// Inject a [`PluginEventForwarder`] into a single community
    /// plugin's sandbox. Symmetric with [`inject_ipc_dispatcher_for`];
    /// used after hot-reload to wire up the freshly built sandbox.
    pub fn inject_event_forwarder_for(
        &mut self,
        plugin_id: &str,
        forwarder: Arc<dyn PluginEventForwarder>,
    ) {
        if let Some(lp) = self.loaded.get(plugin_id) {
            if let Ok(mut backend) = lp.backend.lock() {
                if let PluginBackend::Community(sandbox) = &mut *backend {
                    sandbox.set_event_forwarder(forwarder);
                }
            }
        }
    }

    /// Return the runtime type for `plugin_id`: `"core"`, `"wasm"`, or `"script"`.
    #[must_use]
    pub fn plugin_runtime(&self, plugin_id: &str) -> Option<&'static str> {
        self.loaded.get(plugin_id).map(|lp| {
            let backend = lp.backend.lock().ok();
            match backend.as_deref() {
                Some(PluginBackend::Core(_)) => "core",
                Some(PluginBackend::Community(_)) => "wasm",
                Some(PluginBackend::Script) => "script",
                None => "unknown",
            }
        })
    }

    /// Return the event subscriptions for `plugin_id` as
    /// `(id, filter, enabled)` tuples.
    #[must_use]
    pub fn event_subscriptions(
        &self,
        plugin_id: &str,
    ) -> Vec<(String, String, bool)> {
        self.loaded
            .get(plugin_id)
            .map(|lp| {
                lp.event_subs
                    .iter()
                    .map(|s| (s.id.clone(), s.filter.clone(), s.subscription.is_some()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Enable or disable an event subscription for `plugin_id`.
    ///
    /// When disabling, the live [`EventSubscription`] handle is dropped
    /// (auto-unsubscribes from the bus). When enabling, a new subscription
    /// is created from the stored filter. The toggle state is persisted to
    /// `<plugin_dir>/subscriptions.json`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or
    /// subscription ID is unknown.
    pub fn toggle_event_subscription(
        &mut self,
        plugin_id: &str,
        subscription_id: &str,
        enabled: bool,
    ) -> Result<(), PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        let sub = lp
            .event_subs
            .iter_mut()
            .find(|s| s.id == subscription_id)
            .ok_or_else(|| PluginError::PluginNotFound(
                format!("{plugin_id}:{subscription_id}"),
            ))?;

        if enabled {
            if sub.subscription.is_none() {
                if let Some(ref bus) = self.event_bus {
                    sub.subscription = Some(bus.subscribe(parse_event_filter(&sub.filter)));
                }
            }
        } else {
            // Drop the live subscription handle → auto-unsubscribe.
            sub.subscription = None;
        }

        // Persist the disabled set.
        let disabled: Vec<String> = lp
            .event_subs
            .iter()
            .filter(|s| s.subscription.is_none())
            .map(|s| s.id.clone())
            .collect();
        save_disabled_subscriptions(&lp.plugin_dir, &disabled);

        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a manifest filter string into an [`EventFilter`].
///
/// Rules:
/// - `"*"` or `""` → [`EventFilter::All`]
/// - Known kernel event variant names → [`EventFilter::Variant`]
/// - ends with `".*"` → [`EventFilter::CustomPrefix`] (the prefix before `*`)
/// - otherwise → [`EventFilter::CustomExact`]
fn parse_event_filter(filter: &str) -> EventFilter {
    match filter {
        "" | "*" => EventFilter::All,
        "PluginLoaded"
        | "PluginStarted"
        | "PluginStopped"
        | "PluginCrashed"
        | "CapabilityGranted"
        | "CapabilityDenied" => EventFilter::Variant(filter.to_string()),
        f if f.ends_with(".*") => {
            EventFilter::CustomPrefix(f[..f.len() - 1].to_string()) // strip "*", keep "."
        }
        f => EventFilter::CustomExact(f.to_string()),
    }
}

/// Enforce `api_version` major-version compatibility (F-9.2.1).
///
/// Accepts `"<major>"` (e.g. `"1"`) or `"<major>.<minor>"` (e.g. `"1.2"`).
/// Rejects anything else as `IncompatibleApiVersion`. The host's supported
/// major version is the crate-level constant `PLUGIN_API_VERSION_MAJOR`.
fn check_api_version(requested: &str, plugin_id: &str) -> Result<(), PluginError> {
    let major_part = requested.split('.').next().unwrap_or("");
    let parsed: Option<u32> = major_part.parse().ok();
    match parsed {
        Some(n) if n == crate::PLUGIN_API_VERSION_MAJOR => Ok(()),
        _ => Err(PluginError::IncompatibleApiVersion {
            plugin_id: plugin_id.to_string(),
            requested: requested.to_string(),
            supported: crate::PLUGIN_API_VERSION_MAJOR.to_string(),
        }),
    }
}

/// Build the shared settings cache for a freshly-loaded plugin. Reads the
/// current validated settings via [`SettingsManager::load_settings`] and
/// wraps the pretty-printed JSON in an [`Arc<RwLock<String>>`] so the
/// loader and its sandbox can share a single mutable view. Any error
/// (missing schema, invalid file, I/O) degrades to `"{}"` — a usable
/// default that plugins can parse without special-casing.
fn load_settings_cache(
    settings: &SettingsManager,
    plugin_id: &str,
    plugin_dir: &Path,
) -> Arc<RwLock<String>> {
    let json = settings
        .load_settings(plugin_id, plugin_dir)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| "{}".to_string());
    Arc::new(RwLock::new(json))
}

/// Filename for the per-plugin persisted install-time capability consent
/// (F-5.1.1). Lives alongside the plugin's `plugin.toml` and is keyed by
/// plugin version so a version bump that requests a new HIGH-risk
/// capability re-prompts the user.
const GRANTED_CAPS_FILE: &str = "granted_caps.json";

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
struct GrantedCapsFile {
    /// Plugin version the grants are pinned to. A mismatch with
    /// `manifest.version` resets the grants to empty (re-prompt).
    #[serde(default)]
    version: String,
    /// Capability strings (e.g. `"net.http"`) the user has granted.
    #[serde(default)]
    granted: Vec<String>,
}

/// Load the set of HIGH-risk capabilities the user has consented to for
/// `plugin_version` at `plugin_dir`. Missing file, parse errors, or a
/// version mismatch all yield an empty set (= deny-all) — operators
/// re-grant explicitly for the new version.
fn load_granted_high_risk_caps(plugin_dir: &Path, plugin_version: &str) -> HashSet<Capability> {
    let path = plugin_dir.join(GRANTED_CAPS_FILE);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return HashSet::new();
    };
    let parsed: GrantedCapsFile = match serde_json::from_str(&contents) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(
                audit = true,
                path = %path.display(),
                error = %err,
                "granted_caps.json parse failed — treating as deny-all for HIGH-risk caps",
            );
            return HashSet::new();
        }
    };
    if parsed.version != plugin_version {
        tracing::info!(
            audit = true,
            path = %path.display(),
            grants_version = %parsed.version,
            plugin_version = %plugin_version,
            "granted_caps.json version mismatch — resetting HIGH-risk grants; user must re-consent",
        );
        return HashSet::new();
    }
    parsed
        .granted
        .iter()
        .filter_map(|s| Capability::from_str(s).ok())
        .filter(|c| c.is_high_risk())
        .collect()
}

/// Read the current `granted_caps.json`, merge in (or remove) `cap` for the
/// specified `plugin_version`, and rewrite atomically. Missing / corrupt /
/// version-mismatched existing files are replaced wholesale — the new grant
/// pins to the current version.
fn write_grant(
    plugin_dir: &Path,
    plugin_version: &str,
    cap: Capability,
    grant: bool,
) -> Result<(), PluginError> {
    let path = plugin_dir.join(GRANTED_CAPS_FILE);
    let mut file: GrantedCapsFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|f: &GrantedCapsFile| f.version == plugin_version)
        .unwrap_or_default();
    file.version = plugin_version.to_string();
    let cap_str = cap.as_str().to_string();
    file.granted.retain(|s| s != &cap_str);
    if grant {
        file.granted.push(cap_str);
    }
    file.granted.sort();
    let json = serde_json::to_string_pretty(&file).map_err(|e| PluginError::ManifestInvalid {
        path: path.display().to_string(),
        reason: format!("serialize granted_caps.json: {e}"),
    })?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Build the capability set granted to a plugin at load time.
///
/// Core plugins receive every declared capability unconditionally — they are
/// part of the trusted shell.
///
/// Community plugins receive the union of their declared `required` +
/// `optional` caps, **minus** any HIGH-risk capability that is not present
/// in `<plugin_dir>/granted_caps.json` (F-5.1.1). Denied HIGH-risk caps
/// are logged at `audit = true` level so operators can see which plugins
/// are running with partial capability and decide whether to grant.
///
/// Kept as a free function (rather than a method) so the test harness can
/// exercise it with an isolated `tempfile` directory.
fn build_capabilities(manifest: &PluginManifest, plugin_dir: &Path) -> CapabilitySet {
    match manifest.trust_level {
        TrustLevel::Core => {
            for cap in Capability::ALL {
                audit::log_capability_granted(&manifest.id, cap.as_str());
            }
            Capability::ALL.iter().copied().collect::<CapabilitySet>()
        }
        TrustLevel::Community => {
            let granted = load_granted_high_risk_caps(plugin_dir, &manifest.version);
            let mut denied: Vec<Capability> = Vec::new();
            let caps: Vec<Capability> = manifest
                .capabilities
                .required
                .iter()
                .chain(manifest.capabilities.optional.iter())
                .filter_map(|s| Capability::from_str(s).ok())
                .filter(|c| {
                    if c.is_high_risk() && !granted.contains(c) {
                        denied.push(*c);
                        false
                    } else {
                        true
                    }
                })
                .collect();
            for cap in &caps {
                audit::log_capability_granted(&manifest.id, cap.as_str());
            }
            if !denied.is_empty() {
                let denied_strs: Vec<&str> = denied.iter().map(|c| c.as_str()).collect();
                tracing::warn!(
                    audit = true,
                    plugin_id = %manifest.id,
                    plugin_version = %manifest.version,
                    plugin_dir = %plugin_dir.display(),
                    denied = ?denied_strs,
                    "HIGH-risk capabilities default-denied; grant by adding to granted_caps.json",
                );
            }
            CapabilitySet::from_iter(caps)
        }
    }
}

/// Subscriptions persistence filename.
const SUBSCRIPTIONS_FILE: &str = "subscriptions.json";

/// Load the set of disabled subscription IDs from disk.
///
/// A missing file is normal (first run, or all subscriptions enabled) and
/// yields an empty set. A present-but-corrupt file **fails loud**: the
/// corrupt contents are preserved by renaming the file to
/// `subscriptions.json.corrupt-<unix>` and an `audit = true` warning is
/// emitted so operators can notice that every subscription just silently
/// re-enabled. The empty-set return keeps the plugin loadable rather than
/// bricking it on a disk hiccup.
fn load_disabled_subscriptions(plugin_dir: &Path) -> std::collections::HashSet<String> {
    #[derive(serde::Deserialize)]
    struct SubscriptionState {
        #[serde(default)]
        disabled: Vec<String>,
    }
    let path = plugin_dir.join(SUBSCRIPTIONS_FILE);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return std::collections::HashSet::new();
    };
    match serde_json::from_str::<SubscriptionState>(&contents) {
        Ok(s) => s.disabled.into_iter().collect(),
        Err(err) => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup = plugin_dir.join(format!("{SUBSCRIPTIONS_FILE}.corrupt-{ts}"));
            let rename_result = std::fs::rename(&path, &backup);
            tracing::warn!(
                audit = true,
                plugin_dir = %plugin_dir.display(),
                error = %err,
                backup = %backup.display(),
                renamed = rename_result.is_ok(),
                "corrupt subscriptions.json — falling back to all-enabled and preserving the corrupt file",
            );
            std::collections::HashSet::new()
        }
    }
}

/// Persist the set of disabled subscription IDs to disk.
fn save_disabled_subscriptions(plugin_dir: &Path, disabled: &[String]) {
    let path = plugin_dir.join(SUBSCRIPTIONS_FILE);
    if disabled.is_empty() {
        // Clean up file if everything is enabled.
        let _ = std::fs::remove_file(&path);
        return;
    }
    let json = serde_json::json!({ "disabled": disabled });
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
        tracing::warn!("failed to persist subscription state to {}: {e}", path.display());
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
pub struct SharedPluginLoader {
    inner: Mutex<PluginLoader>,
    /// Per-(target plugin, command) capabilities the caller must hold
    /// in addition to `IpcCall`. Populated by [`Self::add_cap_requirement`]
    /// at bootstrap time and consulted by
    /// [`<SharedPluginLoader as IpcDispatcher>::required_caller_caps`].
    /// Held outside the loader mutex so the on-every-`ipc_call` lookup
    /// doesn't serialize behind plugin loading. See issue #77.
    cap_requirements: std::sync::RwLock<HashMap<(String, String), Vec<Capability>>>,
}

impl SharedPluginLoader {
    /// Wrap a loader for shared kernel access.
    #[must_use]
    pub fn new(loader: PluginLoader) -> Self {
        Self {
            inner: Mutex::new(loader),
            cap_requirements: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Acquire the loader lock; panics on poison.
    ///
    /// # Panics
    /// Panics if the inner mutex is poisoned by a previous panic.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, PluginLoader> {
        self.inner.lock().expect("plugin loader mutex poisoned")
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
        self.inner
            .lock()
            .expect("plugin loader mutex poisoned")
            .wire_context(plugin_id, ctx)
    }

    /// Require callers of `(target_plugin_id, command_id)` to hold every
    /// capability in `caps`, on top of the unconditional `IpcCall` check
    /// the kernel context performs.
    ///
    /// Bootstrap calls this at registration time for the small set of
    /// commands documented as needing more than `IpcCall` — currently
    /// `com.nexus.terminal::create_session` and
    /// `com.nexus.mcp.host::connect`, both of which spawn arbitrary
    /// processes (issue #77). Replaces the prior implicit "any plugin
    /// holding `IpcCall` can spawn arbitrary processes through the
    /// terminal/MCP handlers" laundering surface.
    ///
    /// Idempotent on `(target, command)`: the latest call wins. The
    /// kernel context's `ipc_call` reads under a shared lock, so this
    /// can be called concurrently with active dispatch (it just affects
    /// future calls).
    ///
    /// # Panics
    /// Panics if the requirements lock is poisoned.
    pub fn add_cap_requirement(
        &self,
        target_plugin_id: impl Into<String>,
        command_id: impl Into<String>,
        caps: Vec<Capability>,
    ) {
        let mut map = self
            .cap_requirements
            .write()
            .expect("ipc cap-requirements lock poisoned");
        map.insert((target_plugin_id.into(), command_id.into()), caps);
    }
}

impl IpcDispatcher for SharedPluginLoader {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        // Resolve under the loader lock, then release before backend
        // execution. This lets WASM plugins issue nested IPC calls
        // (host::invoke_command) without deadlocking.
        let (backend, handler_id) = {
            let loader = self
                .inner
                .lock()
                .map_err(|_| IpcError::PluginCrashedDuringCall {
                    plugin_id: target_plugin_id.to_string(),
                    command: command_id.to_string(),
                })?;

            loader.resolve_ipc(target_plugin_id, command_id).map_err(
                |e| match e {
                    PluginError::PluginNotFound(id) => {
                        // Could be the plugin ID or the command ID — check.
                        if id == target_plugin_id {
                            IpcError::PluginNotFound { plugin_id: id }
                        } else {
                            IpcError::CommandNotFound {
                                plugin_id: target_plugin_id.to_string(),
                                command: id,
                            }
                        }
                    }
                    _ => IpcError::PluginCrashedDuringCall {
                        plugin_id: target_plugin_id.to_string(),
                        command: command_id.to_string(),
                    },
                },
            )?
        };
        // Loader lock released. Detect genuine reentrancy (same thread
        // already dispatching this plugin — would deadlock on `lock()`)
        // via a thread-local guard, then take the backend mutex with a
        // blocking `lock()` so unrelated concurrent calls queue rather
        // than erroring.
        let _dispatch_guard =
            DispatchGuard::enter(target_plugin_id).ok_or_else(|| IpcError::ReentrantCall {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })?;
        let mut guard = backend
            .lock()
            .map_err(|_| IpcError::PluginCrashedDuringCall {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })?;
        guard
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
            let loader = self.inner.lock().ok()?;
            let lp = loader.loaded.get(&target)?;
            let handler_id = lp
                .manifest
                .registrations
                .ipc_commands
                .iter()
                .find(|r| r.id == command)
                .map(|r| r.handler_id)?;
            let backend = lp.backend.clone();
            let mut guard = backend.lock().ok()?;
            guard.dispatch_async(handler_id, &args)?
        };

        Some(Box::pin(async move {
            inner.await.map_err(|_| IpcError::PluginCrashedDuringCall {
                plugin_id: target,
                command,
            })
        }))
    }

    fn required_caller_caps(
        &self,
        target_plugin_id: &str,
        command_id: &str,
    ) -> Vec<Capability> {
        // Read-locked map populated by `add_cap_requirement` at bootstrap.
        // Empty result is the default (no extra caps beyond `IpcCall`).
        // See issue #77.
        let map = match self.cap_requirements.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        map.get(&(target_plugin_id.to_string(), command_id.to_string()))
            .cloned()
            .unwrap_or_default()
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

    #[test]
    fn api_version_accepts_matching_major() {
        check_api_version("1", "com.example.p").unwrap();
        check_api_version("1.2", "com.example.p").unwrap();
    }

    #[test]
    fn api_version_rejects_mismatched_major() {
        let err = check_api_version("2", "com.example.p").unwrap_err();
        assert!(matches!(err, PluginError::IncompatibleApiVersion { .. }));
        let err = check_api_version("0", "com.example.p").unwrap_err();
        assert!(matches!(err, PluginError::IncompatibleApiVersion { .. }));
    }

    #[test]
    fn api_version_rejects_garbage() {
        let err = check_api_version("abc", "com.example.p").unwrap_err();
        assert!(matches!(err, PluginError::IncompatibleApiVersion { .. }));
        let err = check_api_version("", "com.example.p").unwrap_err();
        assert!(matches!(err, PluginError::IncompatibleApiVersion { .. }));
    }

    // ─── F-5.1.1: HIGH-risk capability gating ─────────────────────────────

    fn community_manifest(caps: &[&str]) -> PluginManifest {
        use crate::manifest::{
            ActivationConfig, LifecycleConfig, ManifestCapabilities, PluginRuntime, Registrations,
        };
        PluginManifest {
            id: "com.example.hr".to_string(),
            name: "High-Risk Plugin".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Community,
            api_version: "1".to_string(),
            runtime: PluginRuntime::Wasm,
            capabilities: ManifestCapabilities {
                required: caps.iter().map(|s| (*s).to_string()).collect(),
                optional: vec![],
            },
            wasm: None,
            script: None,
            settings: None,
            registrations: Registrations::default(),
            lifecycle: LifecycleConfig::default(),
            activation: ActivationConfig::default(),
            dependencies: vec![],
        }
    }

    #[test]
    fn high_risk_caps_default_denied_for_community_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = community_manifest(&["fs.read", "net.http", "process.spawn"]);
        let caps = build_capabilities(&manifest, dir.path());
        assert!(caps.contains(Capability::FsRead), "low-risk cap must be granted");
        assert!(
            !caps.contains(Capability::NetHttp),
            "HIGH-risk net.http must be denied without grants file"
        );
        assert!(
            !caps.contains(Capability::ProcessSpawn),
            "HIGH-risk process.spawn must be denied without grants file"
        );
    }

    #[test]
    fn granted_caps_file_allows_high_risk_through_for_matching_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(GRANTED_CAPS_FILE),
            r#"{"version":"1.0.0","granted":["net.http"]}"#,
        )
        .unwrap();
        let manifest = community_manifest(&["net.http", "process.spawn"]);
        let caps = build_capabilities(&manifest, dir.path());
        assert!(caps.contains(Capability::NetHttp), "granted cap must be present");
        assert!(
            !caps.contains(Capability::ProcessSpawn),
            "non-granted HIGH-risk cap must still be denied"
        );
    }

    #[test]
    fn granted_caps_version_mismatch_resets_grants() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(GRANTED_CAPS_FILE),
            r#"{"version":"0.9.0","granted":["net.http"]}"#,
        )
        .unwrap();
        let manifest = community_manifest(&["net.http"]);
        let caps = build_capabilities(&manifest, dir.path());
        assert!(
            !caps.contains(Capability::NetHttp),
            "version mismatch must re-deny previously-granted caps"
        );
    }

    #[test]
    fn core_plugins_get_all_caps_regardless_of_grants_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = community_manifest(&[]);
        manifest.trust_level = TrustLevel::Core;
        let caps = build_capabilities(&manifest, dir.path());
        assert!(caps.contains(Capability::NetHttp));
        assert!(caps.contains(Capability::ProcessSpawn));
        assert!(caps.contains(Capability::FsWriteExternal));
    }

    #[test]
    fn write_grant_round_trips_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        write_grant(dir.path(), "1.0.0", Capability::NetHttp, true).unwrap();
        write_grant(dir.path(), "1.0.0", Capability::ProcessSpawn, true).unwrap();
        let granted = load_granted_high_risk_caps(dir.path(), "1.0.0");
        assert!(granted.contains(&Capability::NetHttp));
        assert!(granted.contains(&Capability::ProcessSpawn));

        write_grant(dir.path(), "1.0.0", Capability::NetHttp, false).unwrap();
        let granted = load_granted_high_risk_caps(dir.path(), "1.0.0");
        assert!(!granted.contains(&Capability::NetHttp));
        assert!(granted.contains(&Capability::ProcessSpawn));
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

    fn setup_plugin_dir_with_dep(
        plugin_id: &str,
        dep_id: &str,
        version_req: &str,
    ) -> tempfile::TempDir {
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
name = "Dependent"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read"]

[wasm]
module = "test.wasm"

[dependencies]
"{dep_id}" = "{version_req}"
"#
        );
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
        tmp
    }

    #[test]
    fn load_rejects_missing_dependency() {
        let tmp = setup_plugin_dir_with_dep(
            "com.test.needs-dep",
            "com.test.provider",
            "^1.0.0",
        );
        let plugin_dir = tmp.path().join("com.test.needs-dep");
        let mut loader = PluginLoader::new(tmp.path());
        let err = loader.load(&plugin_dir).unwrap_err();
        assert!(
            matches!(
                err,
                PluginError::DependencyUnsatisfied { ref reason, .. }
                    if reason.contains("com.test.provider")
                        && reason.contains("not loaded")
            ),
            "expected DependencyUnsatisfied(not loaded), got {err:?}"
        );
    }

    #[test]
    fn load_rejects_dependency_version_mismatch() {
        // Provider installed at 1.0.0; dependent requires ^2.0 → reject.
        let provider_tmp = setup_plugin_dir("com.test.provider.v1");
        let provider_dir = provider_tmp.path().join("com.test.provider.v1");

        let dependent_tmp = setup_plugin_dir_with_dep(
            "com.test.needs-v2",
            "com.test.provider.v1",
            "^2.0.0",
        );
        let dependent_dir = dependent_tmp.path().join("com.test.needs-v2");

        let mut loader = PluginLoader::new(provider_tmp.path());
        loader.load(&provider_dir).unwrap();
        let err = loader.load(&dependent_dir).unwrap_err();
        assert!(
            matches!(
                err,
                PluginError::DependencyUnsatisfied { ref reason, .. }
                    if reason.contains("does not satisfy")
            ),
            "expected DependencyUnsatisfied(version mismatch), got {err:?}"
        );
    }

    #[test]
    fn consecutive_timeouts_quarantine_plugin() {
        let tmp = setup_plugin_dir("com.test.watchdog");
        let plugin_dir = tmp.path().join("com.test.watchdog");
        let mut loader = PluginLoader::new(tmp.path());
        loader.set_max_timeout_streak(3);
        loader.load(&plugin_dir).unwrap();

        let plugin_id = "com.test.watchdog";
        let synth_timeout = || -> Result<(), PluginError> {
            Err(PluginError::ExecutionTimeout {
                plugin_id: plugin_id.to_string(),
            })
        };

        // First two timeouts leave the plugin operational.
        loader.record_dispatch_result(plugin_id, &synth_timeout());
        assert!(loader.check_quarantine(plugin_id).is_ok());
        loader.record_dispatch_result(plugin_id, &synth_timeout());
        assert!(loader.check_quarantine(plugin_id).is_ok());

        // Third timeout crosses the threshold → quarantined.
        loader.record_dispatch_result(plugin_id, &synth_timeout());
        let err = loader.check_quarantine(plugin_id).unwrap_err();
        assert!(
            matches!(
                err,
                PluginError::Quarantined { ref plugin_id, consecutive_timeouts }
                    if plugin_id == "com.test.watchdog" && consecutive_timeouts == 3
            ),
            "expected Quarantined, got {err:?}"
        );
    }

    #[test]
    fn success_resets_timeout_streak() {
        let tmp = setup_plugin_dir("com.test.reset-streak");
        let plugin_dir = tmp.path().join("com.test.reset-streak");
        let mut loader = PluginLoader::new(tmp.path());
        loader.set_max_timeout_streak(3);
        loader.load(&plugin_dir).unwrap();

        let plugin_id = "com.test.reset-streak";
        let timeout = Err::<(), _>(PluginError::ExecutionTimeout {
            plugin_id: plugin_id.to_string(),
        });
        let success = Ok::<(), PluginError>(());

        // Two timeouts, then a success resets the counter so the next
        // timeout doesn't trip the watchdog.
        loader.record_dispatch_result(plugin_id, &timeout);
        loader.record_dispatch_result(plugin_id, &timeout);
        loader.record_dispatch_result(plugin_id, &success);
        loader.record_dispatch_result(plugin_id, &timeout);

        assert!(
            loader.check_quarantine(plugin_id).is_ok(),
            "single post-success timeout should not quarantine"
        );
    }

    #[test]
    fn clear_quarantine_reactivates_plugin() {
        let tmp = setup_plugin_dir("com.test.clear-q");
        let plugin_dir = tmp.path().join("com.test.clear-q");
        let mut loader = PluginLoader::new(tmp.path());
        loader.set_max_timeout_streak(1);
        loader.load(&plugin_dir).unwrap();

        let plugin_id = "com.test.clear-q";
        let timeout = Err::<(), _>(PluginError::ExecutionTimeout {
            plugin_id: plugin_id.to_string(),
        });

        loader.record_dispatch_result(plugin_id, &timeout);
        assert!(loader.check_quarantine(plugin_id).is_err());

        loader.clear_quarantine(plugin_id);
        assert!(loader.check_quarantine(plugin_id).is_ok());
    }

    #[test]
    fn load_succeeds_when_dependency_satisfied() {
        let provider_tmp = setup_plugin_dir("com.test.dep.provider");
        let provider_dir = provider_tmp.path().join("com.test.dep.provider");

        let dependent_tmp = setup_plugin_dir_with_dep(
            "com.test.dep.dependent",
            "com.test.dep.provider",
            "^1.0.0",
        );
        let dependent_dir = dependent_tmp.path().join("com.test.dep.dependent");

        let mut loader = PluginLoader::new(provider_tmp.path());
        loader.load(&provider_dir).unwrap();
        let info = loader.load(&dependent_dir).unwrap();
        assert_eq!(info.id, "com.test.dep.dependent");
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
    /// the echo handler (id 100) instead of a `cli_subcommand`.
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
