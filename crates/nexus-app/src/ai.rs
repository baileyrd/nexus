//! Tauri command bridge into the AI core plugin.
//!
//! Thin adapter: serializes args to JSON and calls
//! [`nexus_kernel::PluginContext::ipc_call`] on `com.nexus.ai`.
//! Streaming chunks travel out-of-band via the kernel event bus
//! (`com.nexus.ai.stream_*`) — see
//! [`crate::start_ai_event_forwarder`] for the bridge to Tauri.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

const AI_PLUGIN_ID: &str = "com.nexus.ai";

/// Chat streams may run for many seconds on remote providers — give them
/// a generous upper bound instead of the editor's 5-second timeout.
const AI_CALL_TIMEOUT: Duration = Duration::from_secs(120);

async fn call_ai(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(AI_PLUGIN_ID, command, args, AI_CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Detected provider snapshot (provider, model, `base_url`, `has_api_key`).
#[tauri::command]
pub async fn ai_config(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_ai(runtime, "config", serde_json::json!({})).await
}

/// Streaming chat completion. Chunks arrive as `ai:stream_chunk` Tauri
/// events; this call resolves with the full final text once the provider
/// finishes. `messages` is `[{role, content}, …]`.
#[tauri::command]
pub async fn ai_stream_chat(
    messages: serde_json::Value,
    system: Option<String>,
    session_id: Option<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    let mut args = serde_json::json!({ "messages": messages });
    if let Some(sys) = system {
        args["system"] = serde_json::Value::String(sys);
    }
    if let Some(sid) = session_id {
        args["session_id"] = serde_json::Value::String(sid);
    }
    call_ai(runtime, "stream_chat", args).await
}

/// Streaming RAG completion — retrieves top-`limit` chunks from the
/// indexed vector store, prepends them as a system prompt, and streams
/// the chat response through the same `ai:stream_*` event channel as
/// [`ai_stream_chat`]. Returns the final text plus the list of sources
/// used so the UI can surface citations.
#[tauri::command]
pub async fn ai_stream_ask(
    messages: serde_json::Value,
    limit: Option<u64>,
    session_id: Option<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    let mut args = serde_json::json!({ "messages": messages });
    if let Some(n) = limit {
        args["limit"] = serde_json::Value::from(n);
    }
    if let Some(sid) = session_id {
        args["session_id"] = serde_json::Value::String(sid);
    }
    call_ai(runtime, "stream_ask", args).await
}
