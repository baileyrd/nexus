//! Host function registration for the Nexus plugin WASM sandbox.
//!
//! Registers all `("host", "*")` functions onto a wasmtime [`Linker`] so that
//! WASM plugins can call back into the host environment.

use wasmtime::{Caller, Linker};

use crate::{sandbox::PluginData, PluginError};

// ─── Error-code constants ─────────────────────────────────────────────────────

/// Returned by a host function when the call succeeded.
pub const HOST_OK: i32 = 0;

/// Returned by a host function when the call failed for an unspecified reason.
pub const HOST_ERROR: i32 = -1;

/// Returned when the plugin does not hold the required capability.
// Will be used when KV / event host functions are added (Tasks 14+).
#[allow(dead_code)]
pub const HOST_CAPABILITY_DENIED: i32 = -1001;

/// Returned when the output buffer supplied by the plugin is too small.
// Will be used when KV / event host functions are added (Tasks 14+).
#[allow(dead_code)]
pub const HOST_BUFFER_OVERFLOW: i32 = -1002;

// ─── Registration ─────────────────────────────────────────────────────────────

/// Register all host functions on `linker`.
///
/// Currently registers:
/// - `host::log` — write a log message at the given severity level.
///
/// # Errors
/// Returns [`PluginError::WasmLoadFailed`] (with a synthetic plugin id) if
/// wasmtime rejects a function definition.
pub fn register_host_fns(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    register_host_log(linker)?;
    Ok(())
}

// ─── host::log ────────────────────────────────────────────────────────────────

fn register_host_log(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "log",
            |mut caller: Caller<'_, PluginData>, level: i32, msg_ptr: i32, msg_len: i32| -> i32 {
                let plugin_id = caller.data().plugin_id.clone();

                // Resolve the WASM linear memory export.
                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    tracing::error!(
                        plugin_id = %plugin_id,
                        "host::log: WASM module has no 'memory' export"
                    );
                    return HOST_ERROR;
                };

                // Bounds-check the slice. Reject negative ptr/len values.
                let Ok(start) = usize::try_from(msg_ptr) else {
                    tracing::error!(plugin_id = %plugin_id, "host::log: negative msg_ptr");
                    return HOST_ERROR;
                };
                let Ok(len) = usize::try_from(msg_len) else {
                    tracing::error!(plugin_id = %plugin_id, "host::log: negative msg_len");
                    return HOST_ERROR;
                };
                let mem_data = memory.data(&caller);
                let end = match start.checked_add(len) {
                    Some(e) if e <= mem_data.len() => e,
                    _ => {
                        tracing::error!(
                            plugin_id = %plugin_id,
                            "host::log: msg_ptr/msg_len out of bounds"
                        );
                        return HOST_ERROR;
                    }
                };

                let bytes = &mem_data[start..end];

                // Convert to UTF-8 (lossy so a bad plugin cannot panic the host).
                let msg = String::from_utf8_lossy(bytes);

                match level {
                    0 => tracing::debug!(plugin_id = %plugin_id, "{}", msg),
                    1 => tracing::info!(plugin_id = %plugin_id, "{}", msg),
                    2 => tracing::warn!(plugin_id = %plugin_id, "{}", msg),
                    _ => tracing::error!(plugin_id = %plugin_id, "{}", msg),
                }

                HOST_OK
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::log: {e}"),
        })?;

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_distinct() {
        assert_ne!(HOST_OK, HOST_ERROR);
        assert_ne!(HOST_OK, HOST_CAPABILITY_DENIED);
        assert_ne!(HOST_OK, HOST_BUFFER_OVERFLOW);
        assert_ne!(HOST_ERROR, HOST_CAPABILITY_DENIED);
    }
}
