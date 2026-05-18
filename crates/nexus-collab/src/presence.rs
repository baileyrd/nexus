//! BL-143 Phase 1.3 — presence wire shape + bus-topic constants.
//!
//! The relay is topic-agnostic (it just forwards
//! [`crate::protocol::ClientMessage::Envelope`] frames between peers);
//! the semantics of "this is a presence update" live here.
//!
//! Local subscribers learn about peer activity through three bus
//! topics under the `com.nexus.collab` namespace:
//!
//! - [`PRESENCE_TOPIC`] — peer-authored cursor / focus updates
//!   carrying a [`PresenceEvent`] payload. The local presence
//!   publisher fires these into the bus; the [`crate::CollabClient`]
//!   outbound filter ships them to the relay; the inbound bridge
//!   re-publishes them on the bus for the shell to render.
//! - [`PEER_JOINED_TOPIC`] / [`PEER_LEFT_TOPIC`] — derived from
//!   [`crate::protocol::ServerMessage::PeerJoined`] /
//!   [`crate::protocol::ServerMessage::PeerLeft`]. These are
//!   relay-authored, not peer-authored; the bridge synthesises them
//!   as bus events so the shell's peers panel can subscribe without
//!   reaching into the wire protocol.
//!
//! Cursor coordinates carry the relpath plus optional `block_id`,
//! character `offset`, and `selection_end`. BL-143 Phase 1.3 shipped
//! only `relpath` + `block_id`; Phase 2.2 added the two character-level
//! fields as `#[serde(default)]` so Phase 1.3 peers keep decoding
//! Phase 2.2 frames (`offset` / `selection_end` silently drop to
//! `None`) and Phase 1.3 frames keep decoding on Phase 2.2 receivers
//! (the missing fields default to `None`). The relay is topic-agnostic
//! and doesn't decode the cursor at all.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Bus topic carrying [`PresenceEvent`] payloads. Peer-authored cursor
/// state — shipped by the local publisher → relay → all peers.
pub const PRESENCE_TOPIC: &str = "com.nexus.collab.presence";

/// Bus topic the [`crate::CollabClient`] uses to surface
/// [`crate::protocol::ServerMessage::PeerJoined`] frames. Payload is
/// the [`crate::protocol::PeerInfo`] of the newcomer (JSON object with
/// `peer_id` + `display_name`).
pub const PEER_JOINED_TOPIC: &str = "com.nexus.collab.peers.joined";

/// Bus topic the [`crate::CollabClient`] uses to surface
/// [`crate::protocol::ServerMessage::PeerLeft`] frames. Payload is
/// `{ "peer_id": "<peer>" }`.
pub const PEER_LEFT_TOPIC: &str = "com.nexus.collab.peers.left";

/// Common prefix for every topic in this module. Used by the client's
/// inbound republish router (`com.nexus.collab.*` → publish under
/// the collab plugin id) and by the default outbound subscription
/// (`EventFilter::CustomPrefix(COLLAB_TOPIC_PREFIX)`).
pub const COLLAB_TOPIC_PREFIX: &str = "com.nexus.collab.";

/// Cursor / focus location carried inside a [`PresenceEvent`].
///
/// Phase 1.3 shipped `relpath` + `block_id`. Phase 2.2 added `offset`
/// + `selection_end` so the CM6 caret position rides on the wire.
/// All three optional fields are `#[serde(default,
/// skip_serializing_if = "Option::is_none")]` so the wire stays
/// compatible in both directions — Phase 1.3 peers see Phase 2.2
/// frames with the new fields silently dropped, and Phase 2.2 peers
/// see Phase 1.3 frames with the new fields defaulted to `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct PresenceCursor {
    /// Forge-relative path of the file the peer is focused on.
    pub relpath: String,
    /// Optional block id (matches the ADR 0017 stamp). `None` means
    /// the peer is on the file but not inside any block (e.g.
    /// scrolling, navigating).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    /// Caret position as a character offset into the file
    /// (CodeMirror `EditorSelection.main.head`). `None` when the
    /// publisher only knows the file (Phase 1.3 peers, idle focus).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// When the peer has a non-empty selection, this is the *other*
    /// end of the range (anchor end). `None` for a caret-only
    /// position. Receivers should render the selection as a coloured
    /// background between `offset` and `selection_end` and a caret
    /// at `offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_end: Option<u32>,
}

/// Peer-authored presence frame. Wire payload of [`PRESENCE_TOPIC`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct PresenceEvent {
    /// Collab-protocol peer id (matches
    /// [`crate::protocol::PeerInfo::peer_id`]).
    pub user_id: String,
    /// Human-readable name to surface alongside the cursor.
    pub display_name: String,
    /// Current cursor / focus, or `None` if the peer is connected but
    /// not focused on any file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<PresenceCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn presence_event_serialises_with_cursor() {
        let ev = PresenceEvent {
            user_id: "alice".into(),
            display_name: "Alice".into(),
            cursor: Some(PresenceCursor {
                relpath: "notes/today.md".into(),
                block_id: Some("b-7".into()),
                offset: None,
                selection_end: None,
            }),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["user_id"], "alice");
        assert_eq!(v["cursor"]["relpath"], "notes/today.md");
        let back: PresenceEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn presence_event_round_trips_without_cursor() {
        let ev = PresenceEvent {
            user_id: "alice".into(),
            display_name: "Alice".into(),
            cursor: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert!(v.get("cursor").is_none(), "cursor=None is omitted");
        let back: PresenceEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn presence_cursor_block_id_optional() {
        let v = json!({"relpath": "x.md"});
        let cur: PresenceCursor = serde_json::from_value(v).unwrap();
        assert_eq!(cur.relpath, "x.md");
        assert_eq!(cur.block_id, None);
        assert_eq!(cur.offset, None);
        assert_eq!(cur.selection_end, None);
    }

    #[test]
    fn presence_cursor_round_trips_offset_and_selection() {
        let cur = PresenceCursor {
            relpath: "x.md".into(),
            block_id: None,
            offset: Some(42),
            selection_end: Some(57),
        };
        let v = serde_json::to_value(&cur).unwrap();
        assert_eq!(v["offset"], 42);
        assert_eq!(v["selection_end"], 57);
        assert!(v.get("block_id").is_none(), "block_id=None is omitted");
        let back: PresenceCursor = serde_json::from_value(v).unwrap();
        assert_eq!(back, cur);
    }

    #[test]
    fn phase_1_3_cursor_decodes_without_offset_fields() {
        // BL-143 Phase 2.2 compat: a Phase 1.3 peer's cursor frame
        // has neither `offset` nor `selection_end`; receivers must
        // accept that and surface `None` for both.
        let v = json!({"relpath": "x.md", "block_id": "b-1"});
        let cur: PresenceCursor = serde_json::from_value(v).unwrap();
        assert_eq!(cur.relpath, "x.md");
        assert_eq!(cur.block_id.as_deref(), Some("b-1"));
        assert_eq!(cur.offset, None);
        assert_eq!(cur.selection_end, None);
    }

    #[test]
    fn topic_constants_share_prefix() {
        for t in [PRESENCE_TOPIC, PEER_JOINED_TOPIC, PEER_LEFT_TOPIC] {
            assert!(t.starts_with(COLLAB_TOPIC_PREFIX), "{t} must share prefix");
        }
    }
}
