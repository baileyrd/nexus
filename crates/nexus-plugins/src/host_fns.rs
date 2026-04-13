//! Host function registration for the Nexus plugin WASM sandbox.
//!
//! Registers all `("host", "*")` functions onto a wasmtime [`Linker`] so that
//! WASM plugins can call back into the host environment.

use std::path::Path;

use wasmtime::{Caller, Linker};

use nexus_kernel::Capability;

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
/// Registers:
/// - `host::log` — write a log message at the given severity level.
/// - `host::kv_get` — read a value from the plugin's KV namespace.
/// - `host::kv_set` — write a value to the plugin's KV namespace.
/// - `host::emit_event` — publish a custom event to the kernel event bus.
/// - `host::read_file` — read a file from within the plugin's forge root.
///
/// # Errors
/// Returns [`PluginError::WasmLoadFailed`] (with a synthetic plugin id) if
/// wasmtime rejects a function definition.
pub fn register_host_fns(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    register_host_log(linker)?;
    register_host_kv_get(linker)?;
    register_host_kv_set(linker)?;
    register_host_emit_event(linker)?;
    register_host_write_file(linker)?;
    register_host_read_file(linker)?;
    register_host_invoke_command(linker)?;
    Ok(())
}

// ─── Memory helpers ───────────────────────────────────────────────────────────

/// Copy bytes from WASM linear memory at `[ptr, ptr+len)` into a `Vec<u8>`.
///
/// Returns `None` if the range is out of bounds or `ptr`/`len` are negative.
fn read_wasm_bytes(memory: &wasmtime::Memory, caller: &impl wasmtime::AsContext, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let start = usize::try_from(ptr).ok()?;
    let length = usize::try_from(len).ok()?;
    let data = memory.data(caller);
    let end = start.checked_add(length).filter(|&e| e <= data.len())?;
    Some(data[start..end].to_vec())
}

/// Read a UTF-8 string from WASM linear memory. Returns `None` on any error.
fn read_wasm_str(memory: &wasmtime::Memory, caller: &impl wasmtime::AsContext, ptr: i32, len: i32) -> Option<String> {
    let bytes = read_wasm_bytes(memory, caller, ptr, len)?;
    String::from_utf8(bytes).ok()
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

// ─── host::kv_get ─────────────────────────────────────────────────────────────

/// `host::kv_get(key_ptr, key_len, out_ptr, out_cap) -> i32`
///
/// Reads the value stored under `key` in the plugin's KV namespace and writes
/// it into the WASM buffer `[out_ptr, out_ptr+out_cap)`.
///
/// Returns the number of bytes written on success, `HOST_BUFFER_OVERFLOW` when
/// the value is larger than `out_cap`, or `HOST_ERROR` on any other failure.
/// Returns `0` when the key does not exist.
fn register_host_kv_get(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "kv_get",
            |mut caller: Caller<'_, PluginData>,
             key_ptr: i32,
             key_len: i32,
             out_ptr: i32,
             out_cap: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let kv = caller.data().kv.clone();
                let Some(kv) = kv else {
                    tracing::warn!(plugin_id = %plugin_id, "host::kv_get: kv store not injected");
                    return HOST_ERROR;
                };
                if !caller.data().capabilities.contains(Capability::KvRead) {
                    return HOST_CAPABILITY_DENIED;
                }

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(key) = read_wasm_str(&memory, &caller, key_ptr, key_len) else {
                    return HOST_ERROR;
                };

                let value = match kv.get(&plugin_id, &key) {
                    Ok(Some(v)) => v,
                    Ok(None) => return 0,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::kv_get error: {e}");
                        return HOST_ERROR;
                    }
                };

                let Ok(o_start) = usize::try_from(out_ptr) else { return HOST_ERROR; };
                let Ok(o_cap) = usize::try_from(out_cap) else { return HOST_ERROR; };
                if value.len() > o_cap {
                    return HOST_BUFFER_OVERFLOW;
                }
                let end = o_start + value.len();
                let mem_data = memory.data_mut(&mut caller);
                if end > mem_data.len() {
                    return HOST_ERROR;
                }
                mem_data[o_start..end].copy_from_slice(&value);
                // Safe: value.len() <= o_cap, and o_cap <= i32::MAX (derived from i32 out_cap).
                i32::try_from(value.len()).unwrap_or(HOST_ERROR)
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::kv_get: {e}"),
        })?;
    Ok(())
}

// ─── host::kv_set ─────────────────────────────────────────────────────────────

