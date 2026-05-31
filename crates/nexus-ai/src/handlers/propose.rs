//! G7 — `propose_tool_calls`: single-turn provider call that returns
//! the model's tool-use blocks WITHOUT executing them. Used by
//! `nexus-agent` (ADR 0023) to derive a `Plan` for later
//! approval-gated execution.

use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{
    build_ai_provider, exec_err, filter_to_read_only, ipc_messages_to_chat, messages_to_turns,
};
use crate::ipc::{
    AiProposeArgs, AiProposeReply, AiProposedToolCall, AiToolPolicy, AiUnmappedToolCall,
};
use crate::tools::ToolRegistry;

/// G7 — single-turn provider call that returns the model's tool-use
/// blocks without executing any of them, for the agent's
/// plan-then-approve flow (ADR 0023).
///
/// Mirrors `stream_chat`'s setup (registry resolution per
/// `AiToolPolicy`, including the MCP bridge under `AutoWithMcp`)
/// but uses `chat_turn_with_tools` exactly once with a no-op chunk
/// sink. Streaming events are intentionally NOT published — this
/// handler is for backgrounded planning, not user-visible chat.
pub(crate) async fn handle_propose_tool_calls(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    tools: Option<Arc<ToolRegistry>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiProposeArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("propose_tool_calls: args decode: {e}")))?;

    let ai_cfg = ai_cfg.ok_or_else(|| {
        exec_err("propose_tool_calls: no AI chat provider configured".to_string())
    })?;
    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let policy = parsed.tools.unwrap_or_default();
    let registry: Arc<ToolRegistry> = match policy {
        AiToolPolicy::None => Arc::new(ToolRegistry::new()),
        AiToolPolicy::Auto => tools.unwrap_or_else(|| Arc::new(ToolRegistry::new())),
        AiToolPolicy::AutoWithMcp => {
            let base = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            crate::tools::discover_mcp_tools(Arc::clone(&ctx), base).await
        }
        AiToolPolicy::AutoReadOnly => {
            let base = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            Arc::new(filter_to_read_only(&base))
        }
    };

    let messages = ipc_messages_to_chat(&parsed.messages);
    let turns = messages_to_turns(messages);
    let schemas = registry.schemas();
    let on_chunk = |_: String| {};
    let output = ai
        .chat_turn_with_tools(&turns, parsed.system.as_deref(), &schemas, &on_chunk)
        .await
        .map_err(|e| exec_err(format!("propose_tool_calls: provider: {e}")))?;

    let mut mapped: Vec<AiProposedToolCall> = Vec::new();
    let mut unmapped: Vec<AiUnmappedToolCall> = Vec::new();
    for call in output.tool_calls {
        match crate::tools::dispatch_target(&call.name, call.input.clone()) {
            Ok(target) => mapped.push(AiProposedToolCall {
                id: call.id,
                name: call.name,
                target_plugin_id: target.target_plugin_id,
                command_id: target.command_id,
                args: target.args,
            }),
            Err(e) => unmapped.push(AiUnmappedToolCall {
                id: call.id,
                name: call.name,
                input: call.input,
                error: e.to_string(),
            }),
        }
    }

    let reply = AiProposeReply {
        text: output.text,
        tool_calls: mapped,
        unmapped_tool_calls: unmapped,
    };
    serde_json::to_value(&reply)
        .map_err(|e| exec_err(format!("propose_tool_calls: encode reply: {e}")))
}
