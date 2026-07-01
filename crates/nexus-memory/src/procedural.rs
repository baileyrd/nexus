//! Procedural memory — learned skill and pattern registry.
//!
//! Procedural memory stores *how* the agent accomplishes tasks:
//! named prompt templates, action sequences, and learned heuristics.
//! When a user query or session goal matches a stored skill's trigger
//! patterns, the context builder (Move 5) injects it into the model's
//! context so the model can apply the learned approach without
//! re-discovering it from scratch.
//!
//! Phase 1 shipped an in-memory registry with substring trigger
//! matching. Phase 5 (here) adds optional `SQLite` persistence to the
//! shared `<forge>/.forge/memory/memory.db` (`procedural_skills`
//! table) — construct via [`ProceduralStore::open`] to load learned
//! skills and write every subsequent mutation through.
//! Embedding-based trigger matching remains a follow-up; the interface
//! is stable across both.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{MemoryDb, MemoryDbError};

/// Opaque identifier for a [`ProceduralEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProceduralId(Uuid);

impl ProceduralId {
    /// Allocate a fresh random procedural id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wrap an existing UUID (durable-store load path).
    #[must_use]
    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ProceduralId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProceduralId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// One learned skill or procedural pattern.
///
/// `trigger_patterns` are matched against the current session goal (or
/// user message) with case-insensitive substring search. `template` is
/// injected verbatim into the model's context window when the skill
/// matches; it may be a prompt fragment, a tool-call sequence, or any
/// other guidance the model should follow for this task type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralEntry {
    /// Unique identifier.
    pub id: ProceduralId,
    /// Human-readable name (e.g. `"format_markdown_table"`).
    pub name: String,
    /// Short description for observability.
    pub description: String,
    /// Strings that, when found in the session goal, trigger this skill.
    pub trigger_patterns: Vec<String>,
    /// The skill content injected into context (prompt fragment, etc.).
    pub template: String,
    /// Session that taught the agent this skill, if any.
    pub source_session: Option<Uuid>,
    /// When the skill was learned / registered.
    pub learned_at: DateTime<Utc>,
    /// Number of times this skill has been applied.
    pub use_count: u64,
}

impl ProceduralEntry {
    /// Convenience constructor — fills timestamps and allocates an id.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        trigger_patterns: impl IntoIterator<Item = impl Into<String>>,
        template: impl Into<String>,
    ) -> Self {
        Self {
            id: ProceduralId::new(),
            name: name.into(),
            description: description.into(),
            trigger_patterns: trigger_patterns.into_iter().map(Into::into).collect(),
            template: template.into(),
            source_session: None,
            learned_at: Utc::now(),
            use_count: 0,
        }
    }
}

/// Procedural skill registry — in-memory map with optional durable
/// backing.
///
/// Thread-safe and `Clone`-able (both clones share the same `Arc`).
/// Skills are matched by substring against caller-supplied trigger
/// text; reads always come from the in-memory map, and the `SQLite`
/// layer exists so learned skills survive process restarts. Embedding
/// similarity can replace the matcher later without changing the
/// public interface.
#[derive(Clone, Debug)]
pub struct ProceduralStore {
    inner: Arc<Mutex<HashMap<ProceduralId, ProceduralEntry>>>,
    persist: Option<MemoryDb>,
}

