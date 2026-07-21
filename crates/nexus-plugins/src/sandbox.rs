//! WASM sandbox: wraps a single plugin's wasmtime Engine/Module/Store/Instance
//! and provides the `dispatch` call used by all higher-level plugin code.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use wasmtime::{Engine, Instance, Linker, Module, Store, StoreLimitsBuilder, Trap};

use nexus_kernel::{CapabilitySet, EventBus, IpcDispatcher, KvStore};
use nexus_types::ForgePathValidator;

use crate::{PluginError, WasmConfig};

// ─── PluginEventForwarder ────────────────────────────────────────────────────

/// Callback for forwarding plugin events to the application layer.
///
/// When a WASM plugin calls `host::emit_event`, the event is published
/// to the kernel [`EventBus`] and also forwarded through this trait so
/// the Tauri frontend receives a `plugin:event` notification in real
/// time (rather than only via the `events` return-array path).
pub trait PluginEventForwarder: Send + Sync {
    /// Forward an event to the application layer.
    fn forward(&self, plugin_id: &str, type_id: &str, payload: &serde_json::Value);
}

// ─── PluginData ───────────────────────────────────────────────────────────────

/// Per-plugin data stored inside the wasmtime [`Store`].
///
/// Host functions receive a `Caller<'_, PluginData>` giving access to this
/// data alongside the WASM memory.
pub struct PluginData {
    /// Reverse-DNS plugin identifier (e.g. `com.example.my-plugin`).
    pub plugin_id: String,
    /// Capabilities that were granted to this plugin at load time.
    pub capabilities: CapabilitySet,
    /// Kernel KV store (injected by kernel at load time). `None` in test
    /// sandboxes that don't need storage.
    pub kv: Option<Arc<dyn KvStore>>,
    /// Kernel event bus (injected by kernel at load time). `None` in test
    /// sandboxes.
    pub event_bus: Option<Arc<EventBus>>,
    /// Forge root path used to confine file I/O from host functions.
    pub forge_root: PathBuf,
    /// Path validator rooted at `forge_root`. Used by the write host
    /// function to close the canonicalize-parent-then-open TOCTOU race
    /// (MK audit finding F-5.3.1). `None` in test sandboxes that never
    /// exercise the write path or that construct a `PluginData` with an
    /// empty forge root.
    pub path_validator: Option<ForgePathValidator>,
    /// Live cache of the plugin's settings JSON. The loader initialises
    /// this with the validated settings at load time (or `"{}"` when no
    /// schema is declared) and overwrites the contents in-place whenever
    /// the user saves new values, so `host::get_settings` always reads
    /// the authoritative view.
    pub settings_json: Option<Arc<RwLock<String>>>,
    /// wasmtime resource limiter — enforces `memory_mb` cap.
    /// [`WasmSandbox::new`] overwrites this with the config-derived limit;
    /// callers may supply any placeholder (e.g. `StoreLimitsBuilder::new().build()`).
    pub limits: wasmtime::StoreLimits,
    /// Dispatcher for plugin-to-plugin IPC. Injected after all plugins are
    /// loaded so `host::invoke_command` can route calls to other plugins.
    /// `None` until [`WasmSandbox::set_ipc_dispatcher`] is called.
    pub ipc_dispatch: Option<Arc<dyn IpcDispatcher>>,
    /// Forwarder for surfacing `host::emit_event` calls to the Tauri
    /// frontend as `plugin:event` events. Injected during bootstrap.
    pub event_forwarder: Option<Arc<dyn PluginEventForwarder>>,
    /// Token bucket that caps `host::log` emission at a few thousand
    /// lines per second per plugin. Prevents a runaway plugin from
    /// flooding the host logger and consuming disk/CPU.
    pub log_rate: Arc<Mutex<TokenBucket>>,
    /// Policy governing `host::http_request` (C81). Closed by default;
    /// [`crate::loader::PluginLoader::set_network_policy`] installs the
    /// operator's `<forge>/.forge/sandbox.toml` `[http]` values at
    /// bootstrap, threaded into every subsequently loaded plugin's
    /// `PluginData`.
    pub network_policy: NetworkPolicy,
}

