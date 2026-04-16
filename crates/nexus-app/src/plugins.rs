//! Plugin integration: Tauri managed state + IPC commands.
//!
//! Holds a [`PluginManager`] behind a mutex so the frontend can list
//! plugin-contributed command palette entries and invoke them by id.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::{EventBus, IpcDispatcher, IpcError, NexusEvent};
use nexus_plugins::{
    PluginBackend, PluginEventForwarder, PluginManager, PluginManagerConfig, PluginStatus,
    TrustLevel, UiContribution, UiPanelContribution, UiRibbonItemContribution,
    UiSettingsTabContribution, UiStatusItemContribution,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// Tauri event emitted when one or more community plugins have been
/// hot-reloaded. Payload: `{ "plugin_ids": ["com.nexus.hello", …] }`.
pub const PLUGINS_RELOADED_EVENT: &str = "plugins:reloaded";

/// Tauri event emitted once per plugin-side event. Payload:
/// `{ "plugin_id": "...", "topic": "...", "payload": <any> }`. Plugins
/// surface events by returning an `events` array in their handler
/// response; `invoke_plugin_command` extracts the array and fires one
/// of these per entry.
pub const PLUGIN_EVENT_EVENT: &str = "plugin:event";

/// Reserved source id for events published by the Nexus host shell
/// (forge switches, file opens, theme changes, etc.). Plugins can
/// subscribe with `filter = "nexus.host.*"` to receive every host
/// lifecycle event, or narrow to a specific topic.
pub const HOST_EVENT_SOURCE: &str = "nexus.host";

/// How often the background watcher thread drains pending hot-reload
/// events. The underlying `HotReloader` already debounces filesystem
/// events, so this just needs to be short enough to feel live.
const RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How often the background watcher thread drains pending host/plugin
/// events and dispatches them to subscribing plugin handlers.
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Broadcast capacity of the shared [`EventBus`]. Subscribers that lag
/// past this bound will receive a `RecvError::Lagged` from their
/// subscription; for host events that's acceptable because the host
/// re-fires the interesting state on user interaction.
const EVENT_BUS_CAPACITY: usize = 256;

/// Tauri-managed [`PluginManager`] plus the shared kernel [`EventBus`]
/// it's wired to. The manager lives behind a mutex for interior
/// mutability; the bus is an `Arc` so host commands can publish
/// directly without contending on the manager lock.
pub struct PluginState {
    /// The plugin manager (holds the registry, sandboxes, etc.).
    /// Wrapped in `Arc` so [`TauriIpcDispatcher`] can share it.
    pub manager: Arc<Mutex<PluginManager>>,
    /// The shared event bus. Host code publishes via
    /// [`EventBus::publish_core`]; plugin subscriptions drain on the
    /// background watcher thread.
    pub bus: Arc<EventBus>,
}

// ─── TauriIpcDispatcher ──────────────────────────────────────────────────────

/// [`IpcDispatcher`] impl for the Tauri-managed [`PluginManager`].
///
/// Resolves the target plugin under the manager lock, releases the lock,
/// then dispatches via the per-plugin backend lock. This two-phase
/// approach lets WASM plugins issue nested IPC calls from within
/// `host::invoke_command` without deadlocking.
struct TauriIpcDispatcher {
    manager: Arc<Mutex<PluginManager>>,
}

impl IpcDispatcher for TauriIpcDispatcher {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        let (backend, handler_id) = {
            let mgr = self
                .manager
                .lock()
                .map_err(|_| IpcError::PluginCrashedDuringCall {
                    plugin_id: target_plugin_id.to_string(),
                    command: command_id.to_string(),
                })?;
            mgr.resolve_ipc(target_plugin_id, command_id).map_err(
                |e| match e {
                    nexus_plugins::PluginError::PluginNotFound(id) => {
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
        // Manager lock released — safe for nested calls.
        let mut guard = backend
            .try_lock()
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
}

// ─── TauriEventForwarder ─────────────────────────────────────────────────────

/// Forwards `host::emit_event` calls from WASM plugins to the Tauri
/// frontend as [`PLUGIN_EVENT_EVENT`] events, so the UI receives them
/// in real time.
struct TauriEventForwarder {
    app: AppHandle,
}

impl PluginEventForwarder for TauriEventForwarder {
    fn forward(&self, plugin_id: &str, type_id: &str, payload: &serde_json::Value) {
        let envelope = serde_json::json!({
            "plugin_id": plugin_id,
            "topic": type_id,
            "payload": payload,
        });
        if let Err(err) = self.app.emit(PLUGIN_EVENT_EVENT, envelope) {
            tracing::warn!(%err, plugin = plugin_id, topic = type_id, "event forwarder: emit failed");
        }
    }
}

/// Frontend-facing projection of [`nexus_kernel::PluginInfo`].
///
/// Kept separate from `PluginInfo` so we can serialize without forcing
/// `Serialize` onto kernel types (their `CapabilitySet` in particular).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginSummary {
    /// Plugin identifier (reverse-DNS).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Version string from the manifest.
    pub version: String,
    /// Trust level — `"core"` or `"community"`.
    pub trust_level: String,
    /// Current runtime status — `"loaded"`, `"initialized"`, `"running"`,
    /// `"stopped"`, or `"crashed"`.
    pub status: String,
    /// Plugin runtime: `"core"`, `"wasm"`, or `"script"`.
    pub runtime: String,
    /// Event subscriptions declared by this plugin.
    pub event_subscriptions: Vec<SubscriptionSummary>,
}

/// A single event subscription declared in a plugin's manifest.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubscriptionSummary {
    /// Subscription identifier from the manifest.
    pub id: String,
    /// Event filter expression (e.g. `"nexus.host.*"`).
    pub filter: String,
    /// Whether the subscription is currently active.
    pub enabled: bool,
}

fn trust_level_str(level: TrustLevel) -> &'static str {
    match level {
        TrustLevel::Core => "core",
        TrustLevel::Community => "community",
    }
}

fn status_str(status: PluginStatus) -> &'static str {
    match status {
        PluginStatus::Loaded => "loaded",
        PluginStatus::Initialized => "initialized",
        PluginStatus::Running => "running",
        PluginStatus::Stopped => "stopped",
        PluginStatus::Crashed => "crashed",
    }
}

/// Resolve the plugins directory.
///
/// Order of precedence:
/// 1. `NEXUS_PLUGINS_DIR` environment variable (absolute path).
/// 2. The repository's `plugins/` directory when running in dev (detected by
///    walking up from `CARGO_MANIFEST_DIR`).
/// 3. `$CWD/plugins`.
fn resolve_plugins_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("NEXUS_PLUGINS_DIR") {
        return PathBuf::from(explicit);
    }
    // crates/nexus-app -> repo root is two levels up.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo_root) = manifest_dir.parent().and_then(|p| p.parent()) {
        let candidate = repo_root.join("plugins");
        if candidate.exists() {
            return candidate;
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("plugins")
}

/// Build the [`PluginManager`], scan the plugins directory, and return the
/// managed state. Missing plugin directories are created silently so the
/// hot-reload watcher can attach.
///
/// The shared [`EventBus`] is created and injected into the manager
/// **before** `load_all()` so any plugin that declares
/// `[[registrations.event_subscriber]]` picks up its subscription at
/// load time.
pub fn bootstrap() -> PluginState {
    let dir = resolve_plugins_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%err, path = %dir.display(), "failed to ensure plugins dir");
    }
    let config = PluginManagerConfig::default();
    let mut manager = match PluginManager::new(&dir, &config) {
        Ok(m) => m,
        Err(err) => {
            tracing::warn!(%err, "plugin manager init failed; plugins disabled");
            // Fall back to a no-op manager rooted at a scratch dir so the
            // managed-state shape is preserved.
            let scratch = std::env::temp_dir().join("nexus-plugins-empty");
            let _ = std::fs::create_dir_all(&scratch);
            PluginManager::new(
                &scratch,
                &PluginManagerConfig {
                    hot_reload: false,
                    ..PluginManagerConfig::default()
                },
            )
            .expect("scratch plugin manager")
        }
    };
    let bus = Arc::new(EventBus::new(EVENT_BUS_CAPACITY));
    manager.set_event_bus(bus.clone());
    match manager.load_all() {
        Ok(infos) => {
            tracing::info!(count = infos.len(), "loaded plugins");
        }
        Err(err) => {
            tracing::warn!(%err, "plugin scan failed");
        }
    }

    // Wrap the manager and create a dispatcher for plugin-to-plugin IPC.
    let manager = Arc::new(Mutex::new(manager));
    let dispatcher: Arc<dyn IpcDispatcher> = Arc::new(TauriIpcDispatcher {
        manager: manager.clone(),
    });
    // Inject the dispatcher into every community (WASM) plugin so
    // host::invoke_command can route calls.
    if let Ok(mut mgr) = manager.lock() {
        mgr.inject_ipc_dispatcher(dispatcher);
    }

    PluginState { manager, bus }
}

