//! Hybrid vector recall for the memory engine.
//!
//! The memory store keeps its rows in `SQLite` (FTS5 for lexical search). For
//! semantic recall it reuses the existing embedding + vector infrastructure
//! through IPC rather than carrying its own model or vector table
//! (design decision D-1):
//!
//! - **embedding** — [`com.nexus.ai::embed_text`] turns text into a dense vector.
//! - **storage** — [`com.nexus.storage::vector_insert` / `vector_query`] persist
//!   and search those vectors in the dedicated `memory` namespace (so they never
//!   mingle with the forge-notes RAG corpus).
//!
//! [`recall`] fuses the FTS ranking with the vector ranking via
//! [`reciprocal_rank_fusion`]; [`vector_sync`] backfills embeddings for stored
//! memories so the vector arm has something to find. Both degrade gracefully:
//! with no wired context (or an unconfigured embedding provider) `recall` simply
//! returns its FTS results.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::MemoryDb;
use crate::model::Memory;

/// AI plugin id — owns `embed_text`.
const AI_PLUGIN: &str = "com.nexus.ai";
/// Storage plugin id — owns the namespaced vector store.
const STORAGE_PLUGIN: &str = "com.nexus.storage";
/// Vector-store namespace memories live in (isolated from forge `notes`).
const VECTOR_NAMESPACE: &str = "memory";
/// Timeout for the nested embed / vector IPC calls.
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
/// Reciprocal-rank-fusion damping constant (the canonical RRF default).
const RRF_K: f64 = 60.0;
/// Default number of memories returned by `recall`.
const DEFAULT_RECALL_LIMIT: usize = 20;
/// How many candidates each arm contributes before fusion (oversample so a
/// memory ranked outside the final `limit` in one arm can still win on fusion).
const ARM_OVERSAMPLE: usize = 10;
/// Default cap on how many memories one `vector_sync` call (re)indexes.
const DEFAULT_SYNC_LIMIT: usize = 1000;

/// Synthetic vector-store path for a memory id (the store keys rows by path).
fn vector_path(id: Uuid) -> String {
    format!("memory://{id}")
}

/// Recover a memory id from a `memory://<uuid>` vector-store path.
fn id_from_vector_path(path: &str) -> Option<Uuid> {
    path.strip_prefix("memory://")
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Reciprocal Rank Fusion. Each input is a ranked list of ids (best first).
/// Returns the ids ranked by `Σ 1/(k + rank)` across the lists, best first,
/// truncated to `limit`. A higher position in any list lifts an id; appearing
/// in multiple lists compounds.
#[must_use]
pub(crate) fn reciprocal_rank_fusion(rankings: &[Vec<Uuid>], k: f64, limit: usize) -> Vec<Uuid> {
    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for ranking in rankings {
        for (rank, id) in ranking.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)] // ranks are tiny; f64 is ample.
            let contribution = 1.0 / (k + (rank as f64) + 1.0);
            *scores.entry(*id).or_insert(0.0) += contribution;
        }
    }
    let mut ranked: Vec<(Uuid, f64)> = scores.into_iter().collect();
    // Sort by score desc; break ties by id so the order is deterministic.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().take(limit).map(|(id, _)| id).collect()
}

/// Embed `texts` via `com.nexus.ai::embed_text`, returning one vector each.
async fn embed(ctx: &KernelPluginContext, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
    let resp = ctx
        .ipc_call(
            AI_PLUGIN,
            "embed_text",
            json!({ "texts": texts }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| format!("embed_text: {e}"))?;
    let embeddings = resp
        .get("embeddings")
        .cloned()
        .ok_or_else(|| "embed_text: reply missing 'embeddings'".to_string())?;
    serde_json::from_value::<Vec<Vec<f32>>>(embeddings)
        .map_err(|e| format!("embed_text: decode embeddings: {e}"))
}

/// Semantic arm of recall: embed `query`, search the `memory` vector namespace,
/// and return the matched memory ids (best first). Errors propagate so `recall`
/// can fall back to FTS-only.
async fn vector_recall(
    ctx: &KernelPluginContext,
    query: &str,
    limit: usize,
) -> Result<Vec<Uuid>, String> {
    let mut embeddings = embed(ctx, vec![query.to_string()]).await?;
    let query_embedding = embeddings
        .pop()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "vector_recall: empty query embedding".to_string())?;
    let resp = ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "vector_query",
            json!({ "namespace": VECTOR_NAMESPACE, "embedding": query_embedding, "limit": limit }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| format!("vector_query: {e}"))?;
    let matches = resp
        .get("matches")
        .and_then(Value::as_array)
        .ok_or_else(|| "vector_query: reply missing 'matches'".to_string())?;
    Ok(matches
        .iter()
        .filter_map(|m| m.get("file_path").and_then(Value::as_str))
        .filter_map(id_from_vector_path)
        .collect())
}

