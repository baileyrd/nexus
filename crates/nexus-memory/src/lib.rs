//! AI-native memory layer for Nexus.
//!
//! Nexus agents maintain three kinds of memory, mirroring the cognitive
//! science literature on human memory:
//!
//! | Kind | Store | What it holds |
//! |------|-------|---------------|
//! | Episodic | [`EpisodicStore`] | Time-ordered agent events (messages, tool calls, observations) |
//! | Semantic | [`SemanticStore`] | Declarative facts (user preferences, domain knowledge) |
//! | Procedural | [`ProceduralStore`] | Learned skills and how-to patterns |
//!
//! All three are in-memory in Phase 1 (bounded ring for episodic, hash
//! maps for semantic/procedural). Phase 5 adds SQLite persistence so
//! memory survives across process restarts; the API surface is stable
//! across that transition.
//!
//! ## Usage
//!
//! The top-level [`MemoryStore`] holds all three sub-stores and is the
//! type callers should embed:
//!
//! ```rust
//! use nexus_memory::MemoryStore;
//!
//! let mem = MemoryStore::default();
//! // Record a fact.
//! mem.semantic.store(
//!     nexus_memory::SemanticEntry::new("user.language", "Rust")
//! );
//! // Query it.
//! let facts = mem.semantic.search("language", 5);
//! assert_eq!(facts[0].content, "Rust");
//! ```

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod capture;
pub mod capture_pipeline;
pub mod consolidate;
pub mod core_plugin;
pub mod db;
pub mod episodic;
pub mod import;
pub mod model;
pub mod procedural;
pub mod semantic;
pub mod sync;
pub mod vector;
pub mod wiki;

pub use episodic::{
    EpisodicEntry, EpisodicId, EpisodicKind, EpisodicStore, DEFAULT_EPISODIC_CAPACITY,
};
pub use procedural::{ProceduralEntry, ProceduralId, ProceduralStore};
pub use semantic::{SemanticEntry, SemanticId, SemanticStore};

pub use capture::event_to_memory;
pub use db::{MemoryDb, MemoryDbError};
pub use core_plugin::MemoryCorePlugin;
pub use import::{import_chat_log, import_remind_me_db, ImportReport};
pub use model::{Memory, MemoryStatus, MemoryType};

/// Unified memory handle carrying all three memory kinds.
///
/// `Clone`-able — both clones share the same backing `Arc`s, so all
/// workers in a session can record to and read from the same stores
/// without additional synchronisation.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    /// Time-ordered record of agent events this process has observed.
    pub episodic: EpisodicStore,
    /// Declarative facts the agent has stored or been given.
    pub semantic: SemanticStore,
    /// Learned skills and procedural patterns.
    pub procedural: ProceduralStore,
}

impl MemoryStore {
    /// Create a `MemoryStore` with the default episodic capacity
    /// ([`DEFAULT_EPISODIC_CAPACITY`]) and empty semantic/procedural stores.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a `MemoryStore` with a custom episodic capacity.
    #[must_use]
    pub fn with_episodic_capacity(capacity: usize) -> Self {
        Self {
            episodic: EpisodicStore::new(capacity),
            semantic: SemanticStore::new(),
            procedural: ProceduralStore::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn memory_store_default_is_empty() {
        let mem = MemoryStore::new();
        assert!(mem.episodic.is_empty());
        assert!(mem.semantic.is_empty());
        assert!(mem.procedural.is_empty());
    }

    #[test]
    fn memory_store_clone_shares_state() {
        let mem = MemoryStore::new();
        let mem2 = mem.clone();
        let sid = Uuid::new_v4();
        mem.episodic.record(EpisodicEntry::for_session(
            sid,
            EpisodicKind::UserMessage,
            serde_json::json!({ "text": "hi" }),
        ));
        // Both clones see the new entry.
        assert_eq!(mem2.episodic.len(), 1);
    }

    #[test]
    fn with_episodic_capacity_sets_custom_cap() {
        let mem = MemoryStore::with_episodic_capacity(8);
        let sid = Uuid::new_v4();
        for i in 0..10u32 {
            mem.episodic.record(EpisodicEntry::for_session(
                sid,
                EpisodicKind::UserMessage,
                serde_json::json!({ "i": i }),
            ));
        }
        // Capacity of 8 — oldest 2 dropped.
        assert_eq!(mem.episodic.len(), 8);
    }
}
