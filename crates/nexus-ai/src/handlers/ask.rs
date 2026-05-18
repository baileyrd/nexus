//! `ask` IPC handler — RAG: embed → search → chat round-trip.

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{build_ai_provider, build_embedding_provider, exec_err};
use crate::rag;

pub(crate) async fn handle_ask(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let question = args
        .get("question")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("ask: missing 'question' string".to_string()))?;
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);

    let ai_cfg = ai_cfg.ok_or_else(|| exec_err("ask: no AI chat provider configured".to_string()))?;
    let embed_cfg =
        embed_cfg.ok_or_else(|| exec_err("ask: no AI embedding provider configured".to_string()))?;

    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let response = rag::query(
        ctx,
        ai.as_ref(),
        embedder.as_ref(),
        question,
        limit,
        ai_cfg.injection_policy,
    )
    .await
    .map_err(|e| exec_err(format!("rag query failed: {e}")))?;
    serde_json::to_value(&response).map_err(|e| exec_err(format!("ask: serialize: {e}")))
}
