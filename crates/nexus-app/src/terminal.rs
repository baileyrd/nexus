//! Tauri command bridge into the terminal core plugin.
//!
//! Thin adapters: each command serializes its args into JSON and calls
//! [`nexus_kernel::PluginContext::ipc_call`] on
//! `com.nexus.terminal` via the kernel runtime held in Tauri state.
//!
//! State ownership lives in
//! [`nexus_terminal::core_plugin::TerminalCorePlugin`] — registered by
//! [`nexus_bootstrap`]. Mirrors [`crate::editor`]'s shape so the
//! frontend reaches the terminal engine through `invoke("term_…")`
//! calls that land on `ipc_call("com.nexus.terminal", …)` under the
//! hood. No direct `nexus-terminal` dep in `nexus-app`'s public
//! surface beyond this file.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

/// Reverse-DNS id of the terminal core plugin. Duplicated from
/// `nexus-terminal` so `nexus-app` doesn't need that crate as a direct
/// dep — the reverse-DNS string is the stable public contract.
const TERMINAL_PLUGIN_ID: &str = "com.nexus.terminal";

/// Per-call timeout. Pumps are usually tens of ms; 30 s is the outer
/// bound for `wait_for_pattern` which caller-drives its own deadline.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

async fn call_terminal(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(TERMINAL_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Spawn a new PTY-backed session. Returns `{ id: String }`.
#[tauri::command]
pub async fn term_create_session(
    name: Option<String>,
    shell: Option<String>,
    shell_args: Option<Vec<String>>,
    working_dir: Option<String>,
    env: Option<Vec<(String, String)>>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    // Build the args object only with fields the caller actually set so
    // optional defaults at the plugin layer kick in for the rest.
    let mut args = serde_json::Map::new();
    if let Some(v) = name {
        args.insert("name".into(), v.into());
    }
    if let Some(v) = shell {
        args.insert("shell".into(), v.into());
    }
    if let Some(v) = shell_args {
        args.insert("shell_args".into(), serde_json::to_value(v).unwrap());
    }
    if let Some(v) = working_dir {
        args.insert("working_dir".into(), v.into());
    }
    if let Some(v) = env {
        args.insert("env".into(), serde_json::to_value(v).unwrap());
    }
    call_terminal(runtime, "create_session", args.into()).await
}

/// Graceful shutdown via the §5.1 signal ladder.
#[tauri::command]
pub async fn term_close_session(
    id: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(
        runtime,
        "close_session",
        serde_json::json!({ "id": id }),
    )
    .await
}

/// Line-terminated input into the session's stdin.
#[tauri::command]
pub async fn term_send_input(
    id: String,
    input: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(
        runtime,
        "send_input",
        serde_json::json!({ "id": id, "input": input }),
    )
    .await
}

/// Raw bytes (no newline added) — used for control sequences like
/// `0x03` (Ctrl+C), arrow keys, etc.
#[tauri::command]
pub async fn term_send_raw_input(
    id: String,
    data: Vec<u8>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(
        runtime,
        "send_raw_input",
        serde_json::json!({ "id": id, "data": data }),
    )
    .await
}

/// Drain the PTY into the session's line buffer. Returns byte count
/// drained; the frontend reads the line snapshot separately via
/// [`term_read_output`].
#[tauri::command]
pub async fn term_pump(
    id: String,
    timeout_ms: Option<u64>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), id.into());
    if let Some(v) = timeout_ms {
        args.insert("timeout_ms".into(), v.into());
    }
    call_terminal(runtime, "pump", args.into()).await
}

/// Structured line snapshot. `start` + `count` slice the buffer like
/// Python — both clamp to available lines.
#[tauri::command]
pub async fn term_read_output(
    id: String,
    start: Option<usize>,
    count: Option<usize>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), id.into());
    if let Some(v) = start {
        args.insert("start".into(), v.into());
    }
    if let Some(v) = count {
        args.insert("count".into(), v.into());
    }
    call_terminal(runtime, "read_output", args.into()).await
}

/// Literal or regex search over the session's line buffer.
#[tauri::command]
pub async fn term_search_output(
    id: String,
    query: String,
    is_regex: Option<bool>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(
        runtime,
        "search_output",
        serde_json::json!({
            "id": id,
            "query": query,
            "is_regex": is_regex.unwrap_or(false),
        }),
    )
    .await
}

/// Metadata for one session (name, shell, line count, created_at).
#[tauri::command]
pub async fn term_get_session_info(
    id: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(
        runtime,
        "get_session_info",
        serde_json::json!({ "id": id }),
    )
    .await
}

/// Every session the server knows about.
#[tauri::command]
pub async fn term_list_sessions(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_terminal(runtime, "list_sessions", serde_json::json!({})).await
}