/// Inject a [`TauriEventForwarder`] into all loaded community plugins
/// so `host::emit_event` calls are surfaced to the frontend as
/// `plugin:event` Tauri events.
///
/// Must be called from the `setup` closure where the [`AppHandle`] is
/// available — `bootstrap()` runs before the app handle exists.
pub fn inject_event_forwarder(handle: AppHandle) {
    let forwarder: Arc<dyn PluginEventForwarder> = Arc::new(TauriEventForwarder {
        app: handle.clone(),
    });
    let manager = {
        let Some(state) = handle.try_state::<PluginState>() else {
            return;
        };
        state.manager.clone()
    };
    let Ok(mut mgr) = manager.lock() else { return };
    mgr.inject_event_forwarder(forwarder);
}

/// List all plugin-contributed palette commands across every loaded plugin.
#[tauri::command]
pub fn list_plugin_contributions(state: State<'_, PluginState>) -> Vec<UiContribution> {
    state
        .manager
        .lock()
        .map(|mgr| mgr.ui_contributions())
        .unwrap_or_default()
}

/// List all plugin-contributed side panels across every loaded plugin.
/// The frontend merges these into the active layout's left/right side
/// panel arrays at render time.
#[tauri::command]
pub fn list_plugin_panels(state: State<'_, PluginState>) -> Vec<UiPanelContribution> {
    state
        .manager
        .lock()
        .map(|mgr| mgr.ui_panels())
        .unwrap_or_default()
}

