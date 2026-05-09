//! Wire-format helpers for the Phase 3 sync layer.
//!
//! The sync transport gossips [`crate::CrdtOp`] payloads over the
//! kernel event bus on a per-file topic
//! `com.nexus.editor.ops.<relpath>`. This module owns the topic name
//! shape and the JSON payload schema so producers (the editor core
//! plugin) and consumers ([`crate::SyncLoop`]) can't disagree.
//!
//! ## Topic
//!
//! ```text
//! com.nexus.editor.ops.<relpath>
//! ```
//!
//! `relpath` is the forge-relative path of the file (matches the topic
//! suffix already used by `com.nexus.editor.changed.<relpath>`).
//!
//! ## Payload
//!
//! ```json
//! {
//!   "op": <CrdtOp serialized as JSON>
//! }
//! ```
//!
//! Wrapping the op in an envelope leaves room for future fields
//! (cursor positions, presence) without breaking subscribers.

use serde::{Deserialize, Serialize};

use crate::error::{CrdtError, Result};
use crate::op::CrdtOp;

/// Topic prefix for the per-file CRDT-op channel. Mirrors
/// `EVENT_CHANGED_PREFIX` in the editor plugin.
pub const OPS_TOPIC_PREFIX: &str = "com.nexus.editor.ops.";

/// Compose the per-file topic name.
#[must_use]
pub fn ops_topic(relpath: &str) -> String {
    format!("{OPS_TOPIC_PREFIX}{relpath}")
}

/// If `topic` is one of our per-file ops topics, return the relative
/// path it carries. Returns `None` for unrelated topics.
#[must_use]
pub fn relpath_of_topic(topic: &str) -> Option<&str> {
    topic.strip_prefix(OPS_TOPIC_PREFIX)
}

/// JSON envelope for a single op gossiped over the bus.
///
/// Wrapping `CrdtOp` in a struct (rather than using the bare op as the
/// payload) keeps room for future fields like presence/cursor data
/// without breaking subscribers — a struct gains optional fields
/// compatibly via `#[serde(default)]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpEnvelope {
    /// The CRDT op being gossiped.
    pub op: CrdtOp,
}

impl OpEnvelope {
    /// Wrap an op in an envelope.
    #[must_use]
    pub fn new(op: CrdtOp) -> Self {
        Self { op }
    }

    /// Encode the envelope to a JSON value suitable for
    /// [`nexus_kernel::EventBus::publish_plugin`].
    ///
    /// # Errors
    ///
    /// Propagates [`CrdtError::Wire`] if serialization fails — only
    /// possible for non-finite floats inside an op payload, which the
    /// editor's [`nexus_editor::Operation`] never produces.
    pub fn to_json(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|e| CrdtError::Wire(e.to_string()))
    }

    /// Decode an envelope from a JSON value received off the bus.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::Wire`] if the payload doesn't match the
    /// expected schema (missing `op`, wrong shape, etc.).
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| CrdtError::Wire(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_round_trips_relpath() {
        let t = ops_topic("notes/today.md");
        assert_eq!(t, "com.nexus.editor.ops.notes/today.md");
        assert_eq!(relpath_of_topic(&t), Some("notes/today.md"));
        assert_eq!(relpath_of_topic("com.nexus.editor.changed.x"), None);
    }
}
