//! WASM sandbox: wraps a single plugin's wasmtime Engine/Module/Store/Instance
//! and provides the `dispatch` call used by all higher-level plugin code.

use wasmtime::{Engine, Instance, Linker, Module, Store, Trap};

use nexus_kernel::CapabilitySet;

use crate::{PluginError, WasmConfig};

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

        let engine = Engine::new(&wt_config).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("engine creation failed: {e}"),
        })?;

        let module = Module::new(&engine, wasm_bytes).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("module compilation failed: {e}"),
        })?;

        let mut store = Store::new(&engine, plugin_data);

        if config.fuel > 0 {
            store
                .set_fuel(config.fuel)
                .map_err(|e| PluginError::WasmLoadFailed {
                    plugin_id: plugin_id.clone(),
                    reason: format!("set_fuel failed: {e}"),
                })?;
        }

        let mut linker: Linker<PluginData> = Linker::new(&engine);
        crate::host_fns::register_host_fns(&mut linker).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("host function registration failed: {e}"),
        })?;

        let instance =
            linker
                .instantiate(&mut store, &module)
                .map_err(|e| PluginError::WasmLoadFailed {
                    plugin_id: plugin_id.clone(),
                    reason: format!("instantiation failed: {e}"),
                })?;

        Ok(Self { store, instance })
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
        let args_len = u32::try_from(args_bytes.len()).map_err(|_| PluginError::ExecutionFailed {
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
        let ret = nexus_dispatch
            .call(&mut self.store, (handler_id, args_ptr, args_len))
            .map_err(|e| map_trap_error(&e, &plugin_id))?;

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
/// Fuel-exhaustion traps become [`PluginError::ExecutionTimeout`]; everything
/// else becomes [`PluginError::ExecutionFailed`].
fn map_trap_error(err: &wasmtime::Error, plugin_id: &str) -> PluginError {
    if err
        .downcast_ref::<Trap>()
        .is_some_and(|t| *t == Trap::OutOfFuel)
    {
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
    fn plugin_data_stores_id_and_capabilities() {
        let pd = PluginData {
            plugin_id: "com.test.plugin".to_string(),
            capabilities: CapabilitySet::empty(),
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
        };
        let pd = PluginData {
            plugin_id: "com.test.invalid".to_string(),
            capabilities: CapabilitySet::empty(),
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
        }
    }

    fn test_plugin_data() -> PluginData {
        PluginData {
            plugin_id: "com.test.minimal".to_string(),
            capabilities: CapabilitySet::empty(),
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
        let mut sandbox =
            WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        let args = serde_json::json!({"hello": "world"});
        let result = sandbox.dispatch(100, &args).unwrap();
        assert_eq!(result, args, "echo handler should return the input unchanged");
    }

    #[test]
    fn sandbox_lifecycle_hooks_succeed() {
        let bytes = test_wasm_bytes();
        let mut sandbox =
            WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        sandbox.call_on_init().unwrap();
        sandbox.call_on_start().unwrap();
        sandbox.call_on_stop().unwrap();
    }

    #[test]
    fn sandbox_unknown_handler_returns_error_json() {
        let bytes = test_wasm_bytes();
        let mut sandbox =
            WasmSandbox::new(&bytes, &test_config(), test_plugin_data()).unwrap();
        let result = sandbox
            .dispatch(999, &serde_json::json!({}))
            .unwrap();
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
}
