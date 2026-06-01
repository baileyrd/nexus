//! Semantic memory — factual knowledge store with keyword search.
//!
//! Semantic memory holds declarative facts the agent has learned or
//! been given: user preferences, domain knowledge, entity summaries,
//! and extracted insights. Phase 1 provides keyword/prefix matching.
//! Phase 5 layers in embedding-based vector search using the forge's
//! existing Tantivy index as the persistence substrate.
//!
//! Entries are tagged and source-linked so the context builder (Move 5)
//! can filter by relevance to the current session's domain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque identifier for a [`SemanticEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticId(Uuid);

impl SemanticId {
    /// Allocate a fresh random semantic id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for SemanticId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SemanticId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// One fact stored in semantic memory.
///
/// `key` is a human-readable label (e.g. `"user.prefers_dark_mode"`);
/// `content` is the fact body. Both are matched by [`SemanticStore::search`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntry {
    /// Unique identifier.
    pub id: SemanticId,
    /// Short label used as a retrieval key (e.g. `"user.language"`).
    pub key: String,
    /// Fact content — free-form text or JSON-serialized structured data.
    pub content: String,
    /// Categorisation tags for coarse filtering (e.g. `["user", "preferences"]`).
    pub tags: Vec<String>,
    /// Session that produced this fact, if any.
    pub source_session: Option<Uuid>,
    /// When the fact was stored.
    pub stored_at: DateTime<Utc>,
}

impl SemanticEntry {
    /// Convenience constructor — fills timestamps and allocates an id.
    #[must_use]
    pub fn new(key: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: SemanticId::new(),
            key: key.into(),
            content: content.into(),
            tags: Vec::new(),
            source_session: None,
            stored_at: Utc::now(),
        }
    }

    /// Builder: attach tags.
    #[must_use]
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: attach a source session.
    #[must_use]
    pub fn with_session(mut self, session_id: Uuid) -> Self {
        self.source_session = Some(session_id);
        self
    }
}

/// In-memory semantic knowledge store with keyword search.
///
/// Thread-safe and `Clone`-able (both clones share the same `Arc`).
/// Phase 5 adds a persistence layer and replaces `search` with an
/// embedding-based retrieval path while keeping the same interface.
#[derive(Clone, Debug)]
pub struct SemanticStore {
    inner: Arc<Mutex<HashMap<SemanticId, SemanticEntry>>>,
}

impl SemanticStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store a fact. Replaces any existing entry with the same `key`
    /// (last writer wins; keyed de-duplication avoids stale facts).
    pub fn store(&self, entry: SemanticEntry) {
        let mut g = self.inner.lock().expect("semantic store poisoned");
        // Remove any entry with the same key before inserting the new one
        // so the store stays de-duplicated by key (not by id).
        g.retain(|_, e| e.key != entry.key);
        g.insert(entry.id, entry);
    }

    /// Case-insensitive keyword search over `key` + `content`. Returns
    /// up to `limit` entries, ordered by `stored_at` descending (most
    /// recently stored first, since recency correlates with relevance
    /// for the Phase-1 in-memory store).
    ///
    /// Phase 5 replaces this with embedding cosine similarity.
    #[must_use]
    pub fn search(&self, query: &str, limit: usize) -> Vec<SemanticEntry> {
        let q = query.to_lowercase();
        let g = self.inner.lock().expect("semantic store poisoned");
        let mut results: Vec<&SemanticEntry> = g
            .values()
            .filter(|e| e.key.to_lowercase().contains(&q) || e.content.to_lowercase().contains(&q))
            .collect();
        results.sort_by_key(|b| std::cmp::Reverse(b.stored_at));
        results.into_iter().take(limit).cloned().collect()
    }

    /// Retrieve a specific entry by id.
    #[must_use]
    pub fn get(&self, id: SemanticId) -> Option<SemanticEntry> {
        self.inner
            .lock()
            .expect("semantic store poisoned")
            .get(&id)
            .cloned()
    }

    /// Retrieve by key (exact match, case-sensitive).
    #[must_use]
    pub fn get_by_key(&self, key: &str) -> Option<SemanticEntry> {
        self.inner
            .lock()
            .expect("semantic store poisoned")
            .values()
            .find(|e| e.key == key)
            .cloned()
    }

    /// Remove an entry. Returns `true` if it was present.
    pub fn remove(&self, id: SemanticId) -> bool {
        self.inner
            .lock()
            .expect("semantic store poisoned")
            .remove(&id)
            .is_some()
    }

    /// All entries matching any of `tags`. Entries that match multiple
    /// tags appear once.
    #[must_use]
    pub fn by_tag(&self, tags: &[&str]) -> Vec<SemanticEntry> {
        self.inner
            .lock()
            .expect("semantic store poisoned")
            .values()
            .filter(|e| tags.iter().any(|t| e.tags.iter().any(|et| et == t)))
            .cloned()
            .collect()
    }

    /// Total entries in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("semantic store poisoned").len()
    }

    /// `true` when no entries are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SemanticStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_get_by_key_round_trips() {
        let store = SemanticStore::new();
        let e = SemanticEntry::new("user.language", "Rust");
        store.store(e.clone());
        let found = store.get_by_key("user.language").expect("entry stored");
        assert_eq!(found.content, "Rust");
    }

    #[test]
    fn store_deduplicates_by_key_last_writer_wins() {
        let store = SemanticStore::new();
        store.store(SemanticEntry::new("pref.theme", "dark"));
        store.store(SemanticEntry::new("pref.theme", "light"));
        assert_eq!(store.len(), 1);
        assert_eq!(store.get_by_key("pref.theme").unwrap().content, "light");
    }

    #[test]
    fn search_matches_key_and_content_case_insensitive() {
        let store = SemanticStore::new();
        store.store(SemanticEntry::new("user.name", "Alice"));
        store.store(SemanticEntry::new("project.goal", "Build a tool for alice"));
        let results = store.search("alice", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_respects_limit() {
        let store = SemanticStore::new();
        for i in 0..5 {
            store.store(SemanticEntry::new(format!("fact.{i}"), "rust programming"));
        }
        let results = store.search("rust", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn remove_entry_is_no_longer_findable() {
        let store = SemanticStore::new();
        let e = SemanticEntry::new("tmp", "value");
        let id = e.id;
        store.store(e);
        assert!(store.get(id).is_some());
        assert!(store.remove(id));
        assert!(store.get(id).is_none());
        assert!(
            !store.remove(id),
            "idempotent — second remove returns false"
        );
    }

    #[test]
    fn by_tag_filters_correctly() {
        let store = SemanticStore::new();
        store.store(SemanticEntry::new("a", "x").with_tags(["user", "prefs"]));
        store.store(SemanticEntry::new("b", "y").with_tags(["project"]));
        store.store(SemanticEntry::new("c", "z").with_tags(["user", "context"]));
        let user_entries = store.by_tag(&["user"]);
        assert_eq!(user_entries.len(), 2);
        assert!(user_entries
            .iter()
            .all(|e| e.tags.contains(&"user".to_string())));
    }
}
