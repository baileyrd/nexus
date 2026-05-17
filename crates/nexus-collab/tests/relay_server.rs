//! End-to-end relay-server tests.
//!
//! These spin up a real `RelayServer` on an ephemeral 127.0.0.1 port
//! and connect two (or three) WebSocket clients to exercise routing,
//! presence broadcasts, and handshake rejection paths. Each test
//! drives clients via [`tokio_tungstenite::client_async`] against a
//! raw `TcpStream::connect` to avoid pulling the `connect-*` /
//! TLS-enabled features of tokio-tungstenite into this crate's dep
//! graph.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nexus_collab::{
    ClientMessage, RelayServer, ServerMessage, Token, ERR_AUTH, ERR_HANDSHAKE,
};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// Per-test timeout — generous enough that a slow CI shard doesn't
/// flake but tight enough that a hang fails fast locally.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

type Ws = WebSocketStream<TcpStream>;

async fn start_server(token: &str) -> SocketAddr {
    let server = Arc::new(RelayServer::new(Token::new(token).unwrap()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.serve_listener(listener).await;
    });
    addr
}

async fn connect(addr: SocketAddr) -> Ws {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (ws, _) = tokio_tungstenite::client_async(format!("ws://{addr}/"), stream)
        .await
        .expect("ws handshake");
    ws
}

async fn send(ws: &mut Ws, msg: &ClientMessage) {
    let payload = serde_json::to_string(msg).unwrap();
    ws.send(Message::Text(payload.into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> ServerMessage {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            match ws.next().await.expect("stream closed") {
                Ok(Message::Text(t)) => return serde_json::from_str(t.as_ref()).expect("parse"),
                Ok(Message::Ping(_) | Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => panic!("connection closed unexpectedly"),
                Ok(Message::Binary(_) | Message::Frame(_)) => panic!("unexpected frame kind"),
                Err(e) => panic!("ws error: {e}"),
            }
        }
    })
    .await
    .expect("recv timed out")
}

async fn recv_close_or_error(ws: &mut Ws) -> Option<ServerMessage> {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    return Some(serde_json::from_str(t.as_ref()).expect("parse"))
                }
                Some(Ok(Message::Close(_))) | None => return None,
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Binary(_) | Message::Frame(_))) => continue,
                Some(Err(_)) => return None,
            }
        }
    })
    .await
    .expect("recv_close_or_error timed out")
}

async fn handshake(ws: &mut Ws, token: &str, peer_id: &str, name: &str) -> ServerMessage {
    send(
        ws,
        &ClientMessage::Hello {
            token: token.to_string(),
            peer_id: peer_id.to_string(),
            display_name: name.to_string(),
        },
    )
    .await;
    recv(ws).await
}

// ----------------------------------------------------------------------------