/// List all plugin-contributed Settings-modal tabs. The frontend
/// renders one row per tab under the Settings modal's "Plugins" rail
/// group.
#[tauri::command]
pub fn list_plugin_settings_tabs(
    state: State<'_, PluginState>,
) -> Vec<UiSettingsTabContribution> {
    state
        .manager
        .lock()
        .map(|mgr| mgr.ui_settings_tabs())
        .unwrap_or_default()
}

/// List all plugin-contributed workspace-ribbon icons. The frontend
/// merges these into the active layout's `ribbon` array at render
/// time.
#[tauri::command]
pub fn list_plugin_ribbon_items(
    state: State<'_, PluginState>,
) -> Vec<UiRibbonItemContribution> {
    state
        .manager
        .lock()
        .map(|mgr| mgr.ui_ribbon_items())
        .unwrap_or_default()
}

/// List all plugin-contributed status-bar entries. The frontend
/// merges these into the active layout's `statusBar` array.
#[tauri::command]
pub fn list_plugin_status_items(
    state: State<'_, PluginState>,
) -> Vec<UiStatusItemContribution> {
    state
        .manager
        .lock()
        .map(|mgr| mgr.ui_status_items())
        .unwrap_or_default()
}

/// Return the JSON Schema declared by `plugin_id`, or `null` if the
/// plugin isn't loaded or didn't declare a `[settings]` block.
#[tauri::command]
pub fn get_plugin_settings_schema(
    state: State<'_, PluginState>,
    plugin_id: String,
) -> Option<serde_json::Value> {
    state
        .manager
        .lock()
        .ok()
        .and_then(|mgr| mgr.get_settings_schema(&plugin_id))
}

