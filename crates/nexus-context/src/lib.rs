//! Typed context construction pipeline for Nexus AI sessions.
//!
//! The context pipeline is the bridge between the memory layer (Move 4)
//! and the model call (Move 7). It assembles a `Context` — an ordered,
//! budget-bounded sequence of typed [`ContextEntry`]s — that the session
//! worker hands to the model at each perceive-reason step.
//!
//! ## Design
//!
//! Rather than concatenating raw strings, the pipeline works with typed
//! entries. This lets:
//!
//! - Budget enforcement happen at the entry level (token estimates per
//!   entry, hard stop when the budget is full)
//! - The model adapter layer decide how to format each entry type for
//!   a specific provider (OpenAI, Claude, etc.) without leaking those
//!   formatting details into the memory or runtime layers
//! - Replay-testing work without running a real model (the `Context`
//!   is a pure value — no async, no I/O)
//!
//! ## Usage
//!
//! ```rust
//! use nexus_context::{ContextBuilder, ContextBudget};
//!
//! let budget = ContextBudget::new(8192);
//! let ctx = ContextBuilder::new(budget)
//!     .system("You are a helpful assistant.")
//!     .user_turn("What is 2 + 2?")
//!     .build();
//! assert_eq!(ctx.entries.len(), 2);
//! ```

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use nexus_memory::{EpisodicStore, ProceduralStore, SemanticStore};

// ─── Token estimation ─────────────────────────────────────────────────────────

/// Rough character-to-token ratio used for budget estimation.
/// 4 characters ≈ 1 token is the standard heuristic for English text;
/// Phase 5 can inject a real tokenizer without changing the budget API.
const CHARS_PER_TOKEN: u32 = 4;

/// Estimate the token count of a string using the 4-chars-per-token
/// heuristic. Always returns at least 1 to avoid zero-cost entries.
#[must_use]
fn estimate_tokens(s: &str) -> u32 {
    (u32::try_from(s.len()).unwrap_or(u32::MAX) / CHARS_PER_TOKEN).max(1)
}

// ─── Context budget ───────────────────────────────────────────────────────────

/// Token budget for a single model context window.
///
/// The total budget is `max_tokens`. The builder halts adding entries
/// once the estimated token count would exceed this ceiling. Callers
/// can tune the per-section reserves to prioritise conversation history
/// over memory injections or vice-versa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Hard ceiling on tokens for the entire context window.
    pub max_tokens: u32,
    /// Soft reserve for the system prompt. The builder always includes
    /// system entries even when over budget — they are mandatory.
    pub system_reserve: u32,
    /// Soft reserve for conversation history entries (user/assistant turns).
    pub history_reserve: u32,
    /// Soft reserve for memory injections (episodic, semantic, procedural).
    pub memory_reserve: u32,
}

impl ContextBudget {
    /// Create a budget with the given ceiling and default reserves.
    /// The defaults allocate 20% to system, 60% to history, and 20%
    /// to memory injections.
    #[must_use]
    pub fn new(max_tokens: u32) -> Self {
        Self {
            max_tokens,
            system_reserve: max_tokens / 5,
            history_reserve: max_tokens * 3 / 5,
            memory_reserve: max_tokens / 5,
        }
    }

    /// Remaining token headroom from the hard ceiling.
    #[must_use]
    pub fn remaining(&self, used: u32) -> u32 {
        self.max_tokens.saturating_sub(used)
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::new(8192)
    }
}

// ─── Context entries ──────────────────────────────────────────────────────────

/// Source kind for a [`ContextEntry::MemoryInjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    /// From [`EpisodicStore`] — a past conversation event.
    Episodic,
    /// From [`SemanticStore`] — a stored fact.
    Semantic,
    /// From [`ProceduralStore`] — a learned skill template.
    Procedural,
}

/// One typed building block of the model's context window.
///
/// The model adapter layer (Phase 6 / `nexus-protocol`) is responsible
/// for formatting these entries into the wire format expected by the
/// specific model provider. The context pipeline itself stays provider-
/// agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextEntry {
    /// System-level instructions that frame the model's behaviour.
    /// Always included regardless of budget (mandatory entries).
    System {
        /// Instruction content.
        content: String,
    },
    /// A message turn from the user.
    UserTurn {
        /// Message text.
        content: String,
        /// Session this turn belongs to; `None` for synthetic turns.
        #[serde(default)]
        session_id: Option<Uuid>,
    },
    /// A response turn from the model/assistant.
    AssistantTurn {
        /// Response text.
        content: String,
    },
    /// A tool/capability call the model made.
    ToolCall {
        /// Tool name.
        name: String,
        /// Arguments passed to the tool.
        arguments: serde_json::Value,
    },
    /// The result returned by a tool call.
    ToolResult {
        /// Opaque correlation id linking this result to its call.
        call_id: String,
        /// Result content.
        content: String,
    },
    /// An injected memory entry (episodic event, semantic fact, or
    /// procedural skill template). The context builder prefixes these
    /// with a lightweight header so the model can distinguish injected
    /// knowledge from the live conversation.
    MemoryInjection {
        /// Which memory store this came from.
        source: MemorySource,
        /// Injected content.
        content: String,
    },
}