#[tokio::test]
async fn two_peers_handshake_and_see_each_other() {
    let addr = start_server("t").await;

    let mut a = connect(addr).await;
    let hello_a = handshake(&mut a, "t", "alice", "Alice").await;
    match hello_a {
        ServerMessage::Hello { peer_id, peers } => {
            assert_eq!(peer_id, "alice");
            assert!(peers.is_empty(), "first peer sees no peers, got {peers:?}");
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    let mut b = connect(addr).await;
    let hello_b = handshake(&mut b, "t", "bob", "Bob").await;
    match hello_b {
        ServerMessage::Hello { peer_id, peers } => {
            assert_eq!(peer_id, "bob");
            assert_eq!(peers.len(), 1);
            assert_eq!(peers[0].peer_id, "alice");
            assert_eq!(peers[0].display_name, "Alice");
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // Alice should now have received a PeerJoined for Bob.
    let joined = recv(&mut a).await;
    match joined {
        ServerMessage::PeerJoined { peer } => {
            assert_eq!(peer.peer_id, "bob");
            assert_eq!(peer.display_name, "Bob");
        }
        other => panic!("expected PeerJoined, got {other:?}"),
    }
}

#[tokio::test]
async fn envelope_is_broadcast_to_other_peer_only() {
    let addr = start_server("t").await;
    let mut a = connect(addr).await;
    handshake(&mut a, "t", "alice", "Alice").await;
    let mut b = connect(addr).await;
    handshake(&mut b, "t", "bob", "Bob").await;
    // Drain alice's PeerJoined notification.
    let _ = recv(&mut a).await;

    let payload = json!({"op": "edit", "n": 42});
    send(
        &mut a,
        &ClientMessage::Envelope {
            topic: "com.nexus.editor.ops.notes/today.md".into(),
            payload: payload.clone(),
        },
    )
    .await;

    let got = recv(&mut b).await;
    match got {
        ServerMessage::Envelope {
            from,
            topic,
            payload: got_payload,
        } => {
            assert_eq!(from, "alice");
            assert_eq!(topic, "com.nexus.editor.ops.notes/today.md");
            assert_eq!(got_payload, payload);
        }
        other => panic!("expected Envelope, got {other:?}"),
    }

    // Alice must not have received her own envelope. The cleanest
    // assertion is that the next frame on her socket (if any) is NOT
    // an Envelope — we trigger one by having Bob send something.
    send(
        &mut b,
        &ClientMessage::Envelope {
            topic: "x".into(),
            payload: json!("ping"),
        },
    )
    .await;
    let from_b = recv(&mut a).await;
    match from_b {
        ServerMessage::Envelope { from, .. } => assert_eq!(from, "bob"),
        other => panic!("expected Envelope from bob, got {other:?}"),
    }
}

#[tokio::test]
async fn disconnect_emits_peer_left() {
    let addr = start_server("t").await;
    let mut a = connect(addr).await;
    handshake(&mut a, "t", "alice", "Alice").await;
    let mut b = connect(addr).await;
    handshake(&mut b, "t", "bob", "Bob").await;
    let _ = recv(&mut a).await; // PeerJoined for bob

    drop(b);

    let left = recv(&mut a).await;
    match left {
        ServerMessage::PeerLeft { peer_id } => assert_eq!(peer_id, "bob"),
        other => panic!("expected PeerLeft, got {other:?}"),
    }
}

#[tokio::test]
async fn bad_token_rejected() {
    let addr = start_server("correct").await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        &ClientMessage::Hello {
            token: "wrong".into(),
            peer_id: "alice".into(),
            display_name: "Alice".into(),
        },
    )
    .await;
    let msg = recv_close_or_error(&mut ws).await;
    match msg {
        Some(ServerMessage::Error { code, .. }) => assert_eq!(code, ERR_AUTH),
        other => panic!("expected Error(auth), got {other:?}"),
    }
}

#[tokio::test]
async fn first_frame_must_be_hello() {
    let addr = start_server("t").await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        &ClientMessage::Envelope {
            topic: "x".into(),
            payload: json!(null),
        },
    )
    .await;
    let msg = recv_close_or_error(&mut ws).await;
    match msg {
        Some(ServerMessage::Error { code, .. }) => assert_eq!(code, ERR_HANDSHAKE),
        other => panic!("expected Error(handshake), got {other:?}"),
    }
}

#[tokio::test]
async fn duplicate_peer_id_rejected() {
    let addr = start_server("t").await;
    let mut a = connect(addr).await;
    handshake(&mut a, "t", "alice", "Alice").await;
    let mut a2 = connect(addr).await;
    send(
        &mut a2,
        &ClientMessage::Hello {
            token: "t".into(),
            peer_id: "alice".into(),
            display_name: "Alice2".into(),
        },
    )
    .await;
    let msg = recv_close_or_error(&mut a2).await;
    match msg {
        Some(ServerMessage::Error { code, message }) => {
            assert_eq!(code, ERR_HANDSHAKE);
            assert!(message.contains("alice"));
        }
        other => panic!("expected Error(handshake), got {other:?}"),
    }
}

#[tokio::test]
async fn empty_peer_id_rejected() {
    let addr = start_server("t").await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        &ClientMessage::Hello {
            token: "t".into(),
            peer_id: String::new(),
            display_name: "anon".into(),
        },
    )
    .await;
    let msg = recv_close_or_error(&mut ws).await;
    match msg {
        Some(ServerMessage::Error { code, .. }) => assert_eq!(code, ERR_HANDSHAKE),
        other => panic!("expected Error(handshake), got {other:?}"),
    }
}

#[tokio::test]
async fn three_peer_fanout_routes_envelope_to_both_others() {
    let addr = start_server("t").await;
    let mut a = connect(addr).await;
    handshake(&mut a, "t", "a", "A").await;
    let mut b = connect(addr).await;
    handshake(&mut b, "t", "b", "B").await;
    let _ = recv(&mut a).await; // PeerJoined: b
    let mut c = connect(addr).await;
    handshake(&mut c, "t", "c", "C").await;
    let _ = recv(&mut a).await; // PeerJoined: c
    let _ = recv(&mut b).await; // PeerJoined: c

    send(
        &mut a,
        &ClientMessage::Envelope {
            topic: "topic".into(),
            payload: json!("hi"),
        },
    )
    .await;
    for ws in [&mut b, &mut c] {
        match recv(ws).await {
            ServerMessage::Envelope { from, payload, .. } => {
                assert_eq!(from, "a");
                assert_eq!(payload, json!("hi"));
            }
            other => panic!("expected Envelope, got {other:?}"),
        }
    }
}
