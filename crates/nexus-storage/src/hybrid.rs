//! Hybrid search — reciprocal-rank fusion of the Tantivy FTS arm and
//! the vector-store arm.
//!
//! Both engines already live in this crate ([`crate::search`] for BM25,
//! [`crate::vectorstore`] for cosine similarity); this module fuses
//! their rankings with the same RRF convention `nexus-memory` uses for
//! its `recall` handler (damping constant `k = 60`, the canonical RRF
//! default), so "search by meaning" and "search by keyword" compound
//! instead of competing. Exposed over IPC as
//! `com.nexus.storage::hybrid_search` (handler id `76`).
//!
//! Fusion is rank-based, not score-based, deliberately: BM25 scores and
//! cosine similarities live on incomparable scales, and RRF sidesteps
//! the calibration problem entirely.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::search::SearchResult;
use crate::vectorstore::ChunkMatch;

/// Reciprocal-rank-fusion damping constant (the canonical RRF default,
/// matching `nexus-memory`'s `recall`).
const RRF_K: f32 = 60.0;

/// One fused hit from [`crate::StorageEngine::hybrid_search`].
///
/// Serialized directly onto the IPC wire; the mirror type is
/// `crate::ipc::StorageHybridMatch` (kept in sync manually; compared
/// via `cargo test -p nexus-bootstrap --test ipc_schema_emit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridMatch {
    /// Path of the source file (forge-relative).
    pub file_path: String,
    /// Identifier of the matched block.
    pub block_id: u64,
    /// Block type from the FTS arm (e.g. `"paragraph"`), when that arm
    /// produced the hit. Vector-only hits carry `None` — the vector
    /// store does not record block types.
    pub block_type: Option<String>,
    /// Display text: the FTS excerpt when available, otherwise the
    /// matched chunk's text.
    pub excerpt: String,
    /// Fused RRF score — `Σ 1/(k + rank + 1)` across the arms that
    /// returned this block. Higher is more relevant. Only comparable
    /// within one reply.
    pub score: f32,
    /// 0-based rank in the FTS arm, when it hit there.
    pub fts_rank: Option<u32>,
    /// 0-based rank in the vector arm, when it hit there.
    pub vector_rank: Option<u32>,
}

/// Fuse the two ranked arms with reciprocal-rank fusion, best first,
/// truncated to `limit`. A higher position in either arm lifts a block;
/// appearing in both compounds. Ties break on `(file_path, block_id)`
/// so the order is deterministic.
#[must_use]
pub(crate) fn fuse(fts: &[SearchResult], vector: &[ChunkMatch], limit: usize) -> Vec<HybridMatch> {
    let mut merged: HashMap<(String, u64), HybridMatch> = HashMap::new();

    for (rank, hit) in fts.iter().enumerate() {
        let rank_u32 = u32::try_from(rank).unwrap_or(u32::MAX);
        #[allow(clippy::cast_precision_loss)] // ranks are tiny; f32 is ample.
        let contribution = 1.0 / (RRF_K + rank as f32 + 1.0);
        merged
            .entry((hit.file_path.clone(), hit.block_id))
            .and_modify(|m| {
                m.score += contribution;
                m.fts_rank = Some(rank_u32);
            })
            .or_insert_with(|| HybridMatch {
                file_path: hit.file_path.clone(),
                block_id: hit.block_id,
                block_type: Some(hit.block_type.clone()),
                excerpt: hit.excerpt.clone(),
                score: contribution,
                fts_rank: Some(rank_u32),
                vector_rank: None,
            });
    }

    for (rank, hit) in vector.iter().enumerate() {
        let rank_u32 = u32::try_from(rank).unwrap_or(u32::MAX);
        #[allow(clippy::cast_precision_loss)] // ranks are tiny; f32 is ample.
        let contribution = 1.0 / (RRF_K + rank as f32 + 1.0);
        merged
            .entry((hit.file_path.clone(), hit.block_id))
            .and_modify(|m| {
                m.score += contribution;
                m.vector_rank = Some(rank_u32);
                // Keep the FTS excerpt when both arms hit — it is
                // query-highlighted; the chunk text is raw content.
                if m.excerpt.is_empty() {
                    m.excerpt.clone_from(&hit.chunk_text);
                }
            })
            .or_insert_with(|| HybridMatch {
                file_path: hit.file_path.clone(),
                block_id: hit.block_id,
                block_type: None,
                excerpt: hit.chunk_text.clone(),
                score: contribution,
                fts_rank: None,
                vector_rank: Some(rank_u32),
            });
    }

    let mut ranked: Vec<HybridMatch> = merged.into_values().collect();
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.block_id.cmp(&b.block_id))
    });
    ranked.truncate(limit);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fts_hit(path: &str, block: u64, score: f32) -> SearchResult {
        SearchResult {
            file_path: path.to_string(),
            block_id: block,
            block_type: "paragraph".to_string(),
            excerpt: format!("excerpt for {path}#{block}"),
            score,
        }
    }

    fn vec_hit(path: &str, block: u64, score: f32) -> ChunkMatch {
        ChunkMatch {
            file_path: path.to_string(),
            block_id: block,
            chunk_text: format!("chunk for {path}#{block}"),
            score,
        }
    }

    #[test]
    fn block_in_both_arms_outranks_single_arm_leaders() {
        // `both.md#1` is ranked second in each arm; the leaders of each
        // arm appear only once. RRF: 2 × 1/(60+2) > 1/(60+1).
        let fts = vec![fts_hit("fts-only.md", 1, 9.0), fts_hit("both.md", 1, 5.0)];
        let vector = vec![vec_hit("vec-only.md", 1, 0.99), vec_hit("both.md", 1, 0.80)];

        let fused = fuse(&fts, &vector, 10);
        assert_eq!(fused[0].file_path, "both.md");
        assert_eq!(fused[0].fts_rank, Some(1));
        assert_eq!(fused[0].vector_rank, Some(1));
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn single_arm_degrades_to_that_arms_ranking() {
        let fts = vec![fts_hit("a.md", 1, 3.0), fts_hit("b.md", 2, 2.0)];
        let fused = fuse(&fts, &[], 10);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].file_path, "a.md");
        assert_eq!(fused[0].vector_rank, None);
        assert_eq!(fused[1].file_path, "b.md");
    }

    #[test]
    fn ties_break_deterministically_by_path_then_block() {
        // Same single-arm rank each → identical scores.
        let fts = vec![fts_hit("z.md", 1, 1.0)];
        let vector = vec![vec_hit("a.md", 7, 1.0)];
        let fused = fuse(&fts, &vector, 10);
        assert_eq!(fused[0].file_path, "a.md");
        assert_eq!(fused[1].file_path, "z.md");
    }

    #[test]
    fn limit_truncates_after_fusion() {
        let fts: Vec<SearchResult> = (0..5).map(|i| fts_hit("f.md", i, 5.0)).collect();
        let vector: Vec<ChunkMatch> = (0..5).map(|i| vec_hit("v.md", i, 0.9)).collect();
        let fused = fuse(&fts, &vector, 3);
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn fts_excerpt_wins_when_both_arms_hit() {
        let fts = vec![fts_hit("n.md", 1, 2.0)];
        let vector = vec![vec_hit("n.md", 1, 0.9)];
        let fused = fuse(&fts, &vector, 10);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].excerpt, "excerpt for n.md#1");
        assert_eq!(fused[0].block_type.as_deref(), Some("paragraph"));
    }
}
