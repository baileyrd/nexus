//! Typed IPC-client helpers for the `com.nexus.terminal` core plugin.
//!
//! CLI / TUI callers reach the terminal subsystem exclusively through these
//! helpers — no direct `nexus-terminal` dependency, matching the pattern of
//! [`crate::storage`] and [`crate::database`].
//!
//! Each helper:
//!
//! 1. Serializes arguments to JSON.
//! 2. `block_on`s the async `ipc_call` on the provided Tokio runtime.
//! 3. Deserializes the response into a typed DTO.
//!
//! DTOs mirror the corresponding structs in `nexus_terminal` but contain
//! only the fields callers read. Extra fields in the response are ignored
//! by serde.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime as TokioRuntime;

use crate::Runtime;

const TERMINAL_PLUGIN: &str = "com.nexus.terminal";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Mirror of `nexus_terminal::OutputLine`. Timestamps are Unix-ms per the
/// crate's JSON encoding of `SystemTime`.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputLine {
    /// Milliseconds since Unix epoch at first ingestion.
    pub timestamp_ms: u64,
    /// ANSI-stripped text content (no trailing newline).
    pub content: String,
    /// Raw bytes including ANSI sequences.
    #[serde(default)]
    pub raw: Vec<u8>,
    /// Adjacent-repeat counter (1 for a distinct line).
    #[serde(default = "one")]
    pub repeats: u32,
}

const fn one() -> u32 {
    1
}

/// Mirror of `nexus_terminal::SessionInfo`.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    /// Opaque session id.
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Shell path (empty when unknown to the server).
    #[serde(default)]
    pub shell: String,
    /// Working directory, if known.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Current line count in the buffer.
    #[serde(default)]
    pub line_count: usize,
    /// Unix-seconds creation timestamp.
    #[serde(default)]
    pub created_at: u64,
}

/// Arguments for the `create_session` helper. Matches
/// `nexus_terminal::CreateSessionArgs`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateSessionArgs {
    /// Optional human-readable label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Absolute path to the shell executable. Falls back to detection
    /// when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Extra args for the shell binary (after the program name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_args: Vec<String>,
    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Extra env vars.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct PumpResponse {
    bytes: usize,
}

// ── Internal helper ──────────────────────────────────────────────────────────

fn call<T: serde::de::DeserializeOwned>(
    runtime: &Runtime,
    rt: &TokioRuntime,
    command: &str,
    args: serde_json::Value,
) -> Result<T> {
    let value = rt
        .block_on(
            runtime
                .context
                .ipc_call(TERMINAL_PLUGIN, command, args, IPC_TIMEOUT),
        )
        .with_context(|| format!("terminal ipc call '{command}' failed"))?;
    serde_json::from_value(value)
        .with_context(|| format!("terminal ipc response '{command}' decode failed"))
}

// ── Public helpers ───────────────────────────────────────────────────────────

/// Spawn a new PTY session and return its id.
///
/// # Errors
/// Propagates any IPC / shell-spawn failure surfaced by the core plugin.
pub fn create_session(
    runtime: &Runtime,
    rt: &TokioRuntime,
    args: CreateSessionArgs,
) -> Result<String> {
    let resp: CreateSessionResponse =
        call(runtime, rt, "create_session", serde_json::to_value(args)?)?;
    Ok(resp.id)
}

/// Graceful shutdown of the session's PTY child via the §5.1 signal
/// ladder.
///
/// # Errors
/// Propagates any IPC / signal-delivery failure.
pub fn close_session(runtime: &Runtime, rt: &TokioRuntime, id: &str) -> Result<()> {
    let _: serde_json::Value = call(
        runtime,
        rt,
        "close_session",
        serde_json::json!({ "id": id }),
    )?;
    Ok(())
}

/// Write `input` to the session's stdin, appending a newline if absent.
///
/// # Errors
/// Propagates any IPC / write failure.
pub fn send_input(
    runtime: &Runtime,
    rt: &TokioRuntime,
    id: &str,
    input: &str,
) -> Result<()> {
    let _: serde_json::Value = call(
        runtime,
        rt,
        "send_input",
        serde_json::json!({ "id": id, "input": input }),
    )?;
    Ok(())
}

/// Send raw bytes to the session's stdin (no newline added).
///
/// # Errors
/// Propagates any IPC / write failure.
pub fn send_raw_input(
    runtime: &Runtime,
    rt: &TokioRuntime,
    id: &str,
    data: &[u8],
) -> Result<()> {
    let _: serde_json::Value = call(
        runtime,
        rt,
        "send_raw_input",
        serde_json::json!({ "id": id, "data": data }),
    )?;
    Ok(())
}

/// Drain the PTY into the session's line buffer. Blocks up to
/// `timeout_ms` for the first byte. Returns the byte count drained.
///
/// # Errors
/// Propagates any IPC / read failure.
pub fn pump(
    runtime: &Runtime,
    rt: &TokioRuntime,
    id: &str,
    timeout_ms: u64,
) -> Result<usize> {
    let resp: PumpResponse = call(
        runtime,
        rt,
        "pump",
        serde_json::json!({ "id": id, "timeout_ms": timeout_ms }),
    )?;
    Ok(resp.bytes)
}

/// Read a window of structured lines from the session buffer.
///
/// # Errors
/// Propagates any IPC failure.
pub fn read_output(
    runtime: &Runtime,
    rt: &TokioRuntime,
    id: &str,
    start: Option<usize>,
    count: Option<usize>,
) -> Result<Vec<OutputLine>> {
    let mut args = serde_json::json!({ "id": id });
    if let Some(s) = start {
        args["start"] = s.into();
    }
    if let Some(c) = count {
        args["count"] = c.into();
    }
    call(runtime, rt, "read_output", args)
}

/// List every session the server knows about.
///
/// # Errors
/// Propagates any IPC failure.
pub fn list_sessions(runtime: &Runtime, rt: &TokioRuntime) -> Result<Vec<SessionInfo>> {
    call(runtime, rt, "list_sessions", serde_json::json!({}))
}

/// Look up metadata for one session.
///
/// # Errors
/// Propagates any IPC failure; returns a hard error (not `None`) for
/// unknown ids so callers can distinguish "gone" from "empty buffer".
pub fn get_session_info(
    runtime: &Runtime,
    rt: &TokioRuntime,
    id: &str,
) -> Result<SessionInfo> {
    call(
        runtime,
        rt,
        "get_session_info",
        serde_json::json!({ "id": id }),
    )
}