impl ContextEntry {
    /// Rough token estimate for this entry using the 4-chars-per-token
    /// heuristic. Used by [`ContextBuilder`] for budget enforcement.
    #[must_use]
    pub fn estimated_tokens(&self) -> u32 {
        match self {
            Self::System { content } => estimate_tokens(content),
            Self::UserTurn { content, .. } => estimate_tokens(content),
            Self::AssistantTurn { content } => estimate_tokens(content),
            Self::ToolCall { name, arguments } => {
                estimate_tokens(name) + estimate_tokens(&arguments.to_string())
            }
            Self::ToolResult { call_id, content } => {
                estimate_tokens(call_id) + estimate_tokens(content)
            }
            Self::MemoryInjection { content, .. } => estimate_tokens(content),
        }
    }

    /// `true` for entries that are always included regardless of budget
    /// (currently only [`ContextEntry::System`]).
    #[must_use]
    pub fn is_mandatory(&self) -> bool {
        matches!(self, Self::System { .. })
    }
}

// ─── Assembled context ────────────────────────────────────────────────────────

/// The assembled context ready to be formatted and sent to a model.
///
/// Produced by [`ContextBuilder::build`]. Entries are in the order the
/// builder received them — system first, then history, then memory
/// injections, with optional conversation tail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    /// Ordered entries composing the context window.
    pub entries: Vec<ContextEntry>,
    /// Estimated token count across all entries.
    pub estimated_tokens: u32,
    /// The budget this context was built against.
    pub budget: ContextBudget,
}

impl Context {
    /// Count entries matching a predicate.
    #[must_use]
    pub fn count(&self, predicate: impl Fn(&ContextEntry) -> bool) -> usize {
        self.entries.iter().filter(|e| predicate(e)).count()
    }

    /// Whether the budget was fully consumed during assembly.
    #[must_use]
    pub fn is_budget_exhausted(&self) -> bool {
        self.estimated_tokens >= self.budget.max_tokens
    }
}

// ─── Context builder ──────────────────────────────────────────────────────────

/// Assembles a [`Context`] from typed entries and memory stores.
///
/// The builder is a pure value — no async, no I/O. Call the fluent
/// methods to add entries and pull from memory stores, then call
/// [`ContextBuilder::build`] to produce the finished `Context`.
///
/// Budget enforcement is best-effort: mandatory entries (system) are
/// always included; optional entries are skipped once the token
/// estimate exceeds [`ContextBudget::max_tokens`].
#[derive(Debug)]
pub struct ContextBuilder {
    entries: Vec<ContextEntry>,
    token_count: u32,
    budget: ContextBudget,
}

impl ContextBuilder {
    /// Create a builder with the given budget.
    #[must_use]
    pub fn new(budget: ContextBudget) -> Self {
        Self {
            entries: Vec::new(),
            token_count: 0,
            budget,
        }
    }

    /// Add a system instruction. Always included regardless of budget.
    #[must_use]
    pub fn system(mut self, content: impl Into<String>) -> Self {
        let entry = ContextEntry::System {
            content: content.into(),
        };
        // System entries are mandatory — bypass budget check.
        self.token_count += entry.estimated_tokens();
        self.entries.push(entry);
        self
    }

    /// Add a user turn. Skipped if the budget is exhausted.
    #[must_use]
    pub fn user_turn(self, content: impl Into<String>) -> Self {
        self.try_push(ContextEntry::UserTurn {
            content: content.into(),
            session_id: None,
        })
    }

    /// Add a user turn linked to a specific session.
    #[must_use]
    pub fn user_turn_for_session(self, content: impl Into<String>, session_id: Uuid) -> Self {
        self.try_push(ContextEntry::UserTurn {
            content: content.into(),
            session_id: Some(session_id),
        })
    }

    /// Add an assistant turn. Skipped if the budget is exhausted.
    #[must_use]
    pub fn assistant_turn(self, content: impl Into<String>) -> Self {
        self.try_push(ContextEntry::AssistantTurn {
            content: content.into(),
        })
    }

