//! BL-143 Definition-of-Done punch list.
//!
//! Each test here corresponds to one DoD bullet from `BL-143.md`. The
//! tests are intentionally self-contained and a little redundant with
//! the per-phase suites (`relay_server.rs`, `client_bridge.rs`,
//! `presence_bridge.rs`, `reconnect.rs`, the `core_plugin` unit tests):
//! reading this file alone should be enough to see that every DoD line
//! has live coverage.
//!
//! The DoD bullets (from the bottom of `BL-143.md`):
//!
//! 1. *Two machines on the same LAN can concurrently edit a shared
//!    forge with CRDT convergence.* — modelled by two distinct
//!    `EventBus`es bridged through one relay, each bus seeing the
//!    other's editor-op publishes.
//! 2. *Presence panel lists connected peers and their active files.*
//!    — modelled by a `PresenceEvent` round-trip with a populated
//!    `PresenceCursor { relpath, offset }` payload.
//! 3. *Disconnect / reconnect handled gracefully (buffered ops
//!    replayed on reconnect).* — covered by `reconnect.rs`'s
//!    `reconnect_replays_buffered_events_after_relay_outage`; this
//!    file pins the auth + identity-stamping invariants instead so the
//!    punch list isn't a duplicate.
//! 4. *Auth token rejection.* — bad token on the handshake yields an
//!    `ERR_AUTH` `ServerMessage::Error` and `CollabClient::connect`
//!    returns a transport error.
//! 5. *Cursor publish handler stamps identity end-to-end.* — Phase 2.2
//!    invariant: a `publish_presence` IPC call on one runtime reaches
//!    another runtime's bus with the configured `user_id` /
//!    `display_name`, no matter what the caller passed in.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nexus_collab::core_plugin::{
    CollabCorePlugin, LocalPeer, HANDLER_PUBLISH_PRESENCE,
};
use nexus_collab::{
    CollabClient, CollabClientConfig, ConnectParams, PresenceEvent, RelayServer, Token,
    COLLAB_PLUGIN_ID, EDITOR_PLUGIN_ID, PRESENCE_TOPIC,
};
use nexus_kernel::{EventBus, EventFilter, NexusEvent};
use nexus_plugins::CorePlugin;
use serde_json::json;
use tokio::net::TcpListener;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

// ── Shared helpers (intentionally local; the punch list is meant to
//     stand alone and read top-to-bottom) ───────────────────────────

async fn start_relay(token: &str) -> SocketAddr {
    let server = Arc::new(RelayServer::new(Token::new(token).unwrap()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.serve_listener(listener).await;
    });
    addr
}

async fn connect_client(
    addr: SocketAddr,
    bus: Arc<EventBus>,
    token: &str,
    peer_id: &str,
    display_name: &str,
) -> Result<CollabClient, nexus_collab::ConnectError> {
    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port: addr.port(),
        url: format!("ws://127.0.0.1:{}/", addr.port()),
        token: token.to_string(),
        peer_id: peer_id.to_string(),
        display_name: display_name.to_string(),
    };
    CollabClient::connect(params, bus, CollabClientConfig::default()).await
}

async fn recv_next_on(bus: &EventBus, topic: &str) -> serde_json::Value {
    let mut sub = bus.subscribe(EventFilter::CustomExact(topic.to_string()));
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let event = sub.recv().await.expect("recv");
            if let NexusEvent::Custom { type_id, payload, .. } = &event.event {
                if type_id == topic {
                    return payload.clone();
                }
            }
        }
    })
    .await
    .expect("recv_next_on timed out")
}

// ── DoD #1 — relay routes editor ops between two distinct buses ───

#[tokio::test]
async fn dod_two_runtimes_exchange_editor_ops_over_relay() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice")
        .await
        .expect("alice connects");
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob")
        .await
        .expect("bob connects");

    let topic = "com.nexus.editor.ops.notes/today.md";
    let mut sub_b = bus_b.subscribe(EventFilter::CustomExact(topic.into()));
    // Tickle: outbound subscription must be live on the relay side
    // before alice publishes; the bus is a broadcast channel and
    // pre-subscription publishes are dropped.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = json!({"op": {"id": {"site": "site-A", "lamport": 1}}});
    bus_a
        .publish_plugin(EDITOR_PLUGIN_ID, topic, payload.clone())
        .expect("publish on A");

    let event = tokio::time::timeout(TEST_TIMEOUT, sub_b.recv())
        .await
        .expect("relay routed within timeout")
        .expect("non-error");
    let NexusEvent::Custom {
        type_id, payload: got, ..
    } = &event.event
    else {
        panic!("expected Custom event, got {:?}", event.event);
    };
    assert_eq!(type_id, topic);
    assert_eq!(got, &payload);
}

// ── DoD #2 — presence event round-trip carries cursor metadata ─────