/// Load the currently persisted settings for `plugin_id`. Empty
/// object when no settings file exists yet.
///
/// # Errors
/// Returns the load error as a string for the frontend.
#[tauri::command]
pub fn get_plugin_settings(
    state: State<'_, PluginState>,
    plugin_id: String,
) -> Result<serde_json::Value, String> {
    let mgr = state
        .manager
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.get_settings(&plugin_id).map_err(|e| e.to_string())
}

/// Validate `settings` against the registered schema and, if valid,
/// persist them to `<plugin_dir>/settings.json`. Fires the plugin's
/// `on_settings_changed` lifecycle hook if declared.
///
/// # Errors
/// Returns validation / I/O errors as a string for the frontend.
#[tauri::command]
pub fn save_plugin_settings(
    state: State<'_, PluginState>,
    plugin_id: String,
    settings: serde_json::Value,
) -> Result<(), String> {
    let mut mgr = state
        .manager
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.set_settings(&plugin_id, &settings).map_err(|e| e.to_string())
}

/// List every loaded plugin as a serializable summary — used by the
/// Settings modal's plugins tab.
#[tauri::command]
pub fn list_plugins(state: State<'_, PluginState>) -> Vec<PluginSummary> {
    let Ok(mgr) = state.manager.lock() else {
        return Vec::new();
    };
    mgr.list()
        .into_iter()
        .map(|info| {
            let subs = mgr
                .event_subscriptions(&info.id)
                .into_iter()
                .map(|(id, filter, enabled)| SubscriptionSummary { id, filter, enabled })
                .collect();
            let runtime = mgr.plugin_runtime(&info.id)
                .unwrap_or("unknown").to_string();
            PluginSummary {
                id: info.id,
                name: info.name,
                version: info.version,
                trust_level: trust_level_str(info.trust_level).to_string(),
                status: status_str(info.status).to_string(),
                runtime,
                event_subscriptions: subs,
            }
        })
        .collect()
}

/// Invoke a plugin command by `plugin_id` and `command_id`, forwarding
/// arbitrary JSON `args`.
///
/// Side-effect: if the plugin's response is a JSON object containing
/// an `events: [{ topic, payload }, …]` array, each entry is emitted
/// as a [`PLUGIN_EVENT_EVENT`] Tauri event with
/// `{ plugin_id, topic, payload }`. The `events` key is left in the
/// returned value; the frontend can either ignore it or route it
/// through the dedicated event bus.
///
/// # Errors
/// Returns the dispatch error as a string for the frontend.
#[tauri::command]
pub fn invoke_plugin_command(
    app: AppHandle,
    state: State<'_, PluginState>,
    plugin_id: String,
    command_id: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Resolve the backend handle under the manager lock, release the
    // lock, then dispatch via the per-plugin backend lock. This lets
    // WASM plugins issue nested IPC calls without deadlocking.
    let (backend, handler_id) = {
        let mgr = state
            .manager
            .lock()
            .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
        mgr.resolve_ipc(&plugin_id, &command_id)
            .map_err(|e| e.to_string())?
    };
    let mut guard = backend
        .try_lock()
        .map_err(|_| format!("plugin {plugin_id} backend lock contention"))?;
    let result = guard.dispatch(handler_id, &args).map_err(|e| e.to_string())?;
    drop(guard);
    emit_plugin_events(&app, &plugin_id, &result);
    Ok(result)
}

