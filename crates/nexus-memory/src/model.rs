//! Persistent memory record model — the row shape of the `memories` table.
//!
//! Field names and semantics mirror the `remind_me` schema (SPO entity facts,
//! ACT-R vitality, lifecycle status) so a `remind_me` `memory.db` can be
//! imported 1:1. [`MemoryType`] unifies the episodic / semantic / procedural
//! stores onto a single table; the in-memory stores in the crate root remain a
//! thin typed facade over the same records.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Cognitive class of a memory — mirrors the three in-memory stores plus an
/// `Unclassified` bucket for raw, not-yet-categorised captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Time-ordered event: conversation turn, tool call, observation.
    Episodic,
    /// Declarative fact: preference, domain knowledge.
    Semantic,
    /// Learned skill or how-to pattern.
    Procedural,
    /// Not yet classified — the default for raw captures.
    #[default]
    Unclassified,
}

impl MemoryType {
    /// The lowercase token stored in the `memory_type` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Episodic => "episodic",
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
            Self::Unclassified => "unclassified",
        }
    }

    /// Parse a `memory_type` column value, mapping unknown values to
    /// [`MemoryType::Unclassified`] so a foreign/newer database never fails to load.
    #[must_use]
    pub fn from_db(s: &str) -> Self {
        match s {
            "episodic" => Self::Episodic,
            "semantic" => Self::Semantic,
            "procedural" => Self::Procedural,
            _ => Self::Unclassified,
        }
    }
}

/// Lifecycle status of a memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    /// Live and eligible for recall.
    #[default]
    Active,
    /// Retained but excluded from default recall.
    Archived,
    /// Replaced by a newer memory (see [`Memory::superseded_by`]).
    Superseded,
}

impl MemoryStatus {
    /// The lowercase token stored in the `status` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Superseded => "superseded",
        }
    }

    /// Parse a `status` column value, defaulting unknown values to `Active`.
    #[must_use]
    pub fn from_db(s: &str) -> Self {
        match s {
            "archived" => Self::Archived,
            "superseded" => Self::Superseded,
            _ => Self::Active,
        }
    }
}

/// One row of the `memories` table — the unit of persistent memory.
///
/// New memories created inside Nexus get a time-ordered UUID (v7) id and
/// `created_at == updated_at == now`; ids are preserved verbatim on import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Stable unique id (UUID v7 for new rows; preserved on import).
    pub id: Uuid,
    /// The memory text.
    pub content: String,
    /// Coarse grouping label (default `"general"`).
    pub category: String,
    /// Free-form tags.
    pub tags: Vec<String>,
    /// Where the memory came from: `manual` | `capture` | `import` | `event`.
    pub source: String,
    /// Arbitrary structured metadata (JSON object).
    pub metadata: serde_json::Value,
    /// When the memory was first created.
    pub created_at: DateTime<Utc>,
    /// When the memory was last modified.
    pub updated_at: DateTime<Utc>,
    /// Originating client/provider label (e.g. `claude`, `openai`, `ollama`).
    pub client: String,
    /// Sync node that authored the row, if any.
    pub node_id: Option<String>,
    /// Capture batch this memory belongs to, if any.
    pub capture_id: Option<String>,
    /// Upstream capture id this memory was derived from, if any.
    pub source_capture_id: Option<String>,
    /// Cognitive class.
    pub memory_type: MemoryType,
    /// Lifecycle status.
    pub status: MemoryStatus,
    /// Id of the memory that supersedes this one, if any.
    pub superseded_by: Option<Uuid>,
    /// Subject of an SPO entity fact (populated in P2).
    pub subject: Option<String>,
    /// Predicate of an SPO entity fact (populated in P2).
    pub predicate: Option<String>,
    /// Object of an SPO entity fact (populated in P2).
    pub object: Option<String>,
    /// When the memory was last accessed (ACT-R vitality input; P3).
    pub accessed_at: Option<DateTime<Utc>>,
    /// Number of times the memory has been accessed.
    pub access_count: i64,
    /// Per-memory decay rate (ACT-R; P3).
    pub decay_rate: f64,
    /// Current vitality score (ACT-R; P3).
    pub vitality: f64,
    /// Base activation weight (ACT-R; P3).
    pub base_weight: f64,
}

impl Memory {
    /// Create a new active, unclassified memory from `content`, with a fresh
    /// time-ordered id and `created_at == updated_at == now`.
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            content: content.into(),
            category: "general".to_string(),
            tags: Vec::new(),
            source: "manual".to_string(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
            client: "unknown".to_string(),
            node_id: None,
            capture_id: None,
            source_capture_id: None,
            memory_type: MemoryType::Unclassified,
            status: MemoryStatus::Active,
            superseded_by: None,
            subject: None,
            predicate: None,
            object: None,
            accessed_at: None,
            access_count: 0,
            decay_rate: 0.1,
            vitality: 1.0,
            base_weight: 1.0,
        }
    }

    /// Builder: set the cognitive class.
    #[must_use]
    pub fn with_type(mut self, memory_type: MemoryType) -> Self {
        self.memory_type = memory_type;
        self
    }

    /// Builder: set the category.
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Builder: set the source.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Builder: set the originating client/provider.
    #[must_use]
    pub fn with_client(mut self, client: impl Into<String>) -> Self {
        self.client = client.into();
        self
    }

    /// Builder: replace the tag list.
    #[must_use]
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
}
