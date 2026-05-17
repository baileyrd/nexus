//! Wire protocol for the BL-143 collab relay.
//!
//! One JSON message per WebSocket text frame. The relay is intentionally
//! topic-agnostic — it ferries opaque `payload` JSON tagged with a
//! kernel-bus `topic` between peers, leaving CRDT op merge semantics
//! (`com.nexus.editor.ops.<relpath>`) and presence semantics
//! (`com.nexus.collab.presence`) to the consumers. That keeps this
//! crate stable when the consumers grow new topics.
//!
//! ## Handshake
//!
//! 1. Client connects, sends [`ClientMessage::Hello`] as the first frame.
//! 2. Server validates the token. On mismatch it sends an
//!    [`ServerMessage::Error`] with code [`ERR_AUTH`] and closes the
//!    socket. On success it replies with [`ServerMessage::Hello`]
//!    carrying the currently-connected peers, and broadcasts
//!    [`ServerMessage::PeerJoined`] to the other peers.
//! 3. Either side may now send [`ClientMessage::Envelope`] /
//!    [`ServerMessage::Envelope`] frames carrying topic-tagged payloads.
//! 4. When a peer disconnects the server broadcasts
//!    [`ServerMessage::PeerLeft`].
//!
//! ## Echo policy
//!
//! The relay never echoes a peer's own envelope back to itself. That's
//! the relay's only routing rule; per-op self-echo de-duplication (by
//! [`nexus_crdt::SiteId`]) still happens downstream, but cutting the
//! obvious loop here keeps the wire chatter halved.

use serde::{Deserialize, Serialize};

/// Error code returned in [`ServerMessage::Error`] when the client's
/// `Hello.token` does not match the relay's configured token.
pub const ERR_AUTH: &str = "auth";

/// Error code for "the first frame was not a `Hello`". The server
/// rejects everything else until handshake completes.
pub const ERR_HANDSHAKE: &str = "handshake";

/// Error code for malformed wire frames (non-JSON, unexpected variant,
/// missing required fields). The server closes the socket after sending
/// one of these.
pub const ERR_BAD_FRAME: &str = "bad_frame";

/// Lightweight peer descriptor shared in `Hello` / `PeerJoined`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Server-stable peer identifier (the client's choice; the relay
    /// only checks uniqueness within a connection lifetime).
    pub peer_id: String,
    /// Human-readable name to show in the peers panel.
    pub display_name: String,
}

/// Frames the client may send.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Must be the first frame on every connection.
    Hello {
        /// Shared secret. Compared constant-time against the relay's
        /// configured token; empty tokens are rejected at server
        /// construction so the wire can rely on a non-empty value here.
        token: String,
        /// Caller-chosen id; collisions on a live relay are rejected
        /// with [`ERR_HANDSHAKE`] so two peers can't masquerade as one.
        peer_id: String,
        /// Human-readable name.
        display_name: String,
    },
    /// Topic-tagged payload to broadcast to other peers.
    Envelope {
        /// Kernel-bus topic the payload should be re-published under on
        /// each receiving peer (e.g. `com.nexus.editor.ops.notes/today.md`).
        topic: String,
        /// Opaque payload. The relay does not inspect it.
        payload: serde_json::Value,
    },
}

/// Frames the server may send.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Successful handshake reply. Includes a snapshot of peers
    /// already connected so the new arrival can seed its peers panel
    /// without waiting for the next broadcast.
    Hello {
        /// Echo of the client's chosen `peer_id` (lets the client
        /// confirm the relay accepted it).
        peer_id: String,
        /// Peers already connected, excluding the new arrival.
        peers: Vec<PeerInfo>,
    },
    /// Envelope forwarded from another peer.
    Envelope {
        /// `peer_id` of the originator.
        from: String,
        /// Kernel-bus topic for re-publication.
        topic: String,
        /// Opaque payload.
        payload: serde_json::Value,
    },
    /// A new peer joined the relay.
    PeerJoined {
        /// Descriptor for the new peer.
        peer: PeerInfo,
    },
    /// A peer disconnected.
    PeerLeft {
        /// `peer_id` of the departing peer.
        peer_id: String,
    },
    /// Protocol-level error from the server. After sending one of
    /// these the server may also close the connection — see the
    /// per-code policy in module docs.
    Error {
        /// One of the `ERR_*` constants in this module.
        code: String,
        /// Human-readable detail.
        message: String,
    },
}

impl ServerMessage {
    /// Build an [`ServerMessage::Error`] with the given code +
    /// `Into<String>` message.
    pub(crate) fn error(code: &str, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_hello_round_trip() {
        let m = ClientMessage::Hello {
            token: "t".into(),
            peer_id: "p1".into(),
            display_name: "Alice".into(),
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["kind"], "hello");
        assert_eq!(json["token"], "t");
        let back: ClientMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn client_envelope_round_trip() {
        let m = ClientMessage::Envelope {
            topic: "com.nexus.editor.ops.notes/today.md".into(),
            payload: serde_json::json!({"op": {"id": 1}}),
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["kind"], "envelope");
        let back: ClientMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn server_envelope_carries_from() {
        let m = ServerMessage::Envelope {
            from: "p1".into(),
            topic: "x".into(),
            payload: serde_json::json!(null),
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["kind"], "envelope");
        assert_eq!(json["from"], "p1");
    }

    #[test]
    fn server_error_constructor_sets_code_and_message() {
        let m = ServerMessage::error(ERR_AUTH, "bad token");
        match m {
            ServerMessage::Error { code, message } => {
                assert_eq!(code, ERR_AUTH);
                assert_eq!(message, "bad token");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn client_message_rejects_unknown_kind() {
        let raw = serde_json::json!({"kind": "frobnicate"});
        let res: Result<ClientMessage, _> = serde_json::from_value(raw);
        assert!(res.is_err());
    }
}
