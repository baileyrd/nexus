//! Wire types for the comments subsystem.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Stable identifier for a comment thread.
pub type ThreadId = Uuid;

/// Stable identifier for a single comment within a thread.
pub type CommentId = Uuid;

/// One reply within a thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    /// Stable id (uuid v7 — ordered by creation time).
    pub id: CommentId,
    /// Author display name. `None` when the runtime has no
    /// configured user identity (e.g. CLI without `git config user.name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Comment body — opaque markdown / plain text. The shell renders.
    pub body: String,
    /// Mentions extracted from `body` at write time (`@name` tokens).
    /// Stored explicitly so callers don't re-scan the body to surface
    /// notifications.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
    /// First-write timestamp.
    pub created_at: DateTime<Utc>,
    /// Set when the comment is edited in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A thread anchored to one block in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    /// Stable thread id.
    pub id: ThreadId,
    /// The block this thread is anchored to. The editor is
    /// responsible for ensuring `block_id` has been stamped via
    /// `com.nexus.editor::stamp_block` before the thread is created.
    pub block_id: Uuid,
    /// `true` once a participant marks the thread resolved.
    #[serde(default)]
    pub resolved: bool,
    /// Set when `resolved` flips from false to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    /// Author who resolved the thread (best-effort, may be `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    /// Thread-creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Comments in the thread, ordered oldest-first. Always
    /// non-empty: a thread is created with its first comment in a
    /// single transaction.
    pub comments: Vec<Comment>,
}

/// Persistence container for a single file's comment threads.
///
/// Lives at `<forge>/.forge/comments/<relpath>.json`. The
/// `file_path` field is redundant with the on-disk location but
/// stored anyway so a misplaced sidecar can be recovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentFile {
    /// Schema version. Bumped on breaking changes to this struct's
    /// JSON shape.
    pub version: u32,
    /// Forge-relative path of the markdown file these threads
    /// belong to (forward-slash separated).
    pub file_path: String,
    /// Threads, ordered oldest-first.
    #[serde(default)]
    pub threads: Vec<Thread>,
}

impl CommentFile {
    /// Current schema version.
    pub const VERSION: u32 = 1;

    /// Empty container for a freshly-tracked file.
    #[must_use]
    pub fn empty(file_path: String) -> Self {
        Self {
            version: Self::VERSION,
            file_path,
            threads: Vec::new(),
        }
    }
}
