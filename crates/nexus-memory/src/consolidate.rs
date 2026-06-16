//! Deterministic memory consolidation — dedupe by exact normalized content.
//!
//! Mirrors `remind_me`'s pure-function consolidation: cluster memories whose
//! content matches after normalization (trim, lowercase, collapse internal
//! whitespace), keep a single **canonical** per cluster, and let the caller
//! supersede the rest. This is intentionally conservative — only exact
//! normalized duplicates merge, so no semantically-distinct memory is ever lost.
//! (Fuzzy/embedding-based merge can layer on later.)

use crate::model::Memory;

/// Normalize content for duplicate detection: trim, lowercase, and collapse
/// every run of whitespace to a single space.
#[must_use]
pub(crate) fn normalize_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Group `memories` by normalized content, returning only the clusters with a
/// duplicate (size ≥ 2). Each returned cluster is ordered **canonical-first**:
/// most recently updated, ties broken by larger id, so the survivor is the
/// freshest copy and the ordering is deterministic.
#[must_use]
pub(crate) fn cluster_duplicates(memories: Vec<Memory>) -> Vec<Vec<Memory>> {
    use std::collections::HashMap;

    // Preserve first-seen order of cluster keys for deterministic output.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Memory>> = HashMap::new();
    for m in memories {
        let key = normalize_content(&m.content);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(m);
    }

    let mut clusters = Vec::new();
    for key in order {
        let mut group = groups.remove(&key).unwrap_or_default();
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|a, b| {
            b.updated_at.cmp(&a.updated_at).then_with(|| b.id.cmp(&a.id))
        });
        clusters.push(group);
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn mem(content: &str, secs: i64) -> Memory {
        let mut m = Memory::new(content);
        m.updated_at = DateTime::from_timestamp(secs, 0).unwrap();
        m
    }

    #[test]
    fn normalize_collapses_case_and_whitespace() {
        assert_eq!(normalize_content("  Hello   World  "), "hello world");
        assert_eq!(normalize_content("hello world"), "hello world");
        assert_eq!(normalize_content("HELLO\tWORLD\n"), "hello world");
    }

    #[test]
    fn clusters_only_exact_normalized_duplicates_canonical_first() {
        let mems = vec![
            mem("likes Rust", 100),
            mem("likes   rust", 300), // dup of #1 (newer → canonical)
            mem("LIKES RUST", 200),   // dup of #1
            mem("likes Go", 100),     // unique → no cluster
        ];
        let clusters = cluster_duplicates(mems);
        assert_eq!(clusters.len(), 1);
        let cluster = &clusters[0];
        assert_eq!(cluster.len(), 3);
        // Canonical (index 0) is the most recently updated.
        assert_eq!(cluster[0].content, "likes   rust");
    }

    #[test]
    fn no_clusters_when_all_unique() {
        let mems = vec![mem("a", 1), mem("b", 2), mem("c", 3)];
        assert!(cluster_duplicates(mems).is_empty());
    }
}