/// `host::kv_set(key_ptr, key_len, val_ptr, val_len) -> i32`
///
/// Writes `value` under `key` in the plugin's KV namespace.
///
/// Returns `HOST_OK` on success, `HOST_CAPABILITY_DENIED` if the plugin lacks
/// `KvWrite`, or `HOST_ERROR` on any other failure.
fn register_host_kv_set(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "kv_set",
            |mut caller: Caller<'_, PluginData>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let kv = caller.data().kv.clone();
                let Some(kv) = kv else {
                    tracing::warn!(plugin_id = %plugin_id, "host::kv_set: kv store not injected");
                    return HOST_ERROR;
                };
                if !caller.data().capabilities.contains(Capability::KvWrite) {
                    return HOST_CAPABILITY_DENIED;
                }

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(key) = read_wasm_str(&memory, &caller, key_ptr, key_len) else {
                    return HOST_ERROR;
                };
                let Some(value) = read_wasm_bytes(&memory, &caller, val_ptr, val_len) else {
                    return HOST_ERROR;
                };

                match kv.set(&plugin_id, &key, &value) {
                    Ok(()) => HOST_OK,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::kv_set error: {e}");
                        HOST_ERROR
                    }
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::kv_set: {e}"),
        })?;
    Ok(())
}

// ─── host::emit_event ─────────────────────────────────────────────────────────

/// `host::emit_event(type_id_ptr, type_id_len, payload_ptr, payload_len) -> i32`
///
/// Publishes a custom event to the kernel event bus. The `type_id` must be
/// namespaced under the plugin's reverse-DNS ID (e.g. `com.example.plugin.*`).
/// The `payload` must be valid UTF-8 JSON.
///
/// Returns `HOST_OK` on success or `HOST_ERROR` on failure.
fn register_host_emit_event(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "emit_event",
            |mut caller: Caller<'_, PluginData>,
             type_id_ptr: i32,
             type_id_len: i32,
             payload_ptr: i32,
             payload_len: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let event_bus = caller.data().event_bus.clone();
                let Some(event_bus) = event_bus else {
                    tracing::warn!(plugin_id = %plugin_id, "host::emit_event: event bus not injected");
                    return HOST_ERROR;
                };

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(type_id) = read_wasm_str(&memory, &caller, type_id_ptr, type_id_len) else {
                    return HOST_ERROR;
                };
                let Some(payload_bytes) = read_wasm_bytes(&memory, &caller, payload_ptr, payload_len) else {
                    return HOST_ERROR;
                };

                let payload: serde_json::Value = match serde_json::from_slice(&payload_bytes) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::emit_event: invalid JSON payload: {e}");
                        return HOST_ERROR;
                    }
                };

                match event_bus.publish_plugin(&plugin_id, &type_id, payload) {
                    Ok(()) => HOST_OK,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::emit_event error: {e}");
                        HOST_ERROR
                    }
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::emit_event: {e}"),
        })?;
    Ok(())
}

// ─── host::write_file ─────────────────────────────────────────────────────────

/// `host::write_file(path_ptr, path_len, data_ptr, data_len) -> i32`
///
/// Writes `data` to the file at `path` (relative to the plugin's forge root).
/// Parent directories are created if absent.
///
/// Returns `HOST_OK` on success, `HOST_CAPABILITY_DENIED` if the plugin lacks
/// `FsWrite`, or `HOST_ERROR` on any other failure.
fn register_host_write_file(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "write_file",
            |mut caller: Caller<'_, PluginData>,
             path_ptr: i32,
             path_len: i32,
             data_ptr: i32,
             data_len: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let forge_root = caller.data().forge_root.clone();

                if !caller.data().capabilities.contains(Capability::FsWrite) {
                    return HOST_CAPABILITY_DENIED;
                }

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(path_str) = read_wasm_str(&memory, &caller, path_ptr, path_len) else {
                    return HOST_ERROR;
                };
                let Some(data) = read_wasm_bytes(&memory, &caller, data_ptr, data_len) else {
                    return HOST_ERROR;
                };

                // Confine path to forge root (same rules as read_file).
                let requested = Path::new(&path_str);
                let absolute = if requested.is_absolute() {
                    requested.to_path_buf()
                } else {
                    forge_root.join(requested)
                };
                // For writes, the file may not exist yet; check parent directory.
                let parent = absolute.parent().unwrap_or(&absolute);
                if !forge_root.as_os_str().is_empty() {
                    let canon_parent = if let Ok(p) = parent.canonicalize() {
                        p
                    } else {
                        // Parent doesn't exist yet — create it and re-check.
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            tracing::warn!(plugin_id = %plugin_id, "host::write_file: mkdir failed: {e}");
                            return HOST_ERROR;
                        }
                        match parent.canonicalize() {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!(plugin_id = %plugin_id, "host::write_file: canonicalize failed: {e}");
                                return HOST_ERROR;
                            }
                        }
                    };
                    if !canon_parent.starts_with(&forge_root) {
                        tracing::warn!(
                            plugin_id = %plugin_id,
                            "host::write_file: path traversal denied: {}",
                            absolute.display()
                        );
                        return HOST_CAPABILITY_DENIED;
                    }
                }

                match std::fs::write(&absolute, &data) {
                    Ok(()) => HOST_OK,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::write_file: write error: {e}");
                        HOST_ERROR
                    }
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::write_file: {e}"),
        })?;
    Ok(())
}

