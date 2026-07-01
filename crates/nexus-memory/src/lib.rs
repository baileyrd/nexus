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
//! All three stores read from memory (bounded ring for episodic, hash
//! maps for semantic/procedural) and, as of Phase 5, can write through
//! to `SQLite` so memory survives across process restarts: construct via
//! [`MemoryStore::open`] (or the per-store `open` constructors) for
//! durable stores, or [`MemoryStore::new`] for the original in-memory
//! semantics. The API surface is identical across both — exactly the
//! stability contract Phase 1 promised. The durable tables live in the
//! same `<forge>/.forge/memory/memory.db` the `com.nexus.memory` core
//! plugin owns (`episodic_log` / `semantic_facts` / `procedural_skills`).
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
pub use core_plugin::MemoryCorePlugin;
pub use db::{MemoryDb, MemoryDbError};
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

    /// Open a durable `MemoryStore` rooted at `forge_root` (Phase 5).
    ///
    /// Opens (creating if needed) `<forge_root>/.forge/memory/memory.db`
    /// — the same database the `com.nexus.memory` core plugin owns; the
    /// cognitive tables are disjoint from the plugin's `memories` table
    /// and every pooled connection uses WAL + a busy timeout, so both
    /// handles can coexist in one process. Prior episodic / semantic /
    /// procedural state is loaded into memory and every subsequent
    /// mutation writes through.
    ///
    /// # Errors
    /// Returns an error if the memory directory cannot be created, the
    /// database cannot be opened, or stored state cannot be decoded.
    pub fn open(forge_root: &std::path::Path) -> Result<Self, MemoryDbError> {
        Self::open_with_capacity(forge_root, DEFAULT_EPISODIC_CAPACITY)
    }

    /// [`open`](Self::open) with a custom episodic capacity.
    ///
    /// # Errors
    /// Returns an error under the same conditions as [`open`](Self::open).
    pub fn open_with_capacity(
        forge_root: &std::path::Path,
        capacity: usize,
    ) -> Result<Self, MemoryDbError> {
        let dir = forge_root.join(".forge").join("memory");
        std::fs::create_dir_all(&dir)?;
        let db = MemoryDb::open(&dir.join("memory.db"))?;
        Self::with_db(&db, capacity)
    }

    /// Build a durable `MemoryStore` over an existing [`MemoryDb`]
    /// handle (tests / embedding alongside a `MemoryCorePlugin`).
    ///
    /// # Errors
    /// Returns an error if stored state cannot be read or decoded.
    pub fn with_db(db: &MemoryDb, capacity: usize) -> Result<Self, MemoryDbError> {
        Ok(Self {
            episodic: EpisodicStore::open(db.clone(), capacity)?,
            semantic: SemanticStore::open(db.clone())?,
            procedural: ProceduralStore::open(db.clone())?,
        })
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

    // ─── Phase 5 — durable stores ──────────────────────────────────────────

    #[test]
    fn open_persists_all_three_stores_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let sid = Uuid::new_v4();

        {
            let mem = MemoryStore::open(dir.path()).unwrap();
            mem.episodic.record(EpisodicEntry::for_session(
                sid,
                EpisodicKind::UserMessage,
                serde_json::json!({ "text": "hello" }),
            ));
            mem.semantic
                .store(SemanticEntry::new("user.language", "Rust").with_tags(["user"]));
            mem.procedural.register(ProceduralEntry::new(
                "format_table",
                "Format a markdown table",
                ["table"],
                "Align the pipes.",
            ));
        } // dropped — simulates process exit

        let mem = MemoryStore::open(dir.path()).unwrap();
        let hist = mem.episodic.session_history(sid);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].content["text"], "hello");
        assert_eq!(hist[0].kind, EpisodicKind::UserMessage);

        let fact = mem.semantic.get_by_key("user.language").unwrap();
        assert_eq!(fact.content, "Rust");
        assert_eq!(fact.tags, vec!["user".to_string()]);

        let skill = mem.procedural.get_by_name("format_table").unwrap();
        assert_eq!(skill.template, "Align the pipes.");
        assert_eq!(mem.procedural.lookup("make me a table").len(), 1);
    }

    #[test]
    fn episodic_ring_capacity_enforced_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let sid = Uuid::new_v4();

        {
            let mem = MemoryStore::open_with_capacity(dir.path(), 3).unwrap();
            for i in 0..5u32 {
                mem.episodic.record(EpisodicEntry::for_session(
                    sid,
                    EpisodicKind::UserMessage,
                    serde_json::json!({ "i": i.to_string() }),
                ));
            }
            assert_eq!(mem.episodic.len(), 3);
        }

        let mem = MemoryStore::open_with_capacity(dir.path(), 3).unwrap();
        assert_eq!(mem.episodic.len(), 3);
        let hist = mem.episodic.session_history(sid);
        // Oldest two pruned from the durable log too; order chronological.
        assert_eq!(hist[0].content["i"], "2");
        assert_eq!(hist[2].content["i"], "4");
    }

    #[test]
    fn semantic_key_dedup_and_remove_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let doomed = SemanticEntry::new("scratch.note", "temp");

        {
            let mem = MemoryStore::open(dir.path()).unwrap();
            mem.semantic
                .store(SemanticEntry::new("user.language", "Python"));
            // Same key — replaces, not duplicates (in memory AND on disk).
            mem.semantic
                .store(SemanticEntry::new("user.language", "Rust"));
            mem.semantic.store(doomed.clone());
            assert!(mem.semantic.remove(doomed.id));
        }

        let mem = MemoryStore::open(dir.path()).unwrap();
        assert_eq!(mem.semantic.len(), 1);
        assert_eq!(
            mem.semantic.get_by_key("user.language").unwrap().content,
            "Rust"
        );
        assert!(mem.semantic.get_by_key("scratch.note").is_none());
    }

    #[test]
    fn procedural_use_count_and_unregister_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let skill = ProceduralEntry::new("greet", "Say hi", ["hello"], "Hi!");
        let doomed = ProceduralEntry::new("doomed", "gone", ["x"], "y");
        let skill_id = skill.id;

        {
            let mem = MemoryStore::open(dir.path()).unwrap();
            mem.procedural.register(skill);
            mem.procedural.register(doomed.clone());
            mem.procedural.record_use(skill_id);
            mem.procedural.record_use(skill_id);
            assert!(mem.procedural.unregister(doomed.id));
        }

        let mem = MemoryStore::open(dir.path()).unwrap();
        let loaded = mem.procedural.get_by_name("greet").unwrap();
        assert_eq!(loaded.use_count, 2);
        assert!(mem.procedural.get_by_name("doomed").is_none());
    }

    #[test]
    fn with_db_shares_one_database_with_core_plugin_handle() {
        // The facade and the core plugin can coexist over one MemoryDb.
        let db = MemoryDb::open_in_memory().unwrap();
        let mem = MemoryStore::with_db(&db, 16).unwrap();
        mem.semantic.store(SemanticEntry::new("k", "v"));
        // A second facade over the same handle sees the durable fact.
        let mem2 = MemoryStore::with_db(&db, 16).unwrap();
        assert_eq!(mem2.semantic.get_by_key("k").unwrap().content, "v");
    }
}