/// Hybrid recall: fuse FTS5 (lexical) and vector (semantic) rankings via RRF.
///
/// Always runs the FTS arm; runs the vector arm only when a context is wired
/// and the embedding/vector calls succeed. With no vector hits it returns the
/// FTS ranking unchanged. Returns a JSON array of full memory rows in fused
/// order.
pub(crate) async fn recall(
    db: MemoryDb,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| "recall: missing 'query' string".to_string())?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_RECALL_LIMIT);
    let arm_limit = limit + ARM_OVERSAMPLE;

    // Lexical arm — always available, and the source of truth for materialising.
    let fts = db
        .search(query, arm_limit)
        .map_err(|e| format!("recall: fts: {e}"))?;
    let fts_ids: Vec<Uuid> = fts.iter().map(|m| m.id).collect();

    // Semantic arm — best-effort. Any failure (no ctx, no embedder, IPC error)
    // degrades to FTS-only rather than failing the recall.
    let vec_ids: Vec<Uuid> = match ctx {
        Some(ref ctx) => vector_recall(ctx, query, arm_limit)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let ordered: Vec<Uuid> = if vec_ids.is_empty() {
        fts_ids.into_iter().take(limit).collect()
    } else {
        reciprocal_rank_fusion(&[fts_ids, vec_ids], RRF_K, limit)
    };

    // Materialise in fused order, reusing FTS rows and fetching vector-only hits.
    let mut by_id: HashMap<Uuid, Memory> = fts.into_iter().map(|m| (m.id, m)).collect();
    let mut out: Vec<Memory> = Vec::with_capacity(ordered.len());
    for id in ordered {
        if let Some(m) = by_id.remove(&id) {
            out.push(m);
        } else if let Ok(Some(m)) = db.get(id) {
            out.push(m);
        }
    }
    serde_json::to_value(&out).map_err(|e| format!("recall: serialize: {e}"))
}

/// Backfill embeddings for stored memories into the `memory` vector namespace
/// so [`recall`]'s semantic arm has data. Embeds up to `limit` active memories
/// (default 1000) in one batch and upserts each under its `memory://<id>` path.
/// Idempotent — re-running re-embeds and replaces. Requires a wired context.
pub(crate) async fn vector_sync(
    db: MemoryDb,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, String> {
    let ctx = ctx.ok_or_else(|| "vector_sync: plugin context not wired".to_string())?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_SYNC_LIMIT);

    let memories = db
        .list_filtered(None, None, Some("active"), None, limit)
        .map_err(|e| format!("vector_sync: list: {e}"))?;
    if memories.is_empty() {
        return Ok(json!({ "indexed": 0 }));
    }

    let texts: Vec<String> = memories.iter().map(|m| m.content.clone()).collect();
    let embeddings = embed(&ctx, texts).await?;
    if embeddings.len() != memories.len() {
        return Err(format!(
            "vector_sync: embedder returned {} vectors for {} memories",
            embeddings.len(),
            memories.len()
        ));
    }

    let mut indexed = 0_u64;
    for (m, embedding) in memories.iter().zip(embeddings) {
        let path = vector_path(m.id);
        let chunk = json!({
            "file_path": path,
            "block_id": 0,
            "chunk_text": m.content,
            "embedding": embedding,
        });
        ctx.ipc_call(
            STORAGE_PLUGIN,
            "vector_insert",
            json!({ "namespace": VECTOR_NAMESPACE, "file_path": path, "chunks": [chunk] }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| format!("vector_sync: vector_insert {path}: {e}"))?;
        indexed += 1;
    }
    Ok(json!({ "indexed": indexed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn rrf_rewards_top_rank_and_cross_list_agreement() {
        let x = uuid(1);
        let y = uuid(2);
        let z = uuid(3);
        let w = uuid(4);
        // x: rank 0 in both lists. z: present in both (ranks 2 and 1).
        // y: only the lexical list. w: only the vector list.
        let fts = vec![x, y, z];
        let vec = vec![x, z, w];
        let fused = reciprocal_rank_fusion(&[fts, vec], RRF_K, 4);
        // x tops both → clear winner; z (in both) beats the single-list y/w.
        assert_eq!(fused, vec![x, z, y, w]);
    }

    #[test]
    fn rrf_single_list_preserves_order_and_truncates() {
        let ids: Vec<Uuid> = (1..=5).map(uuid).collect();
        let fused = reciprocal_rank_fusion(std::slice::from_ref(&ids), RRF_K, 3);
        assert_eq!(fused, ids[..3].to_vec());
    }

    #[test]
    fn rrf_empty_input_is_empty() {
        assert!(reciprocal_rank_fusion(&[], RRF_K, 5).is_empty());
    }

    #[test]
    fn vector_path_round_trips() {
        let id = Uuid::now_v7();
        assert_eq!(id_from_vector_path(&vector_path(id)), Some(id));
        assert_eq!(id_from_vector_path("notes/a.md"), None);
        assert_eq!(id_from_vector_path("memory://not-a-uuid"), None);
    }

    #[tokio::test]
    async fn recall_without_context_returns_fts_results() {
        let db = MemoryDb::open_in_memory().unwrap();
        db.insert(&Memory::new("the deployment runs on kubernetes"))
            .unwrap();
        db.insert(&Memory::new("a note about cats")).unwrap();
        // No wired context → vector arm is skipped, FTS arm still works.
        let out = recall(db, None, &json!({ "query": "kubernetes" }))
            .await
            .unwrap();
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0]["content"].as_str().unwrap().contains("kubernetes"));
    }

    #[tokio::test]
    async fn recall_requires_query() {
        let db = MemoryDb::open_in_memory().unwrap();
        let err = recall(db, None, &json!({})).await.unwrap_err();
        assert!(err.contains("missing 'query'"), "got: {err}");
    }

    #[tokio::test]
    async fn vector_sync_requires_context() {
        let db = MemoryDb::open_in_memory().unwrap();
        let err = vector_sync(db, None, &json!({})).await.unwrap_err();
        assert!(err.contains("context not wired"), "got: {err}");
    }
}