    /// Add a tool call. Skipped if the budget is exhausted.
    #[must_use]
    pub fn tool_call(self, name: impl Into<String>, arguments: serde_json::Value) -> Self {
        self.try_push(ContextEntry::ToolCall {
            name: name.into(),
            arguments,
        })
    }

    /// Add a tool result. Skipped if the budget is exhausted.
    #[must_use]
    pub fn tool_result(self, call_id: impl Into<String>, content: impl Into<String>) -> Self {
        self.try_push(ContextEntry::ToolResult {
            call_id: call_id.into(),
            content: content.into(),
        })
    }

    /// Pull recent episodic events for `session_id` from `store` and
    /// inject them as [`ContextEntry::MemoryInjection`] entries. At
    /// most `limit` events are injected; older events are skipped once
    /// the memory reserve is full.
    #[must_use]
    pub fn inject_episodic(
        mut self,
        store: &EpisodicStore,
        session_id: Uuid,
        limit: usize,
    ) -> Self {
        let history = store.session_history(session_id);
        let recent: Vec<_> = history.iter().rev().take(limit).rev().collect();
        for entry in recent {
            let content = format!(
                "[{kind:?}] {content}",
                kind = entry.kind,
                content = entry.content,
            );
            let memory = ContextEntry::MemoryInjection {
                source: MemorySource::Episodic,
                content,
            };
            let tokens = memory.estimated_tokens();
            if self.token_count + tokens > self.budget.max_tokens {
                break;
            }
            self.token_count += tokens;
            self.entries.push(memory);
        }
        self
    }

    /// Search `store` for facts matching `query` and inject the top
    /// `limit` results as [`ContextEntry::MemoryInjection`] entries.
    #[must_use]
    pub fn inject_semantic(mut self, store: &SemanticStore, query: &str, limit: usize) -> Self {
        for fact in store.search(query, limit) {
            let content = format!("[fact: {}] {}", fact.key, fact.content);
            let memory = ContextEntry::MemoryInjection {
                source: MemorySource::Semantic,
                content,
            };
            let tokens = memory.estimated_tokens();
            if self.token_count + tokens > self.budget.max_tokens {
                break;
            }
            self.token_count += tokens;
            self.entries.push(memory);
        }
        self
    }

    /// Look up skills in `store` triggered by `trigger` and inject
    /// matching skill templates as [`ContextEntry::MemoryInjection`]
    /// entries. Records each matched skill via
    /// [`ProceduralStore::record_use`] so frequently-applied skills
    /// rank higher in future lookups.
    #[must_use]
    pub fn inject_procedural(mut self, store: &ProceduralStore, trigger: &str) -> Self {
        for skill in store.lookup(trigger) {
            let content = format!("[skill: {}] {}", skill.name, skill.template);
            let memory = ContextEntry::MemoryInjection {
                source: MemorySource::Procedural,
                content,
            };
            let tokens = memory.estimated_tokens();
            if self.token_count + tokens > self.budget.max_tokens {
                break;
            }
            self.token_count += tokens;
            self.entries.push(memory);
            store.record_use(skill.id);
        }
        self
    }

    /// Consume the builder and produce the finished [`Context`].
    #[must_use]
    pub fn build(self) -> Context {
        Context {
            estimated_tokens: self.token_count,
            budget: self.budget,
            entries: self.entries,
        }
    }

