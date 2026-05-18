//! `semantic_search` + `entity_recall` IPC handlers — embedding-driven
//! retrieval over the shared chunk vectorstore.

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{build_embedding_provider, exec_err};
use crate::rag;

/// BL-040 — embed `query` and return the top-`limit` chunks from the
/// vector store (no chat). Mirrors the embedder build path of `ask`
/// but skips the chat provider entirely so callers (palette, TUI,
/// MCP) get a fast, score-bearing list of hits.
pub(crate) async fn handle_semantic_search(
    ctx: &KernelPluginContext,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let query = args
        .get("query")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("semantic_search: missing 'query' string".to_string()))?;
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(10);

    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("semantic_search: no AI embedding provider configured".to_string()))?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let matches = rag::semantic_search(ctx, embedder.as_ref(), query, limit)
        .await
        .map_err(|e| exec_err(format!("semantic_search: {e}")))?;
    Ok(serde_json::json!({ "matches": matches }))
}

/// BL-128 close — semantic recall over the `entities/` corpus.
///
/// Pipeline: embed `query` → query shared chunk vectorstore with a
/// 5x oversample (so blocky entities still surface even when the
/// best chunk isn't in the top-N) → keep hits whose `file_path`
/// starts with `entities/` → group by file, keeping the max score
/// per entity → resolve each stem through `entity_get` so the
/// caller receives full entity payloads ranked by their best
/// chunk's similarity.
///
/// Returns `EntityRecallResult { results: [] }` (not an error) when
/// the corpus has been embedded but produced no entity hits — the
/// caller treats "no matches" identically to "embedder missing"
/// and falls back to the substring path. Error path is reserved
/// for unconfigured embedder + IPC plumbing failures so the
/// "happy fallback" stays a Result::Ok branch.
pub(crate) async fn handle_entity_recall(
    ctx: &KernelPluginContext,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    use crate::ipc::{EntityRecallHitRow, EntityRecallResult};

    let parsed: crate::ipc::EntityRecallArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("entity_recall: parse args: {e}")))?;
    let query = parsed.query.trim();
    if query.is_empty() {
        return Ok(serde_json::json!(EntityRecallResult { results: Vec::new() }));
    }
    let limit = parsed
        .limit
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5)
        .max(1);

    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("entity_recall: no AI embedding provider configured".to_string()))?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let oversample = limit.saturating_mul(5).max(20);
    let matches = rag::semantic_search(ctx, embedder.as_ref(), query, oversample)
        .await
        .map_err(|e| exec_err(format!("entity_recall: {e}")))?;

    let mut by_file: std::collections::BTreeMap<String, f32> = std::collections::BTreeMap::new();
    for hit in matches {
        if !hit.file_path.starts_with("entities/") {
            continue;
        }
        by_file
            .entry(hit.file_path.clone())
            .and_modify(|s| {
                if hit.score > *s {
                    *s = hit.score;
                }
            })
            .or_insert(hit.score);
    }
    let mut ranked: Vec<(String, f32)> = by_file.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked.truncate(limit);

    let mut results: Vec<EntityRecallHitRow> = Vec::with_capacity(ranked.len());
    for (path, score) in ranked {
        let stem = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| path.clone());
        let resp: serde_json::Value = ctx
            .ipc_call(
                "com.nexus.storage",
                "entity_get",
                serde_json::json!({ "id": stem }),
                std::time::Duration::from_secs(10),
            )
            .await
            .map_err(|e| exec_err(format!("entity_recall: entity_get '{stem}': {e}")))?;
        let Some(obj) = resp.get("entity").and_then(serde_json::Value::as_object) else {
            continue;
        };
        let id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&stem)
            .to_string();
        let entity_type = obj
            .get("entity_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("entity")
            .to_string();
        let description = obj
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let relpath = obj
            .get("relpath")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&path)
            .to_string();
        results.push(EntityRecallHitRow {
            id,
            entity_type,
            description,
            relpath,
            score,
        });
    }
    serde_json::to_value(&EntityRecallResult { results })
        .map_err(|e| exec_err(format!("entity_recall: serialise: {e}")))
}
