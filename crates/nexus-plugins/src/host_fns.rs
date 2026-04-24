//! Host function registration for the Nexus plugin WASM sandbox.
//!
//! Registers all `("host", "*")` functions onto a wasmtime [`Linker`] so that
//! WASM plugins can call back into the host environment.

use std::path::Path;

use wasmtime::{Caller, Linker};

use nexus_kernel::Capability;
use nexus_types::PathValidationError;

use crate::{sandbox::PluginData, PluginError};

// ─── Error-code constants ─────────────────────────────────────────────────────

/// Returned by a host function when the call succeeded.
pub const HOST_OK: i32 = 0;

/// Returned by a host function when the call failed for an unspecified reason.
pub const HOST_ERROR: i32 = -1;

/// Returned when the plugin does not hold the required capability.
pub const HOST_CAPABILITY_DENIED: i32 = -1001;

/// Returned when the output buffer supplied by the plugin is too small.
pub const HOST_BUFFER_OVERFLOW: i32 = -1002;

// ─── Audit helpers ────────────────────────────────────────────────────────────

fn deny_capability(plugin_id: &str, capability: &str) -> i32 {
    tracing::warn!(
        audit = true,
        plugin_id,
        capability,
        result = "denied",
        "capability denied"
    );
    HOST_CAPABILITY_DENIED
}

fn deny_path_traversal(plugin_id: &str, requested_path: &Path, forge_root: &Path) -> i32 {
    tracing::warn!(
        audit = true,
        plugin_id,
        requested_path = %requested_path.display(),
        forge_root = %forge_root.display(),
        "path traversal denied"
    );
    HOST_CAPABILITY_DENIED
}

// ─── Registration ─────────────────────────────────────────────────────────────