    /// Push `entry` if there is token budget remaining; otherwise drop
    /// it silently. System entries bypass this check — use `system()`
    /// for those.
    fn try_push(mut self, entry: ContextEntry) -> Self {
        let tokens = entry.estimated_tokens();
        if self.token_count + tokens <= self.budget.max_tokens {
            self.token_count += tokens;
            self.entries.push(entry);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_memory::{EpisodicEntry, EpisodicKind, MemoryStore, SemanticEntry};

    fn small_budget() -> ContextBudget {
        ContextBudget::new(128)
    }

    #[test]
    fn basic_build_preserves_entry_order() {
        let ctx = ContextBuilder::new(ContextBudget::default())
            .system("You are an AI.")
            .user_turn("Hello")
            .assistant_turn("Hi there!")
            .build();
        assert_eq!(ctx.entries.len(), 3);
        assert!(matches!(ctx.entries[0], ContextEntry::System { .. }));
        assert!(matches!(ctx.entries[1], ContextEntry::UserTurn { .. }));
        assert!(matches!(ctx.entries[2], ContextEntry::AssistantTurn { .. }));
    }

    #[test]
    fn token_estimate_accumulates() {
        let ctx = ContextBuilder::new(ContextBudget::default())
            .system("short")
            .user_turn("also short")
            .build();
        assert!(ctx.estimated_tokens > 0);
    }

    #[test]
    fn budget_exhaustion_skips_optional_entries() {
        // Budget of 10 tokens — system entry uses ~1 token, then no room.
        let budget = ContextBudget::new(10);
        let ctx = ContextBuilder::new(budget)
            .system("hi") // ~1 token — always included
            .user_turn("A very long user message that definitely exceeds the tiny budget")
            .build();
        // System included; user turn skipped.
        assert_eq!(ctx.entries.len(), 1);
        assert!(matches!(ctx.entries[0], ContextEntry::System { .. }));
    }

    #[test]
    fn inject_episodic_pulls_session_history() {
        let mem = MemoryStore::new();
        let sid = Uuid::new_v4();
        mem.episodic.record(EpisodicEntry::for_session(
            sid,
            EpisodicKind::UserMessage,
            serde_json::json!({ "text": "remember this" }),
        ));
        let ctx = ContextBuilder::new(ContextBudget::default())
            .inject_episodic(&mem.episodic, sid, 5)
            .build();
        assert_eq!(ctx.entries.len(), 1);
        if let ContextEntry::MemoryInjection { source, content } = &ctx.entries[0] {
            assert_eq!(*source, MemorySource::Episodic);
            assert!(content.contains("remember this"), "actual: {content}");
        } else {
            panic!("expected MemoryInjection");
        }
    }

    #[test]
    fn inject_semantic_includes_matching_facts() {
        let mem = MemoryStore::new();
        mem.semantic.store(SemanticEntry::new("user.lang", "Rust"));
        mem.semantic
            .store(SemanticEntry::new("project.desc", "A CLI tool"));
        let ctx = ContextBuilder::new(ContextBudget::default())
            .inject_semantic(&mem.semantic, "rust", 5)
            .build();
        assert_eq!(ctx.entries.len(), 1);
        if let ContextEntry::MemoryInjection { source, content } = &ctx.entries[0] {
            assert_eq!(*source, MemorySource::Semantic);
            assert!(content.contains("Rust"), "actual: {content}");
        } else {
            panic!("expected MemoryInjection");
        }
    }

    #[test]
    fn inject_procedural_records_use() {
        use nexus_memory::ProceduralEntry;
        let mem = MemoryStore::new();
        mem.procedural.register(ProceduralEntry::new(
            "write_tests",
            "Generates unit tests",
            ["write tests", "test"],
            "Always write tests for new code.",
        ));
        let skill_id = mem.procedural.get_by_name("write_tests").unwrap().id;
        assert_eq!(mem.procedural.get(skill_id).unwrap().use_count, 0);

        let _ctx = ContextBuilder::new(ContextBudget::default())
            .inject_procedural(&mem.procedural, "please write tests")
            .build();
        assert_eq!(mem.procedural.get(skill_id).unwrap().use_count, 1);
    }

    #[test]
    fn context_budget_remaining_saturates_at_zero() {
        let b = ContextBudget::new(100);
        assert_eq!(b.remaining(50), 50);
        assert_eq!(b.remaining(100), 0);
        assert_eq!(b.remaining(200), 0);
    }

    #[test]
    fn tool_call_and_result_entries() {
        let ctx = ContextBuilder::new(ContextBudget::default())
            .tool_call("read_file", serde_json::json!({ "path": "notes.md" }))
            .tool_result("call-1", "# Notes\nsome content")
            .build();
        assert_eq!(ctx.entries.len(), 2);
        assert!(matches!(ctx.entries[0], ContextEntry::ToolCall { .. }));
        assert!(matches!(ctx.entries[1], ContextEntry::ToolResult { .. }));
    }

    #[test]
    fn context_is_budget_exhausted_when_at_ceiling() {
        let budget = ContextBudget::new(1);
        let ctx = ContextBuilder::new(budget)
            .system("x") // ~1 token; system always included
            .build();
        assert!(ctx.is_budget_exhausted());
    }

    #[test]
    fn small_budget_test() {
        let budget = small_budget();
        assert!(budget.max_tokens > 0);
    }

    #[test]
    fn doctest_example() {
        let budget = ContextBudget::new(8192);
        let ctx = ContextBuilder::new(budget)
            .system("You are a helpful assistant.")
            .user_turn("What is 2 + 2?")
            .build();
        assert_eq!(ctx.entries.len(), 2);
    }
}