// ─── host::invoke_command ─────────────────────────────────────────────────────

/// `host::invoke_command(plugin_id_ptr, plugin_id_len, cmd_ptr, cmd_len, args_ptr, args_len, out_ptr, out_cap) -> i32`
///
/// Stub for plugin-to-plugin IPC. Phase 1: always returns `HOST_ERROR` since
/// calling into another plugin sandbox from within WASM execution requires
/// re-entrant access to the `PluginLoader`, which is deferred to Phase 2.
///
/// Requires `IpcCall` capability (denied early so the plugin gets a clear
/// signal rather than a generic error).
fn register_host_invoke_command(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "invoke_command",
            |caller: Caller<'_, PluginData>,
             _plugin_id_ptr: i32,
             _plugin_id_len: i32,
             _cmd_ptr: i32,
             _cmd_len: i32,
             _args_ptr: i32,
             _args_len: i32,
             _out_ptr: i32,
             _out_cap: i32|
             -> i32 {
                if !caller.data().capabilities.contains(Capability::IpcCall) {
                    return HOST_CAPABILITY_DENIED;
                }
                // IPC calls from within WASM are deferred to Phase 2.
                tracing::debug!(
                    plugin_id = %caller.data().plugin_id,
                    "host::invoke_command: inter-plugin IPC not yet available in WASM"
                );
                HOST_ERROR
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::invoke_command: {e}"),
        })?;
    Ok(())
}

// ─── host::read_file ──────────────────────────────────────────────────────────

/// `host::read_file(path_ptr, path_len, out_ptr, out_cap) -> i32`
///
/// Reads a file at `path` (relative to the plugin's forge root) and writes its
/// contents into the WASM buffer `[out_ptr, out_ptr+out_cap)`.
///
/// Returns the number of bytes written on success, `HOST_BUFFER_OVERFLOW` when
/// the file is larger than `out_cap`, `HOST_CAPABILITY_DENIED` if the plugin
/// lacks `FsRead`, or `HOST_ERROR` on any other failure.
fn register_host_read_file(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "read_file",
            |mut caller: Caller<'_, PluginData>,
             path_ptr: i32,
             path_len: i32,
             out_ptr: i32,
             out_cap: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let forge_root = caller.data().forge_root.clone();

                if !caller.data().capabilities.contains(Capability::FsRead) {
                    return HOST_CAPABILITY_DENIED;
                }

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(path_str) = read_wasm_str(&memory, &caller, path_ptr, path_len) else {
                    return HOST_ERROR;
                };

                // Confine path to forge root.
                let requested = Path::new(&path_str);
                let absolute = if requested.is_absolute() {
                    requested.to_path_buf()
                } else {
                    forge_root.join(requested)
                };
                let canonical = match absolute.canonicalize() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::read_file: canonicalize failed: {e}");
                        return HOST_ERROR;
                    }
                };
                // Forge root confinement: forge_root may be empty (test/stub mode),
                // so only enforce the check when forge_root is non-empty.
                if !forge_root.as_os_str().is_empty() && !canonical.starts_with(&forge_root) {
                    tracing::warn!(
                        plugin_id = %plugin_id,
                        "host::read_file: path traversal denied: {}",
                        canonical.display()
                    );
                    return HOST_CAPABILITY_DENIED;
                }

                let contents = match std::fs::read(&canonical) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(plugin_id = %plugin_id, "host::read_file: read error: {e}");
                        return HOST_ERROR;
                    }
                };

                let Ok(o_start) = usize::try_from(out_ptr) else { return HOST_ERROR; };
                let Ok(o_cap) = usize::try_from(out_cap) else { return HOST_ERROR; };
                if contents.len() > o_cap {
                    return HOST_BUFFER_OVERFLOW;
                }
                let end = o_start + contents.len();
                let mem_data = memory.data_mut(&mut caller);
                if end > mem_data.len() {
                    return HOST_ERROR;
                }
                mem_data[o_start..end].copy_from_slice(&contents);
                // Safe: contents.len() <= o_cap, and o_cap <= i32::MAX (derived from i32 out_cap).
                i32::try_from(contents.len()).unwrap_or(HOST_ERROR)
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::read_file: {e}"),
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