impl ProceduralStore {
    /// Create a purely in-memory store (Phase-1 semantics — nothing
    /// survives a restart).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            persist: None,
        }
    }

    /// Open a store backed by `db` (Phase 5): learned skills are loaded
    /// into memory and every subsequent mutation writes through.
    ///
    /// # Errors
    /// Returns an error if the durable store cannot be read.
    pub fn open(db: MemoryDb) -> Result<Self, MemoryDbError> {
        let entries = db.procedural_all()?;
        let map = entries.into_iter().map(|e| (e.id, e)).collect();
        Ok(Self {
            inner: Arc::new(Mutex::new(map)),
            persist: Some(db),
        })
    }

    /// Register a skill. Silently replaces any skill with the same `name`
    /// (re-registration updates the template; last writer wins).
    ///
    /// When a durable store is attached the skill is written through
    /// best-effort: a write failure is logged and the in-memory map
    /// still updates, preserving the infallible Phase-1 contract.
    pub fn register(&self, entry: ProceduralEntry) {
        if let Some(db) = &self.persist {
            if let Err(e) = db.procedural_upsert(&entry) {
                tracing::warn!(error = %e, "procedural write-through failed; skill kept in-memory only");
            }
        }
        let mut g = self.inner.lock().expect("procedural store poisoned");
        g.retain(|_, e| e.name != entry.name);
        g.insert(entry.id, entry);
    }

    /// Look up skills whose trigger patterns match `trigger` (case-
    /// insensitive substring). Returns all matching skills sorted by
    /// `use_count` descending so the most battle-tested skill ranks
    /// first.
    #[must_use]
    pub fn lookup(&self, trigger: &str) -> Vec<ProceduralEntry> {
        let t = trigger.to_lowercase();
        let g = self.inner.lock().expect("procedural store poisoned");
        let mut results: Vec<&ProceduralEntry> = g
            .values()
            .filter(|e| {
                e.trigger_patterns
                    .iter()
                    .any(|p| t.contains(&p.to_lowercase()))
            })
            .collect();
        results.sort_by_key(|b| std::cmp::Reverse(b.use_count));
        results.into_iter().cloned().collect()
    }

    /// Fetch a skill by id.
    #[must_use]
    pub fn get(&self, id: ProceduralId) -> Option<ProceduralEntry> {
        self.inner
            .lock()
            .expect("procedural store poisoned")
            .get(&id)
            .cloned()
    }

    /// Fetch a skill by name (exact match, case-sensitive).
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<ProceduralEntry> {
        self.inner
            .lock()
            .expect("procedural store poisoned")
            .values()
            .find(|e| e.name == name)
            .cloned()
    }

    /// Increment the `use_count` for a skill. Called by the context
    /// builder each time a skill is injected into a context window so
    /// frequently-applied skills rank higher in future lookups.
    ///
    /// Write-through mirrors [`register`](Self::register): best-effort,
    /// with failures logged.
    pub fn record_use(&self, id: ProceduralId) {
        if let Some(db) = &self.persist {
            if let Err(e) = db.procedural_record_use(id) {
                tracing::warn!(error = %e, "procedural use-count write-through failed");
            }
        }
        let mut g = self.inner.lock().expect("procedural store poisoned");
        if let Some(entry) = g.get_mut(&id) {
            entry.use_count = entry.use_count.saturating_add(1);
        }
    }

    /// Remove a skill. Returns `true` if it was present.
    ///
    /// Write-through mirrors [`register`](Self::register): best-effort,
    /// with failures logged.
    pub fn unregister(&self, id: ProceduralId) -> bool {
        if let Some(db) = &self.persist {
            if let Err(e) = db.procedural_delete(id) {
                tracing::warn!(error = %e, "procedural delete write-through failed");
            }
        }
        self.inner
            .lock()
            .expect("procedural store poisoned")
            .remove(&id)
            .is_some()
    }

    /// Total skills registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("procedural store poisoned").len()
    }

    /// `true` when no skills are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ProceduralStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, patterns: &[&str]) -> ProceduralEntry {
        ProceduralEntry::new(name, "test skill", patterns.iter().copied(), "do the thing")
    }

    #[test]
    fn register_and_get_by_name_round_trips() {
        let store = ProceduralStore::new();
        store.register(skill("format_table", &["table", "format table"]));
        let found = store.get_by_name("format_table").expect("skill registered");
        assert_eq!(found.name, "format_table");
    }

    #[test]
    fn register_deduplicates_by_name_last_writer_wins() {
        let store = ProceduralStore::new();
        store.register(ProceduralEntry::new("sk", "v1", ["foo"], "old template"));
        store.register(ProceduralEntry::new("sk", "v2", ["foo"], "new template"));
        assert_eq!(store.len(), 1);
        assert_eq!(store.get_by_name("sk").unwrap().template, "new template");
    }

    #[test]
    fn lookup_matches_trigger_patterns_case_insensitively() {
        let store = ProceduralStore::new();
        store.register(skill("write_tests", &["write tests", "test"]));
        store.register(skill("summarize", &["summarize", "tldr"]));
        let results = store.lookup("Please write TESTS for this module");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "write_tests");
    }

    #[test]
    fn lookup_returns_empty_when_no_match() {
        let store = ProceduralStore::new();
        store.register(skill("format_table", &["table"]));
        assert!(store.lookup("summarize document").is_empty());
    }

    #[test]
    fn record_use_increments_count_and_lookup_sorts_by_use_count() {
        let store = ProceduralStore::new();
        store.register(skill("skill_a", &["common"]));
        store.register(skill("skill_b", &["common"]));
        let id_b = store.get_by_name("skill_b").unwrap().id;
        store.record_use(id_b);
        store.record_use(id_b);
        let results = store.lookup("common task");
        assert_eq!(results[0].name, "skill_b", "higher use_count ranks first");
    }

    #[test]
    fn unregister_removes_skill() {
        let store = ProceduralStore::new();
        store.register(skill("tmp", &["tmp"]));
        let id = store.get_by_name("tmp").unwrap().id;
        assert!(store.unregister(id));
        assert!(store.get(id).is_none());
        assert!(!store.unregister(id), "idempotent");
    }
}
