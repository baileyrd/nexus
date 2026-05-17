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
//! Cursor coordinates are kept abstract (`relpath` + optional
//! `block_id`) on purpose: BL-143 Phase 2 wires the CM6 caret offset
//! into a richer cursor type, but the relay should not need to evolve
//! for that — Phase 2 lands an additive enum branch / optional field
//! here and the wire payload stays compatible via `#[serde(default)]`.

use serde::{Deserialize, Serialize};

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
/// Phase 1.3 keeps this abstract — just a relpath plus an optional
/// block id (using the BL-117 block-id convention). Phase 2 will
/// extend with character-level offsets and selection range without
/// breaking this struct (the new fields ride alongside as
/// `#[serde(default)]`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceCursor {
    /// Forge-relative path of the file the peer is focused on.
    pub relpath: String,
    /// Optional block id (matches the ADR 0017 stamp). `None` means
    /// the peer is on the file but not inside any block (e.g.
    /// scrolling, navigating).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

/// Peer-authored presence frame. Wire payload of [`PRESENCE_TOPIC`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    }

    #[test]
    fn topic_constants_share_prefix() {
        for t in [PRESENCE_TOPIC, PEER_JOINED_TOPIC, PEER_LEFT_TOPIC] {
            assert!(t.starts_with(COLLAB_TOPIC_PREFIX), "{t} must share prefix");
        }
    }
}
