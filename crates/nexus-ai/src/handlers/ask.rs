//! `ask` IPC handler — RAG: embed → search → chat round-trip.
//! Plus `generate` — a plain prompt → text completion with no retrieval.

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{build_ai_provider, build_embedding_provider, exec_err};
use crate::provider::{ChatMessage, Role};
use crate::rag;

/// Plain prompt → text completion via the configured chat provider, with **no**
/// RAG retrieval (unlike [`handle_ask`], it neither embeds nor searches). Lets
/// other plugins (e.g. memory's wiki synthesis) generate text from content they
/// supply directly in the prompt. `{ prompt, system? }` → `{ text }`.
pub(crate) async fn handle_generate(
    ai_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let prompt = args
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("generate: missing 'prompt' string".to_string()))?;
    let system = args.get("system").and_then(serde_json::Value::as_str);

    let ai_cfg =
        ai_cfg.ok_or_else(|| exec_err("generate: no AI chat provider configured".to_string()))?;
    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let messages = [ChatMessage {
        role: Role::User,
        content: prompt.to_string(),
    }];
    let text = ai
        .chat(&messages, system)
        .await
        .map_err(|e| exec_err(format!("generate (prompt_len={}): {e}", prompt.len())))?;
    Ok(serde_json::json!({ "text": text }))
}

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

    let ai_cfg =
        ai_cfg.ok_or_else(|| exec_err("ask: no AI chat provider configured".to_string()))?;
    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("ask: no AI embedding provider configured".to_string()))?;

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
    .map_err(|e| {
        exec_err(format!(
            "ask: rag query (question_len={}, limit={limit}): {e}",
            question.len()
        ))
    })?;
    serde_json::to_value(&response).map_err(|e| exec_err(format!("ask: serialize: {e}")))
}