/// Policy governing the `host::http_request` WASM host function (C81).
///
/// Mirrors `nexus_security::HttpPolicy`'s shape without depending on that
/// crate — `nexus-security` depends on `nexus-plugins` (it implements
/// [`crate::CorePlugin`]), so the reverse dependency isn't available.
/// `nexus-bootstrap`, which already depends on both, copies the operator's
/// `sandbox.toml` `[http]` values across at wiring time.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkPolicy {
    /// Whether `host::http_request` is permitted at all.
    pub enabled: bool,
    /// Hosts that may be requested (exact host match).
    pub allowed_hosts: Vec<String>,
    /// Hard cap on a single response body, in bytes.
    pub max_response_bytes: u64,
    /// Per-request timeout, in milliseconds.
    pub timeout_ms: u64,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_hosts: Vec::new(),
            max_response_bytes: 10 * 1024 * 1024,
            timeout_ms: 30_000,
        }
    }
}

/// Methods `NetworkPolicy::validate` will forward. Deliberately excludes
/// `CONNECT`/`TRACE` and anything non-standard.
const NETWORK_POLICY_ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

impl NetworkPolicy {
    /// Validate `method` + `url` against this policy. Pure — performs no
    /// I/O. Returns the uppercased method and parsed URL on success, or a
    /// human-readable refusal reason.
    ///
    /// # Errors
    /// Returns a `String` describing the first rule that failed.
    pub fn validate(&self, method: &str, url: &str) -> Result<(String, reqwest::Url), String> {
        if !self.enabled {
            return Err("brokered http requests are disabled by policy".to_string());
        }
        let method = method.to_ascii_uppercase();
        if !NETWORK_POLICY_ALLOWED_METHODS.contains(&method.as_str()) {
            return Err(format!("unsupported method {method:?}"));
        }
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
        if parsed.scheme() != "https" {
            return Err(format!(
                "only https requests are permitted (got scheme {:?})",
                parsed.scheme()
            ));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| "invalid url: missing host".to_string())?;
        if !self.allowed_hosts.iter().any(|h| h == host) {
            return Err(format!("host {host:?} is not on the http allowlist"));
        }
        Ok((method, parsed))
    }
}

/// Token bucket limiter: refills `refill_per_sec` tokens per second up to
/// `capacity`. Each successful call to [`try_consume`](Self::try_consume)
/// subtracts one token. Cheap and allocation-free at steady state.
#[derive(Debug)]
pub struct TokenBucket {
    /// Maximum tokens the bucket can hold.
    capacity: f64,
    /// Tokens added per second.
    refill_per_sec: f64,
    /// Current token count.
    tokens: f64,
    /// Instant of the last refill computation.
    last_refill: Instant,
    /// Count of consume attempts that were denied since the last successful
    /// one; lets callers emit a single suppressed-count log line when the
    /// bucket refills.
    pub denied_since_last: u64,
}

impl TokenBucket {
    /// Create a new bucket filled to capacity.
    #[must_use]
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            tokens: capacity,
            last_refill: Instant::now(),
            denied_since_last: 0,
        }
    }

    /// Attempt to consume one token. Returns `true` on success.
    pub fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.denied_since_last = self.denied_since_last.saturating_add(1);
            false
        }
    }
}

impl Default for PluginData {
    fn default() -> Self {
        Self {
            plugin_id: String::new(),
            capabilities: CapabilitySet::empty(),
            kv: None,
            event_bus: None,
            forge_root: PathBuf::new(),
            path_validator: None,
            settings_json: None,
            limits: StoreLimitsBuilder::new().build(),
            ipc_dispatch: None,
            event_forwarder: None,
            // Capacity 2000 / 1000-per-sec: ~2s of log burst absorbed,
            // 1k lines/sec sustained. Matches the "1000 lines/second"
            // target from F-6.2.2.
            log_rate: Arc::new(Mutex::new(TokenBucket::new(2000.0, 1000.0))),
            network_policy: NetworkPolicy::default(),
        }
    }
}

