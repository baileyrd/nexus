//! BL-045 — `enrich_file` / `enrich_apply` IPC handlers.

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{build_ai_provider, build_embedding_provider, exec_err};

/// BL-045 — `enrich_file`: read a markdown note, ask the AI for
/// tags + summary, run `semantic_search` for related notes, return
/// an [`crate::enrichment::EnrichmentProposal`] WITHOUT writing.
pub(crate) async fn handle_enrich_file(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let path = args
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("enrich_file: missing 'path' string".to_string()))?;

    let ai_cfg = ai_cfg
        .ok_or_else(|| exec_err("enrich_file: no AI chat provider configured".to_string()))?;
    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("enrich_file: no AI embedding provider configured".to_string()))?;

    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let proposal = crate::enrichment::propose(ctx, ai.as_ref(), embedder.as_ref(), path)
        .await
        .map_err(|e| exec_err(format!("enrich_file: {e}")))?;
    serde_json::to_value(&proposal).map_err(|e| exec_err(format!("enrich_file: serialize: {e}")))
}

/// BL-045 — `enrich_apply`: merge a previously-returned proposal back
/// into the file's YAML frontmatter, but only if `body_hash` still
/// matches. Returns `{ applied: bool, reason?: String }`.
pub(crate) async fn handle_enrich_apply(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let raw_proposal = args
        .get("proposal")
        .ok_or_else(|| exec_err("enrich_apply: missing 'proposal'".to_string()))?;
    let proposal: crate::enrichment::EnrichmentProposal =
        serde_json::from_value(raw_proposal.clone())
            .map_err(|e| exec_err(format!("enrich_apply: proposal decode: {e}")))?;
    let (applied, reason) = crate::enrichment::apply(ctx, &proposal)
        .await
        .map_err(|e| exec_err(format!("enrich_apply: {e}")))?;
    Ok(serde_json::json!({
        "applied": applied,
        "reason": reason,
    }))
}
