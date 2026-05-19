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
use nexus_agent::{ChatDriver, Proposal, ProposedToolCall, ToolCall, ToolDispatcher};
use nexus_kernel::{Ipc as _, KernelPluginContext};
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

/// [`ChatDriver`] that dispatches to
/// `com.nexus.ai::propose_tool_calls` (G7-1b / ADR 0023). Returns
/// the model's `tool_use` blocks already mapped to `(target,
/// command, args)` triples by the AI plugin's `dispatch_target`,
/// ready for `LlmAgent` to fold into a [`nexus_agent::Plan`]. No
/// streaming events are published — planning runs are silent.
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

/// Wire shape of `com.nexus.ai::propose_tool_calls`'s reply.
/// Mirrors `nexus_ai::ipc::AiProposeReply` without taking a hard
/// dependency on it.
#[derive(Deserialize)]
struct ProposeWire {
    #[serde(default)]
    text: String,
    #[serde(default)]
    tool_calls: Vec<ProposedWire>,
}

#[derive(Deserialize)]
struct ProposedWire {
    id: String,
    name: String,
    target_plugin_id: String,
    command_id: String,
    args: serde_json::Value,
}

#[async_trait]
impl ChatDriver for AiChatDriver {
    async fn propose(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<Proposal, String> {
        let args = serde_json::json!({
            "messages": [{ "role": "user", "content": user_message }],
            "system": system,
        });
        let raw = self
            .ctx
            .ipc_call(
                "com.nexus.ai",
                "propose_tool_calls",
                args,
                self.timeout,
            )
            .await
            .map_err(|e| e.to_string())?;
        let parsed: ProposeWire = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        let tool_calls = parsed
            .tool_calls
            .into_iter()
            .map(|t| ProposedToolCall {
                id: t.id,
                name: t.name,
                tool_call: ToolCall {
                    target_plugin_id: t.target_plugin_id,
                    command_id: t.command_id,
                    args: t.args,
                },
            })
            .collect();
        Ok(Proposal {
            text: parsed.text,
            tool_calls,
        })
    }
}
