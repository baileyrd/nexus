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

use nexus_types::plugin_ids;
use serde::{Deserialize, Serialize};

use crate::conflict::{Conflict, ConflictDetail};
use crate::error::{CrdtError, Result};
use crate::op::CrdtOp;

/// Topic prefix for the per-file CRDT-op channel. Mirrors
/// `EVENT_CHANGED_PREFIX` in the editor plugin.
pub const OPS_TOPIC_PREFIX: &str = "com.nexus.editor.ops.";

/// Topic prefix for the per-file conflict channel published by the
/// BL-007 pull-landing path when `apply_remote` returns a
/// [`Conflict`] (structural delete-edit or concurrent whole-block
/// replacement). The shell subscribes to render a resolver UI; the
/// CRDT layer can't pick a winner on its own.
pub const CONFLICT_TOPIC_PREFIX: &str = "com.nexus.editor.crdt.conflict.";

const _: () = {
    let prefix = OPS_TOPIC_PREFIX.as_bytes();
    let editor = plugin_ids::EDITOR.as_bytes();
    assert!(prefix.len() > editor.len());
    let mut i = 0;
    while i < editor.len() {
        assert!(prefix[i] == editor[i]);
        i += 1;
    }
    assert!(prefix[editor.len()] == b'.');
};

const _: () = {
    let prefix = CONFLICT_TOPIC_PREFIX.as_bytes();
    let editor = plugin_ids::EDITOR.as_bytes();
    assert!(prefix.len() > editor.len());
    let mut i = 0;
    while i < editor.len() {
        assert!(prefix[i] == editor[i]);
        i += 1;
    }
    assert!(prefix[editor.len()] == b'.');
};

/// Compose the per-file topic name.
#[must_use]
pub fn ops_topic(relpath: &str) -> String {
    format!("{OPS_TOPIC_PREFIX}{relpath}")
}

/// Compose the per-file conflict topic name.
#[must_use]
pub fn conflict_topic(relpath: &str) -> String {
    format!("{CONFLICT_TOPIC_PREFIX}{relpath}")
}

/// If `topic` is one of our per-file ops topics, return the relative
/// path it carries. Returns `None` for unrelated topics.
#[must_use]
pub fn relpath_of_topic(topic: &str) -> Option<&str> {
    topic.strip_prefix(OPS_TOPIC_PREFIX)
}

/// If `topic` is one of our per-file conflict topics, return the
/// relative path it carries. Returns `None` for unrelated topics.
#[must_use]
pub fn relpath_of_conflict_topic(topic: &str) -> Option<&str> {
    topic.strip_prefix(CONFLICT_TOPIC_PREFIX)
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

/// JSON envelope for one or more conflicts surfaced during a
/// pull-landing reload. Wrapping in a struct (instead of a bare
/// `Vec<Conflict>`) leaves room for future fields like the merged
/// version vector or a "remote tip" identifier without breaking
/// subscribers.
///
/// The `conflicts` field holds [`ConflictDetail`]s rather than bare
/// [`Conflict`]s as of BL-074: each entry flattens the underlying
/// `Conflict` into its JSON shape and adds optional content snapshots
/// (`local_content` / `remote_content` / `delete_origin`) so a
/// resolver UI can render side-by-side without an extra round-trip.
/// Pre-BL-074 subscribers (the toast) keep parsing — they only read
/// `kind` and `block_id`, both of which still appear unchanged in the
/// flattened form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConflictEnvelope {
    /// Conflicts surfaced by the reload, in the order
    /// [`crate::CrdtDoc::apply_remote`] returned them.
    pub conflicts: Vec<ConflictDetail>,
}

impl ConflictEnvelope {
    /// Wrap a list of bare [`Conflict`]s in an envelope, with no
    /// content snapshots. Use [`Self::with_details`] to attach the
    /// content the BL-074 resolver modal renders against.
    #[must_use]
    pub fn new(conflicts: Vec<Conflict>) -> Self {
        Self {
            conflicts: conflicts.into_iter().map(ConflictDetail::from).collect(),
        }
    }

    /// Wrap a pre-built list of [`ConflictDetail`]s. Used by the
    /// BL-007 pull-landing path which extracts content snapshots
    /// alongside conflict detection.
    #[must_use]
    pub fn with_details(conflicts: Vec<ConflictDetail>) -> Self {
        Self { conflicts }
    }