/// Register all host functions on `linker`.
///
/// Registers:
/// - `host::log` — write a log message at the given severity level.
/// - `host::kv_get` — read a value from the plugin's KV namespace.
/// - `host::kv_set` — write a value to the plugin's KV namespace.
/// - `host::emit_event` — publish a custom event to the kernel event bus.
/// - `host::read_file` — read a file from within the plugin's forge root.
/// - `host::notify` — show an in-app toast notification in the UI.
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
    register_host_get_settings(linker)?;
    register_host_notify(linker)?;
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

                // Rate-limit: drop log lines once the plugin exceeds its
                // token bucket (default 1000 lines/sec sustained, 2000 burst).
                // Dropped lines are counted and reported once on the next
                // successful emission so operators can see suppression
                // without paying the per-line cost of the dropped work.
                let mut suppressed: u64 = 0;
                let rate = caller.data().log_rate.clone();
                if let Ok(mut bucket) = rate.lock() {
                    if !bucket.try_consume() {
                        return HOST_OK;
                    }
                    suppressed = std::mem::take(&mut bucket.denied_since_last);
                }
                if suppressed > 0 {
                    tracing::warn!(
                        audit = true,
                        plugin_id = %plugin_id,
                        suppressed,
                        "host::log rate limit dropped messages"
                    );
                }

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
                    return deny_capability(&plugin_id, "kv.read");
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
                    return deny_capability(&plugin_id, "kv.write");
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
/// Requires the `events.publish` capability.
///
/// Returns `HOST_OK` on success, `HOST_CAPABILITY_DENIED` if the plugin lacks
/// `events.publish`, or `HOST_ERROR` on any other failure.
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

                if !caller.data().capabilities.contains(Capability::EventsPublish) {
                    return deny_capability(&plugin_id, "events.publish");
                }

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

                match event_bus.publish_plugin(&plugin_id, &type_id, payload.clone()) {
                    Ok(()) => {
                        // Also forward to the Tauri frontend so the UI sees
                        // events published mid-handler, not just via the
                        // `events` return-array path.
                        if let Some(fwd) = caller.data().event_forwarder.clone() {
                            fwd.forward(&plugin_id, &type_id, &payload);
                        }
                        HOST_OK
                    }
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
                    return deny_capability(&plugin_id, "fs.write");
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

                let requested = Path::new(&path_str);

                // Confine path to forge root via `ForgePathValidator::
                // validate_for_write`. The validator walks up to the
                // deepest existing ancestor, canonicalizes *that*
                // (resolving symlinks in one syscall), prefix-checks the
                // canonical ancestor against the canonical forge root,
                // and rejoins the remaining tail. This closes the
                // canonicalize-parent-then-open TOCTOU race the prior
                // inline pattern was vulnerable to (MK audit finding
                // F-5.3.1). Test sandboxes with an empty `forge_root`
                // and no validator skip the check — they operate on an
                // out-of-tree scratch path chosen by the test.
                let target = if forge_root.as_os_str().is_empty() {
                    if requested.is_absolute() {
                        requested.to_path_buf()
                    } else {
                        forge_root.join(requested)
                    }
                } else {
                    let Some(validator) = caller.data().path_validator.as_ref() else {
                        tracing::warn!(
                            plugin_id = %plugin_id,
                            "host::write_file: no path validator configured for plugin — denying"
                        );
                        return HOST_ERROR;
                    };
                    // Ensure the target's (normalized) parent directory
                    // exists before validation — `validate_for_write`
                    // only accepts paths whose deepest existing ancestor
                    // lies inside the forge root, but it handles the
                    // case where intermediate directories don't exist
                    // by canonicalizing the deepest real ancestor.
                    // However, the writer (`std::fs::write`) will not
                    // mkdir, so if any non-existing intermediate
                    // directory is present we must create it first —
                    // and only under the canonical target chain so a
                    // symlinked segment cannot steer mkdir outside the
                    // sandbox.
                    match validator.validate_for_write(requested) {
                        Ok(canonical_target) => {
                            if let Some(parent) = canonical_target.parent() {
                                if !parent.exists() {
                                    if let Err(e) = std::fs::create_dir_all(parent) {
                                        tracing::warn!(
                                            plugin_id = %plugin_id,
                                            "host::write_file: mkdir failed: {e}"
                                        );
                                        return HOST_ERROR;
                                    }
                                }
                            }
                            canonical_target
                        }
                        Err(PathValidationError::PathTraversal(_)) => {
                            return deny_path_traversal(&plugin_id, requested, &forge_root);
                        }
                        Err(PathValidationError::InvalidPath(msg)) => {
                            tracing::warn!(
                                plugin_id = %plugin_id,
                                "host::write_file: invalid path '{}': {msg}",
                                requested.display()
                            );
                            return HOST_ERROR;
                        }
                    }
                };

                match std::fs::write(&target, &data) {
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
/// Plugin-to-plugin IPC. Dispatches a command to another loaded plugin
/// via the [`IpcDispatcher`] injected into [`PluginData`] during
/// bootstrap.
///
/// Requires `IpcCall` capability. Returns bytes written on success,
/// `HOST_BUFFER_OVERFLOW` when the JSON response exceeds `out_cap`,
/// `HOST_CAPABILITY_DENIED` when the caller lacks `ipc.call`, or
/// `HOST_ERROR` on any other failure (target not found, command not
/// found, dispatch error, dispatcher not injected).
fn register_host_invoke_command(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "invoke_command",
            |mut caller: Caller<'_, PluginData>,
             plugin_id_ptr: i32,
             plugin_id_len: i32,
             cmd_ptr: i32,
             cmd_len: i32,
             args_ptr: i32,
             args_len: i32,
             out_ptr: i32,
             out_cap: i32|
             -> i32 {
                let caller_plugin_id = caller.data().plugin_id.clone();

                if !caller.data().capabilities.contains(Capability::IpcCall) {
                    return deny_capability(&caller_plugin_id, "ipc.call");
                }

                // Get WASM memory.
                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };

                // Read target plugin ID, command ID, and args from WASM memory.
                let Some(target_plugin_id) = read_wasm_str(&memory, &caller, plugin_id_ptr, plugin_id_len) else {
                    tracing::warn!(plugin_id = %caller_plugin_id, "host::invoke_command: invalid target_plugin_id");
                    return HOST_ERROR;
                };
                let Some(command_id) = read_wasm_str(&memory, &caller, cmd_ptr, cmd_len) else {
                    tracing::warn!(plugin_id = %caller_plugin_id, "host::invoke_command: invalid command_id");
                    return HOST_ERROR;
                };
                let args: serde_json::Value = if args_len == 0 {
                    serde_json::Value::Null
                } else {
                    let Some(args_bytes) = read_wasm_bytes(&memory, &caller, args_ptr, args_len) else {
                        tracing::warn!(plugin_id = %caller_plugin_id, "host::invoke_command: invalid args");
                        return HOST_ERROR;
                    };
                    match serde_json::from_slice(&args_bytes) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(plugin_id = %caller_plugin_id, "host::invoke_command: invalid args JSON: {e}");
                            return HOST_ERROR;
                        }
                    }
                };

                // Get the injected IPC dispatcher.
                let dispatcher = match caller.data().ipc_dispatch.clone() {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            plugin_id = %caller_plugin_id,
                            "host::invoke_command: IPC dispatcher not injected"
                        );
                        return HOST_ERROR;
                    }
                };

                // Dispatch to target plugin.
                let result = match dispatcher.dispatch(&target_plugin_id, &command_id, &args) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            caller = %caller_plugin_id,
                            target = %target_plugin_id,
                            command = %command_id,
                            "host::invoke_command: dispatch failed: {e}"
                        );
                        return HOST_ERROR;
                    }
                };

                // Serialize result and write to output buffer.
                let result_bytes = match serde_json::to_vec(&result) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(plugin_id = %caller_plugin_id, "host::invoke_command: result serialization failed: {e}");
                        return HOST_ERROR;
                    }
                };

                let Ok(o_start) = usize::try_from(out_ptr) else { return HOST_ERROR; };
                let Ok(o_cap) = usize::try_from(out_cap) else { return HOST_ERROR; };
                if result_bytes.len() > o_cap {
                    return HOST_BUFFER_OVERFLOW;
                }
                let end = o_start + result_bytes.len();
                let mem_data = memory.data_mut(&mut caller);
                if end > mem_data.len() {
                    return HOST_ERROR;
                }
                mem_data[o_start..end].copy_from_slice(&result_bytes);
                i32::try_from(result_bytes.len()).unwrap_or(HOST_ERROR)
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
                    return deny_capability(&plugin_id, "fs.read");
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
                    return deny_path_traversal(&plugin_id, &canonical, &forge_root);
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