/// Dispatch a capability-checked plugin-to-plugin IPC call.
///
/// Like [`invoke_plugin_command`], but first verifies that
/// `caller_plugin_id` holds the `IpcCall` capability before dispatching
/// to `target_plugin_id`. Intended for the frontend to trigger
/// plugin-to-plugin interactions on behalf of a specific plugin.
///
/// # Errors
/// Returns a string error on capability denial or dispatch failure.
#[tauri::command]
pub fn invoke_plugin_ipc(
    app: AppHandle,
    state: State<'_, PluginState>,
    caller_plugin_id: String,
    target_plugin_id: String,
    command_id: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = {
        let mgr = state
            .manager
            .lock()
            .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
        mgr.dispatch_ipc_checked(
            &caller_plugin_id,
            &target_plugin_id,
            &command_id,
            &args,
        )
        .map_err(|e| e.to_string())?
    };
    emit_plugin_events(&app, &target_plugin_id, &result);
    Ok(result)
}

/// Read the JS source code for a script plugin.
///
/// Returns the file contents as a UTF-8 string. Used by the frontend
/// to load script plugins via `new Function` or dynamic `import()`.
///
/// # Errors
/// Returns an error if the plugin is not found, is not a script plugin,
/// or the file cannot be read.
#[tauri::command]
pub fn read_plugin_script(
    state: State<'_, PluginState>,
    plugin_id: String,
) -> Result<String, String> {
    let mgr = state
        .manager
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    let runtime = mgr.plugin_runtime(&plugin_id)
        .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
    if runtime != "script" {
        return Err(format!("plugin {plugin_id} is not a script plugin (runtime: {runtime})"));
    }
    let plugin_dir = mgr.plugin_dir(&plugin_id)
        .ok_or_else(|| format!("plugin directory not found for {plugin_id}"))?
        .to_path_buf();
    let manifest = mgr.manifest(&plugin_id)
        .ok_or_else(|| format!("manifest not found for {plugin_id}"))?;
    let script_cfg = manifest.script.as_ref()
        .ok_or_else(|| format!("no [script] section in manifest for {plugin_id}"))?;
    let script_path = plugin_dir.join(&script_cfg.module);
    std::fs::read_to_string(&script_path)
        .map_err(|e| format!("failed to read {}: {e}", script_path.display()))
}

/// Toggle an event subscription on or off for a specific plugin.
///
/// # Errors
/// Returns a string error if the plugin or subscription is unknown.
#[tauri::command]
pub fn toggle_plugin_subscription(
    state: State<'_, PluginState>,
    plugin_id: String,
    subscription_id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut mgr = state
        .manager
        .lock()
        .map_err(|e| format!("plugin manager lock poisoned: {e}"))?;
    mgr.toggle_event_subscription(&plugin_id, &subscription_id, enabled)
        .map_err(|e| e.to_string())
}

/// Pull an optional `events` array off a plugin's response and emit
/// each entry as a [`PLUGIN_EVENT_EVENT`] Tauri event. Malformed
/// entries (missing `topic`, non-object, etc.) are logged and skipped
/// so one bad event can't take out the rest.
fn emit_plugin_events(app: &AppHandle, plugin_id: &str, result: &serde_json::Value) {
    let Some(events) = result.get("events").and_then(|v| v.as_array()) else {
        return;
    };
    for event in events {
        let Some(topic) = event.get("topic").and_then(|v| v.as_str()) else {
            tracing::warn!(plugin = plugin_id, "plugin event missing 'topic'; skipping");
            continue;
        };
        let payload = event
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let envelope = serde_json::json!({
            "plugin_id": plugin_id,
            "topic": topic,
            "payload": payload,
        });
        if let Err(err) = app.emit(PLUGIN_EVENT_EVENT, envelope) {
            tracing::warn!(%err, plugin = plugin_id, topic, "failed to emit plugin event");
        }
    }
}