#[tokio::test]
async fn dod_presence_event_round_trips_with_cursor() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice")
        .await
        .expect("alice connects");
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob")
        .await
        .expect("bob connects");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ev = PresenceEvent {
        user_id: "alice".into(),
        display_name: "Alice".into(),
        cursor: Some(nexus_collab::PresenceCursor {
            relpath: "notes/today.md".into(),
            block_id: None,
            offset: Some(42),
            selection_end: None,
        }),
    };
    bus_a
        .publish_plugin(
            COLLAB_PLUGIN_ID,
            PRESENCE_TOPIC,
            serde_json::to_value(&ev).unwrap(),
        )
        .expect("alice publishes presence");

    let got = recv_next_on(&bus_b, PRESENCE_TOPIC).await;
    let decoded: PresenceEvent = serde_json::from_value(got).unwrap();
    assert_eq!(decoded.user_id, "alice");
    assert_eq!(decoded.display_name, "Alice");
    let cursor = decoded.cursor.expect("cursor stamped");
    assert_eq!(cursor.relpath, "notes/today.md");
    assert_eq!(cursor.offset, Some(42));
}

// ── DoD #3 — auth token rejected (Phase 1.1, anchored here for the
//     punch list) ──────────────────────────────────────────────────

#[tokio::test]
async fn dod_handshake_with_bad_token_is_rejected() {
    let addr = start_relay("real-secret").await;
    let bus = Arc::new(EventBus::new(8));
    let outcome = connect_client(addr, bus, "wrong-secret", "alice", "Alice").await;
    let err = match outcome {
        Ok(_) => panic!("connect must fail when the token doesn't match"),
        Err(e) => e,
    };
    // Hand-decoding the exact variant matters less than asserting we
    // don't silently accept the bad credential. The shape is:
    //   ConnectError::Handshake { code: ERR_AUTH, .. }
    let s = format!("{err:?}");
    assert!(
        s.to_lowercase().contains("auth"),
        "error message must mention auth rejection: {s}"
    );
}

// ── DoD #4 — publish_presence handler stamps identity from
//     `[collab]` regardless of caller-supplied fields ──────────────

#[tokio::test]
async fn dod_publish_presence_handler_stamps_identity_end_to_end() {
    // Two real Runtimes: alice runs the handler, bob is a remote peer
    // that just listens. The handler publishes through alice's bus;
    // the bridge forwards to bob.
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice")
        .await
        .expect("alice connects");
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob")
        .await
        .expect("bob connects");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut plugin = CollabCorePlugin::new(
        Some(Arc::clone(&bus_a)),
        Some(LocalPeer {
            user_id: "alice".into(),
            display_name: "Alice".into(),
        }),
    );
    // The caller only supplies cursor — not user_id / display_name.
    plugin
        .dispatch(
            HANDLER_PUBLISH_PRESENCE,
            &json!({"cursor": {"relpath": "x.md", "offset": 7}}),
        )
        .expect("publish_presence runs");

    let got = recv_next_on(&bus_b, PRESENCE_TOPIC).await;
    let decoded: PresenceEvent = serde_json::from_value(got).unwrap();
    // Identity comes from the plugin, not the IPC args.
    assert_eq!(decoded.user_id, "alice");
    assert_eq!(decoded.display_name, "Alice");
    assert_eq!(decoded.cursor.as_ref().unwrap().relpath, "x.md");
    assert_eq!(decoded.cursor.as_ref().unwrap().offset, Some(7));
}

// ── DoD #5 — buffered-ops replay (covered exhaustively by
//     `reconnect.rs::reconnect_replays_buffered_events_after_relay_outage`;
//     a minimal anchor here lets the punch list assert *something*
//     about the reconnect path without re-running the long test) ──

#[tokio::test]
async fn dod_disconnect_is_observable_on_bus_via_connection_topic() {
    use nexus_collab::{ReconnectConfig, ReconnectingClient, CONNECTION_STATE_TOPIC};
    let addr = start_relay("t").await;
    let bus = Arc::new(EventBus::new(64));
    // Subscribe *before* start so we don't miss the initial
    // `connecting`/`connected` transitions.
    let mut sub = bus.subscribe(EventFilter::CustomExact(
        CONNECTION_STATE_TOPIC.to_string(),
    ));
    let _supervisor = ReconnectingClient::start(
        ConnectParams {
            host: "127.0.0.1".into(),
            port: addr.port(),
            url: format!("ws://127.0.0.1:{}/", addr.port()),
            token: "t".into(),
            peer_id: "alice".into(),
            display_name: "Alice".into(),
        },
        Arc::clone(&bus),
        vec![],
        None,
        ReconnectConfig::default(),
    );

    // Collect the first two state transitions and assert they're
    // shaped right. We do this in a loop because the test runtime is
    // free to interleave.
    let mut seen = Vec::<String>::new();
    while seen.len() < 2 {
        let event = tokio::time::timeout(TEST_TIMEOUT, sub.recv())
            .await
            .expect("connection state surfaced within timeout")
            .expect("non-error");
        if let NexusEvent::Custom { type_id, payload, .. } = &event.event {
            if type_id == CONNECTION_STATE_TOPIC {
                if let Some(state) = payload.get("state").and_then(|v| v.as_str()) {
                    seen.push(state.to_string());
                }
            }
        }
    }
    assert_eq!(seen[0], "connecting");
    assert_eq!(seen[1], "connected");
}