// ─── WasmSandbox ─────────────────────────────────────────────────────────────

/// A sandboxed WASM plugin instance.
///
/// Owns the wasmtime [`Store`] and [`Instance`] for a single plugin.
/// Call [`WasmSandbox::dispatch`] to invoke the plugin's `nexus_dispatch`
/// export, or use the lifecycle helpers ([`call_on_init`], [`call_on_start`],
/// [`call_on_stop`]).
///
/// [`call_on_init`]: WasmSandbox::call_on_init
/// [`call_on_start`]: WasmSandbox::call_on_start
/// [`call_on_stop`]: WasmSandbox::call_on_stop
pub struct WasmSandbox {
    store: Store<PluginData>,
    instance: Instance,
    /// Cloned handle used to increment the epoch from the timeout watcher thread.
    engine: Engine,
    /// Wall-clock dispatch deadline from the manifest; 0 means no limit.
    max_execution_ms: u64,
    /// Per-call fuel budget from the manifest; 0 means metering disabled.
    /// Reset at the top of every `dispatch` so a long-lived plugin does
    /// not accumulate instruction usage across handler invocations.
    fuel_per_call: u64,
}

impl std::fmt::Debug for WasmSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmSandbox")
            .field("plugin_id", &self.store.data().plugin_id)
            .finish_non_exhaustive()
    }
}