/// Publish a host-origin lifecycle event onto the shared event bus.
///
/// The frontend calls this whenever the UI transitions through a
/// state that plugins may care about — forge switches, file opens,
/// theme changes, and so on. `topic` must begin with
/// [`HOST_EVENT_SOURCE`] (`"nexus.host."`); plugins subscribe with
/// `filter = "nexus.host.*"` (or a more specific exact topic) in
/// their manifest's `[[registrations.event_subscriber]]` block.
///
/// # Errors
/// Returns a stringified error when `topic` does not begin with
/// `"nexus.host."` or when the bus is shut down.
#[tauri::command]
pub fn publish_host_event(
    state: State<'_, PluginState>,
    topic: String,
    payload: serde_json::Value,
) -> Result<(), String> {
    let prefix = format!("{HOST_EVENT_SOURCE}.");
    if !topic.starts_with(&prefix) {
        return Err(format!(
            "host event topic '{topic}' must begin with '{prefix}'"
        ));
    }
    let event = NexusEvent::Custom {
        type_id: topic,
        emitting_plugin: HOST_EVENT_SOURCE.to_string(),
        payload,
    };
    state
        .bus
        .publish_core(HOST_EVENT_SOURCE, event)
        .map_err(|e| e.to_string())
}

/// Spawn a background thread that drains pending event subscriptions
/// via [`PluginManager::poll_events`]. Every handler response is
/// inspected for an `events: [{ topic, payload }, …]` array — matches
/// are re-emitted as [`PLUGIN_EVENT_EVENT`] Tauri events, keeping the
/// frontend consistent whether a plugin surfaces an event from an
/// interactive command or from an async subscription callback.
pub fn start_host_event_watcher(handle: AppHandle) {
    std::thread::Builder::new()
        .name("nexus-plugin-event-watcher".to_string())
        .spawn(move || loop {
            std::thread::sleep(EVENT_POLL_INTERVAL);
            let Some(state) = handle.try_state::<PluginState>() else {
                // Managed state disappeared — app is shutting down.
                return;
            };
            let responses = {
                let Ok(mut mgr) = state.manager.lock() else {
                    continue;
                };
                match mgr.poll_events() {
                    Ok(r) => r,
                    Err(err) => {
                        tracing::warn!(%err, "poll_events failed");
                        continue;
                    }
                }
            };
            for (plugin_id, result) in responses {
                emit_plugin_events(&handle, &plugin_id, &result);
            }
        })
        .expect("spawn plugin event watcher");
}

/// Spawn a background thread that drains [`PluginManager::poll_reloads`]
/// and emits [`PLUGINS_RELOADED_EVENT`] to the frontend whenever one or
/// more plugins have been hot-reloaded.
///
/// The thread lives for the app process lifetime. It is cheap: it sleeps
/// between polls and only briefly locks the [`PluginState`] mutex to
/// drain pending events.
pub fn start_reload_watcher(handle: AppHandle) {
    std::thread::Builder::new()
        .name("nexus-plugin-reload-watcher".to_string())
        .spawn(move || loop {
            std::thread::sleep(RELOAD_POLL_INTERVAL);
            let Some(state) = handle.try_state::<PluginState>() else {
                // Managed state disappeared — app is shutting down.
                return;
            };
            let reloaded = {
                let Ok(mut mgr) = state.manager.lock() else {
                    continue;
                };
                match mgr.poll_reloads() {
                    Ok(ids) => ids,
                    Err(err) => {
                        tracing::warn!(%err, "poll_reloads failed");
                        continue;
                    }
                }
            };
            if reloaded.is_empty() {
                continue;
            }
            tracing::info!(plugins = ?reloaded, "hot-reloaded plugins");
            if let Err(err) = handle.emit(
                PLUGINS_RELOADED_EVENT,
                serde_json::json!({ "plugin_ids": reloaded }),
            ) {
                tracing::warn!(%err, "failed to emit plugins:reloaded");
            }
        })
        .expect("spawn plugin reload watcher");
}

