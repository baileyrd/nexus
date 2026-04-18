//! Bridges between `nexus-agent`'s kernel-free traits and the
//! `KernelPluginContext` the bootstrap owns.
//!
//! `nexus-agent` deliberately depends on neither `nexus-kernel` nor
//! `com.nexus.ai` — keeping the agent library testable without a
//! kernel spun up. This module supplies production implementations
//! of its two boundary traits:
//!
//! - [`KernelToolDispatcher`] — forwards every [`ToolCall`] to
//!   `PluginContext::ipc_call`, turning any plugin into a callable
//!   tool the agent library can drive.
//! - [`AiChatDriver`] — wraps `com.nexus.ai::stream_chat` so
//!   [`nexus_agent::LlmAgent`] can produce plans against whatever
//!   provider the forge is configured with.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_agent::{ChatDriver, ToolCall, ToolDispatcher};
use nexus_kernel::{KernelPluginContext, PluginContext};
use serde::Deserialize;

/// Default per-tool timeout. Matches [`crate::terminal::IPC_TIMEOUT`]
/// and should cover most plugin dispatches; long-running tools (e.g.
/// a streaming chat) supply their own timeout at the provider layer.
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// Chat wait-for-completion bound. `stream_chat` resolves only after
/// the provider closes its stream — needs to outlive remote-model
/// latency.
const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

/// [`ToolDispatcher`] backed by a [`KernelPluginContext`]. Every
/// dispatched call becomes `ipc_call(target_plugin_id, command_id,
/// args)`. Failures (plugin not found, handler panic, timeout) are
/// flattened into strings for the agent library's error surface.
pub struct KernelToolDispatcher {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

impl KernelToolDispatcher {
    /// Wrap a context with the default timeout.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self {
            ctx,
            timeout: DEFAULT_TOOL_TIMEOUT,
        }
    }

    /// Override the per-call timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl ToolDispatcher for KernelToolDispatcher {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String> {
        self.ctx
            .ipc_call(
                &call.target_plugin_id,
                &call.command_id,
                call.args.clone(),
                self.timeout,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// [`ChatDriver`] that dispatches to `com.nexus.ai::stream_chat` and
/// returns the final text once the stream closes. Per-token streaming
/// still happens on the bus — the driver itself only cares about the
/// terminal value for plan assembly.
pub struct AiChatDriver {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

impl AiChatDriver {
    /// Wrap a context with the default chat timeout (5 minutes).
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self {
            ctx,
            timeout: DEFAULT_CHAT_TIMEOUT,
        }
    }

    /// Override the per-call timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[derive(Deserialize)]
struct StreamChatResponse {
    #[serde(default)]
    text: String,
}

#[async_trait]
impl ChatDriver for AiChatDriver {
    async fn chat(&self, system: &str, user_message: &str) -> Result<String, String> {
        let args = serde_json::json!({
            "messages": [{ "role": "user", "content": user_message }],
            "system": system,
        });
        let raw = self
            .ctx
            .ipc_call("com.nexus.ai", "stream_chat", args, self.timeout)
            .await
            .map_err(|e| e.to_string())?;
        let parsed: StreamChatResponse =
            serde_json::from_value(raw).map_err(|e| e.to_string())?;
        Ok(parsed.text)
    }
}