    /// Encode the envelope to a JSON value suitable for
    /// [`nexus_kernel::EventBus::publish_plugin`].
    ///
    /// # Errors
    ///
    /// Propagates [`CrdtError::Wire`] if serialization fails. The
    /// `Conflict` enum only carries `BlockId`s and `OpId`s, so this
    /// is unreachable in practice — left as a fallible API for
    /// symmetry with [`OpEnvelope::to_json`].
    pub fn to_json(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|e| CrdtError::Wire(e.to_string()))
    }

    /// Decode an envelope from a JSON value received off the bus.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::Wire`] if the payload doesn't match the
    /// expected schema.
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| CrdtError::Wire(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::id::{Lamport, OpId, SiteId};

    #[test]
    fn topic_round_trips_relpath() {
        let t = ops_topic("notes/today.md");
        assert_eq!(t, "com.nexus.editor.ops.notes/today.md");
        assert_eq!(relpath_of_topic(&t), Some("notes/today.md"));
        assert_eq!(relpath_of_topic("com.nexus.editor.changed.x"), None);
    }

    #[test]
    fn conflict_topic_round_trips_relpath() {
        let t = conflict_topic("notes/today.md");
        assert_eq!(t, "com.nexus.editor.crdt.conflict.notes/today.md");
        assert_eq!(relpath_of_conflict_topic(&t), Some("notes/today.md"));
        assert_eq!(relpath_of_conflict_topic("com.nexus.editor.ops.x"), None);
    }

    #[test]
    fn conflict_envelope_round_trips_through_json() {
        let s = SiteId::new();
        let conflicts = vec![Conflict::StructuralDeleteEdit {
            block_id: Uuid::new_v4(),
            delete: OpId::new(s, Lamport(1)),
            edit: OpId::new(s, Lamport(2)),
        }];
        let envelope = ConflictEnvelope::new(conflicts.clone());
        let json = envelope.to_json().unwrap();
        let decoded = ConflictEnvelope::from_json(&json).unwrap();
        assert_eq!(decoded.conflicts.len(), 1);
        assert_eq!(decoded.conflicts[0].conflict, conflicts[0]);
        assert!(decoded.conflicts[0].local_content.is_none());
        assert!(decoded.conflicts[0].remote_content.is_none());
    }

    #[test]
    fn conflict_envelope_with_details_carries_content_snapshots() {
        let block_id = Uuid::new_v4();
        let s = SiteId::new();
        let detail = crate::conflict::ConflictDetail {
            conflict: Conflict::ConcurrentBlockEdit {
                block_id,
                local: OpId::new(s, Lamport(1)),
                remote: OpId::new(s, Lamport(2)),
            },
            local_content: Some("hello local".into()),
            remote_content: Some("hello remote".into()),
            delete_origin: None,
        };
        let envelope = ConflictEnvelope::with_details(vec![detail.clone()]);
        let json = envelope.to_json().unwrap();
        // Flattened wire shape: pre-BL-074 fields sit at the top
        // level (kind, block_id, local, remote) alongside the new
        // content fields. This is what keeps the toast subscriber
        // parsing unchanged.
        let conflicts = json.get("conflicts").unwrap().as_array().unwrap();
        let row = &conflicts[0];
        assert_eq!(row.get("kind").unwrap(), "concurrent_block_edit");
        assert!(row.get("block_id").is_some());
        assert_eq!(row.get("local_content").unwrap(), "hello local");
        assert_eq!(row.get("remote_content").unwrap(), "hello remote");
        // Round trip preserves everything.
        let decoded = ConflictEnvelope::from_json(&json).unwrap();
        assert_eq!(decoded.conflicts, vec![detail]);
    }

    #[test]
    fn conflict_envelope_legacy_payload_decodes_with_default_details() {
        // A toast-shape payload (no local_content / remote_content /
        // delete_origin) still decodes — the new fields default to
        // None. This is the back-compat guarantee for the BL-074
        // upgrade.
        let legacy = serde_json::json!({
            "conflicts": [
                {
                    "kind": "concurrent_block_edit",
                    "block_id": Uuid::new_v4(),
                    "local": { "site": SiteId::new(), "lamport": 1 },
                    "remote": { "site": SiteId::new(), "lamport": 2 }
                }
            ]
        });
        let decoded = ConflictEnvelope::from_json(&legacy).unwrap();
        assert_eq!(decoded.conflicts.len(), 1);
        assert!(decoded.conflicts[0].local_content.is_none());
        assert!(decoded.conflicts[0].remote_content.is_none());
        assert!(decoded.conflicts[0].delete_origin.is_none());
    }
}