// ─── host::get_settings ──────────────────────────────────────────────────────

/// `host::get_settings(out_ptr, out_cap) -> i32`
///
/// Writes the plugin's current validated settings (pretty-printed JSON
/// UTF-8) into `[out_ptr, out_ptr+out_cap)`.
///
/// Returns the number of bytes written on success, `HOST_BUFFER_OVERFLOW`
/// when the JSON is larger than `out_cap`, or `HOST_ERROR` on any other
/// failure. Plugins without a registered schema still get a valid
/// response — the loader seeds the cache with `"{}"`, so the call
/// returns `2` and the empty-object bytes.
///
/// No capability gate today — a plugin's own settings are considered
/// first-party. If that becomes a privacy concern (e.g. secrets stored
/// alongside prefs) a future revision can add a `settings.read`
/// capability and enforce it here.
fn register_host_get_settings(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "get_settings",
            |mut caller: Caller<'_, PluginData>,
             out_ptr: i32,
             out_cap: i32|
             -> i32 {
                let plugin_id = caller.data().plugin_id.clone();
                let cache = caller.data().settings_json.clone();
                let Some(cache) = cache else {
                    tracing::warn!(plugin_id = %plugin_id, "host::get_settings: cache not injected");
                    return HOST_ERROR;
                };

                let Ok(guard) = cache.read() else {
                    tracing::warn!(plugin_id = %plugin_id, "host::get_settings: cache poisoned");
                    return HOST_ERROR;
                };
                let bytes = guard.as_bytes().to_vec();
                drop(guard);

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Ok(o_start) = usize::try_from(out_ptr) else { return HOST_ERROR; };
                let Ok(o_cap) = usize::try_from(out_cap) else { return HOST_ERROR; };
                if bytes.len() > o_cap {
                    return HOST_BUFFER_OVERFLOW;
                }
                let end = o_start + bytes.len();
                let mem_data = memory.data_mut(&mut caller);
                if end > mem_data.len() {
                    return HOST_ERROR;
                }
                mem_data[o_start..end].copy_from_slice(&bytes);
                i32::try_from(bytes.len()).unwrap_or(HOST_ERROR)
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::get_settings: {e}"),
        })?;
    Ok(())
}

// ─── host::notify ─────────────────────────────────────────────────────────────

/// `host::notify(level: i32, msg_ptr: i32, msg_len: i32) -> i32`
///
/// Shows an in-app toast notification in the Nexus UI. The event is
/// forwarded to the frontend via the [`PluginEventForwarder`] as a
/// `plugin:event` with topic `"ui.notification"` and payload
/// `{ "level": "info"|"warn"|"error", "message": "<text>" }`.
///
/// `level`: 0 = info, 1 = warn, 2 = error.
///
/// Requires the `ui.notify` capability.
///
/// Returns `HOST_OK` on success, `HOST_CAPABILITY_DENIED` if the plugin lacks
/// `ui.notify`, or `HOST_ERROR` when the forwarder is not injected or the
/// message is invalid UTF-8.
fn register_host_notify(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "notify",
            |mut caller: Caller<'_, PluginData>, level: i32, msg_ptr: i32, msg_len: i32| -> i32 {
                let plugin_id = caller.data().plugin_id.clone();

                if !caller.data().capabilities.contains(Capability::UiNotify) {
                    return deny_capability(&plugin_id, "ui.notify");
                }

                let forwarder = caller.data().event_forwarder.clone();
                let Some(forwarder) = forwarder else {
                    tracing::warn!(plugin_id = %plugin_id, "host::notify: event forwarder not injected");
                    return HOST_ERROR;
                };

                let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                    return HOST_ERROR;
                };
                let Some(message) = read_wasm_str(&memory, &caller, msg_ptr, msg_len) else {
                    tracing::warn!(plugin_id = %plugin_id, "host::notify: invalid UTF-8 message");
                    return HOST_ERROR;
                };

                let level_str = match level {
                    1 => "warn",
                    2 => "error",
                    _ => "info",
                };

                let payload = serde_json::json!({
                    "level": level_str,
                    "message": message,
                });

                forwarder.forward(&plugin_id, "ui.notification", &payload);
                HOST_OK
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: "<host>".to_string(),
            reason: format!("failed to register host::notify: {e}"),
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