impl WasmSandbox {
    /// Load and instantiate a WASM module from raw bytes.
    ///
    /// # Errors
    /// Returns [`PluginError::WasmLoadFailed`] when the bytes are not valid
    /// WASM or instantiation fails for any reason.
    pub fn new(
        wasm_bytes: &[u8],
        config: &WasmConfig,
        plugin_data: PluginData,
    ) -> Result<Self, PluginError> {
        let plugin_id = plugin_data.plugin_id.clone();

        let mut wt_config = wasmtime::Config::new();
        wt_config.wasm_simd(true);
        wt_config.wasm_bulk_memory(true);
        if config.fuel > 0 {
            wt_config.consume_fuel(true);
        }
        if config.max_execution_ms > 0 {
            wt_config.epoch_interruption(true);
        }

        let engine = Engine::new(&wt_config).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("engine creation failed: {e}"),
        })?;

        let module = Module::new(&engine, wasm_bytes).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("module compilation failed: {e}"),
        })?;

        let mut store = Store::new(&engine, plugin_data);
        store.data_mut().limits = StoreLimitsBuilder::new()
            .memory_size(config.memory_mb as usize * 1024 * 1024)
            .build();
        store.limiter(|data| &mut data.limits);

        if config.fuel > 0 {
            store
                .set_fuel(config.fuel)
                .map_err(|e| PluginError::WasmLoadFailed {
                    plugin_id: plugin_id.clone(),
                    reason: format!("set_fuel failed: {e}"),
                })?;
        }

        let mut linker: Linker<PluginData> = Linker::new(&engine);
        crate::host_fns::register_host_fns(&mut linker).map_err(|e| {
            PluginError::WasmLoadFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("host function registration failed: {e}"),
            }
        })?;

        let instance =
            linker
                .instantiate(&mut store, &module)
                .map_err(|e| PluginError::WasmLoadFailed {
                    plugin_id: plugin_id.clone(),
                    reason: format!("instantiation failed: {e}"),
                })?;

        Ok(Self {
            store,
            instance,
            engine,
            max_execution_ms: config.max_execution_ms,
            fuel_per_call: config.fuel,
        })
    }

    /// Fuel consumed by the most recent `dispatch()` call, or `None`
    /// when fuel metering is disabled for this plugin (manifest
    /// `fuel = 0`). C86 / #439 — makes quota pressure observable
    /// instead of only fatal (an `OutOfFuel` trap with no prior
    /// visibility into how close the plugin was running to its cap).
    #[must_use]
    pub fn fuel_consumed_last_call(&self) -> Option<u64> {
        if self.fuel_per_call == 0 {
            return None;
        }
        self.store
            .get_fuel()
            .ok()
            .map(|remaining| self.fuel_per_call.saturating_sub(remaining))
    }

    /// Current WASM linear-memory size in bytes, or `None` if the
    /// module exports no memory (shouldn't happen for a plugin that
    /// passed [`WasmSandbox::new`], which requires one for the
    /// dispatch ABI, but a defensive `None` beats a panic). C86 / #439.
    #[must_use]
    pub fn memory_size_bytes(&mut self) -> Option<u64> {
        self.instance
            .get_memory(&mut self.store, "memory")
            .map(|m| m.data_size(&self.store) as u64)
    }

    /// Inject an [`IpcDispatcher`] so `host::invoke_command` can route
    /// calls to other loaded plugins. Called by the loader after all
    /// plugins are loaded or after a hot-reload.
    pub fn set_ipc_dispatcher(&mut self, dispatcher: Arc<dyn IpcDispatcher>) {
        self.store.data_mut().ipc_dispatch = Some(dispatcher);
    }

    /// Inject a [`PluginEventForwarder`] so `host::emit_event` can
    /// surface events to the Tauri frontend in addition to the kernel
    /// bus.
    pub fn set_event_forwarder(&mut self, forwarder: Arc<dyn PluginEventForwarder>) {
        self.store.data_mut().event_forwarder = Some(forwarder);
    }

    /// Dispatch a call to handler `handler_id` with JSON `args`.
    ///
    /// The call protocol:
    /// 1. Serialise `args` to UTF-8 JSON bytes.
    /// 2. Allocate space in WASM linear memory via `nexus_alloc(len)`.
    /// 3. Copy the JSON bytes into that allocation.
    /// 4. Call `nexus_dispatch(handler_id, ptr, len)` → `u64`.
    /// 5. Unpack the high 32 bits as `result_ptr` and low 32 bits as
    ///    `result_len`.
    /// 6. Read `result_len` bytes from `result_ptr` in WASM memory.
    /// 7. Deserialise the result as JSON.
    ///
    /// # Errors
    /// - [`PluginError::ExecutionTimeout`] when fuel is exhausted.
    /// - [`PluginError::ExecutionFailed`] for any other trap or serialisation
    ///   error.
    pub fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_id = self.store.data().plugin_id.clone();

        // Reset the fuel budget for this call. Without this a long-lived
        // plugin that accumulates instruction usage across dispatches
        // eventually returns `OutOfFuel` on every call.
        if self.fuel_per_call > 0 {
            self.store
                .set_fuel(self.fuel_per_call)
                .map_err(|e| PluginError::ExecutionFailed {
                    plugin_id: plugin_id.clone(),
                    reason: format!("set_fuel failed: {e}"),
                })?;
        }

        // Arm the wall-clock deadline watcher. The spawned thread sleeps for
        // max_execution_ms then increments the epoch once, which wasmtime
        // converts to Trap::Interrupt inside the dispatch call. An AtomicBool
        // cancels the increment if dispatch returns before the deadline.
        let watcher_guard = if self.max_execution_ms > 0 {
            self.store.set_epoch_deadline(1);
            let cancelled = Arc::new(AtomicBool::new(false));
            let cancelled_clone = Arc::clone(&cancelled);
            let engine_clone = self.engine.clone();
            let timeout_ms = self.max_execution_ms;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(timeout_ms));
                if !cancelled_clone.load(Ordering::Relaxed) {
                    engine_clone.increment_epoch();
                }
            });
            Some(cancelled)
        } else {
            None
        };

        // Locate exports.
        let nexus_dispatch = self
            .instance
            .get_typed_func::<(u32, u32, u32), u64>(&mut self.store, "nexus_dispatch")
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("nexus_dispatch export not found: {e}"),
            })?;

        let nexus_alloc = self
            .instance
            .get_typed_func::<u32, u32>(&mut self.store, "nexus_alloc")
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("nexus_alloc export not found: {e}"),
            })?;

        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .ok_or_else(|| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: "WASM module has no 'memory' export".to_string(),
            })?;

        // Serialise args.
        let args_bytes = serde_json::to_vec(args).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("args serialisation failed: {e}"),
        })?;
        let args_len =
            u32::try_from(args_bytes.len()).map_err(|_| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: "args JSON too large for WASM (> 4 GiB)".to_string(),
            })?;

        // Allocate in WASM memory.
        let args_ptr = nexus_alloc
            .call(&mut self.store, args_len)
            .map_err(|e| map_trap_error(&e, &plugin_id))?;

        // Copy JSON bytes into WASM memory.
        memory
            .write(&mut self.store, args_ptr as usize, &args_bytes)
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("write to WASM memory failed: {e}"),
            })?;

        // Call dispatch.
        let dispatch_result = nexus_dispatch
            .call(&mut self.store, (handler_id, args_ptr, args_len))
            .map_err(|e| map_trap_error(&e, &plugin_id));

        // Cancel the epoch watcher so a late increment_epoch doesn't trip
        // the next dispatch call.
        if let Some(cancelled) = watcher_guard {
            cancelled.store(true, Ordering::Relaxed);
        }

        // C86 / #439 — sample resource usage regardless of outcome, so an
        // `OutOfFuel` trap (the case this metric matters most for) still
        // gets recorded rather than skipped by an early `?` return below.
        if let Some(m) = nexus_kernel::metrics::global() {
            if let Some(fuel) = self.fuel_consumed_last_call() {
                m.record_plugin_fuel_consumed(&plugin_id, fuel);
            }
            m.record_plugin_memory_bytes(&plugin_id, memory.data_size(&self.store) as u64);
        }

        let ret = dispatch_result?;

        // Unpack packed pointer+length.
        let result_ptr = (ret >> 32) as u32;
        let result_len = (ret & 0xFFFF_FFFF) as u32;

        // Read result from WASM memory.
        let mut result_bytes = vec![0u8; result_len as usize];
        memory
            .read(&self.store, result_ptr as usize, &mut result_bytes)
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("read from WASM memory failed: {e}"),
            })?;

        // Deserialise JSON.
        serde_json::from_slice(&result_bytes).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("result deserialisation failed: {e}"),
        })
    }

    /// Call the plugin's `on_init` lifecycle hook (handler id 0).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_init(&mut self) -> Result<(), PluginError> {
        self.dispatch(0, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_init"))
    }

    /// Call the plugin's `on_start` lifecycle hook (handler id 1).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_start(&mut self) -> Result<(), PluginError> {
        self.dispatch(1, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_start"))
    }

    /// Call the plugin's `on_stop` lifecycle hook (handler id 2).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_stop(&mut self) -> Result<(), PluginError> {
        self.dispatch(2, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_stop"))
    }

    /// Call the plugin's `on_load` lifecycle hook (handler id 3).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_load(&mut self) -> Result<(), PluginError> {
        self.dispatch(3, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_load"))
    }

    /// Call the plugin's `on_enable` lifecycle hook (handler id 4).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_enable(&mut self) -> Result<(), PluginError> {
        self.dispatch(4, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_enable"))
    }

    /// Call the plugin's `on_disable` lifecycle hook (handler id 5).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_disable(&mut self) -> Result<(), PluginError> {
        self.dispatch(5, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_disable"))
    }

    /// Call the plugin's `on_unload` lifecycle hook (handler id 6).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_unload(&mut self) -> Result<(), PluginError> {
        self.dispatch(6, &serde_json::json!({}))
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_unload"))
    }

    /// Call the plugin's `on_settings_changed` lifecycle hook (handler id 7).
    ///
    /// # Errors
    /// Propagates errors from [`dispatch`](WasmSandbox::dispatch), remapped to
    /// [`PluginError::LifecycleError`].
    pub fn call_on_settings_changed(
        &mut self,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        self.dispatch(7, settings)
            .map(|_| ())
            .map_err(|e| to_lifecycle_error(e, "on_settings_changed"))
    }

    /// Return an immutable reference to the [`PluginData`] stored inside the
    /// wasmtime [`Store`].
    #[must_use]
    pub fn plugin_data(&self) -> &PluginData {
        self.store.data()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Map a wasmtime call error to the appropriate [`PluginError`] variant.
///
/// Fuel-exhaustion (`OutOfFuel`) and epoch-deadline (`Interrupt`) traps both
/// map to [`PluginError::ExecutionTimeout`]; everything else becomes
/// [`PluginError::ExecutionFailed`].
fn map_trap_error(err: &wasmtime::Error, plugin_id: &str) -> PluginError {
    let is_timeout = err
        .downcast_ref::<Trap>()
        .is_some_and(|t| *t == Trap::OutOfFuel || *t == Trap::Interrupt);
    if is_timeout {
        PluginError::ExecutionTimeout {
            plugin_id: plugin_id.to_string(),
        }
    } else {
        PluginError::ExecutionFailed {
            plugin_id: plugin_id.to_string(),
            reason: err.to_string(),
        }
    }
}

/// Convert an execution-level error into a lifecycle-specific error.
///
/// Timeout and execution errors are wrapped so the hook name is preserved.
fn to_lifecycle_error(err: PluginError, hook: &str) -> PluginError {
    match err {
        // Already the right shape — keep it.
        PluginError::LifecycleError { .. } => err,
        other => PluginError::LifecycleError {
            plugin_id: other.to_string(), // will be overwritten below
            hook: hook.to_string(),
            reason: other.to_string(),
        },
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn network_policy_default_is_closed() {
        let p = NetworkPolicy::default();
        assert!(!p.enabled);
        assert!(p.allowed_hosts.is_empty());
        assert_eq!(p.max_response_bytes, 10 * 1024 * 1024);
        assert_eq!(p.timeout_ms, 30_000);
    }

    #[test]
    fn network_policy_validate_rejects_when_disabled() {
        let p = NetworkPolicy::default();
        let err = p.validate("GET", "https://api.example.com/x").unwrap_err();
        assert!(err.contains("disabled"), "got: {err}");
    }

    #[test]
    fn network_policy_validate_rejects_unsupported_method() {
        let p = NetworkPolicy {
            enabled: true,
            allowed_hosts: vec!["api.example.com".to_string()],
            ..Default::default()
        };
        let err = p.validate("CONNECT", "https://api.example.com/x").unwrap_err();
        assert!(err.contains("unsupported method"), "got: {err}");
    }

    #[test]
    fn network_policy_validate_rejects_non_https() {
        let p = NetworkPolicy {
            enabled: true,
            allowed_hosts: vec!["api.example.com".to_string()],
            ..Default::default()
        };
        let err = p.validate("GET", "http://api.example.com/x").unwrap_err();
        assert!(err.contains("https"), "got: {err}");
    }

    #[test]
    fn network_policy_validate_rejects_host_off_allowlist() {
        let p = NetworkPolicy {
            enabled: true,
            allowed_hosts: vec!["good.example.com".to_string()],
            ..Default::default()
        };
        let err = p.validate("GET", "https://evil.example.com/x").unwrap_err();
        assert!(err.contains("not on the http allowlist"), "got: {err}");
    }

    #[test]
    fn network_policy_validate_accepts_allowlisted_https() {
        let p = NetworkPolicy {
            enabled: true,
            allowed_hosts: vec!["api.example.com".to_string()],
            ..Default::default()
        };
        let (method, url) = p.validate("get", "https://api.example.com/x").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(url.host_str(), Some("api.example.com"));
    }

    #[test]
    fn plugin_data_default_network_policy_is_closed() {
        let pd = PluginData::default();
        assert!(!pd.network_policy.enabled);
    }

    #[test]
    fn plugin_data_stores_id_and_capabilities() {
        let pd = PluginData {
            plugin_id: "com.test.plugin".to_string(),
            capabilities: CapabilitySet::empty(),
            ..Default::default()
        };
        assert_eq!(pd.plugin_id, "com.test.plugin");
        // CapabilitySet::empty() has no capabilities granted.
        assert!(!pd.capabilities.contains(nexus_kernel::Capability::FsRead));
    }

    #[test]
    fn sandbox_rejects_invalid_wasm() {
        let config = WasmConfig {
            module: "test.wasm".to_string(),
            memory_mb: 16,
            fuel: 10_000_000,
            max_execution_ms: 5_000,
        };
        let pd = PluginData {
            plugin_id: "com.test.invalid".to_string(),
            ..Default::default()
        };
        let result = WasmSandbox::new(b"not valid wasm", &config, pd);
        assert!(
            matches!(result, Err(PluginError::WasmLoadFailed { .. })),
            "expected WasmLoadFailed, got: {result:?}"
        );
    }

    // ── Integration tests (require minimal-plugin.wasm) ───────────────────────

    fn test_wasm_bytes() -> Vec<u8> {
        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::read(&wasm_path).expect("minimal-plugin.wasm must exist — run Task 9 first")
    }

    fn test_config() -> WasmConfig {
        WasmConfig {
            module: "test.wasm".to_string(),
            memory_mb: 16,
            fuel: 10_000_000,
            max_execution_ms: 5_000,
        }
    }

    fn test_plugin_data() -> PluginData {
        PluginData {
            plugin_id: "com.test.minimal".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn sandbox_loads_valid_wasm() {
        let bytes = test_wasm_bytes();
        let result = WasmSandbox::new(&bytes, &test_config(), test_plugin_data());
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn sandbox_dispatch_echo_handler() {
        let bytes = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        let args = serde_json::json!({"hello": "world"});
        let result = sandbox.dispatch(100, &args).unwrap();
        assert_eq!(
            result, args,
            "echo handler should return the input unchanged"
        );
    }

    #[test]
    fn sandbox_lifecycle_hooks_succeed() {
        let bytes = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        sandbox.call_on_init().unwrap();
        sandbox.call_on_start().unwrap();
        sandbox.call_on_stop().unwrap();
    }

    #[test]
    fn sandbox_unknown_handler_returns_error_json() {
        let bytes = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        let result = sandbox.dispatch(999, &serde_json::json!({})).unwrap();
        assert!(
            result.get("error").is_some(),
            "expected JSON with 'error' key, got: {result}"
        );
    }

    #[test]
    fn sandbox_plugin_data_accessible() {
        let bytes = test_wasm_bytes();
        let sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        assert_eq!(sandbox.plugin_data().plugin_id, "com.test.minimal");
    }

    // ── Resource metrics (C86 / #439) ───────────────────────────────────────

    #[test]
    fn sandbox_reports_no_fuel_consumption_before_any_dispatch() {
        // Nothing has run yet, so remaining == budget == 0 consumed.
        let bytes = test_wasm_bytes();
        let sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        assert_eq!(sandbox.fuel_consumed_last_call(), Some(0));
    }

    #[test]
    fn sandbox_records_fuel_consumed_after_dispatch() {
        let bytes = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        sandbox.dispatch(100, &serde_json::json!({"hello": "world"})).unwrap();
        let consumed = sandbox
            .fuel_consumed_last_call()
            .expect("fuel metering is enabled in test_config()");
        assert!(consumed > 0, "expected some fuel spent running the echo handler");
        assert!(
            consumed <= test_config().fuel,
            "consumed ({consumed}) must not exceed the per-call budget"
        );
    }

    #[test]
    fn sandbox_fuel_consumed_is_none_when_metering_disabled() {
        let bytes = test_wasm_bytes();
        let config = WasmConfig {
            fuel: 0,
            ..test_config()
        };
        let mut sandbox = WasmSandbox::new(&bytes, &config, test_plugin_data()).unwrap();
        sandbox.dispatch(100, &serde_json::json!({})).unwrap();
        assert_eq!(
            sandbox.fuel_consumed_last_call(),
            None,
            "fuel=0 in the manifest means metering is off, not zero consumption"
        );
    }

    #[test]
    fn sandbox_reports_nonzero_memory_size() {
        let bytes = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        // A freshly-instantiated module already has at least one memory
        // page (64 KiB) before any dispatch.
        let before = sandbox.memory_size_bytes().expect("module exports memory");
        assert!(before > 0);

        sandbox.dispatch(100, &serde_json::json!({"hello": "world"})).unwrap();
        let after = sandbox.memory_size_bytes().expect("module exports memory");
        assert!(after > 0);
    }
}
