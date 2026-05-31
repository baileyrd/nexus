//! End-to-end tests for BL-143 Phase 1.3 presence bridging.
//!
//! Boots a real relay + two `CollabClient`s and exercises:
//! - presence events round-trip on the `com.nexus.collab.presence` topic
//! - `PeerJoined` from the relay surfaces on each connected peer's bus
//!   as `com.nexus.collab.peers.joined`
//! - `PeerLeft` surfaces on `com.nexus.collab.peers.left`
//! - bridge-republished events do NOT loop back through the outbound
//!   subscription (the emitting-plugin filter at the outbound side
//!   exercises this — see `outbound_skips_bridge_authored_events`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nexus_collab::{
    CollabClient, CollabClientConfig, ConnectParams, PresenceCursor, PresenceEvent, RelayServer,
    Token, COLLAB_BRIDGE_PLUGIN_ID, COLLAB_PLUGIN_ID, PEER_JOINED_TOPIC, PEER_LEFT_TOPIC,
    PRESENCE_TOPIC,
};
use nexus_kernel::{EventBus, EventFilter, NexusEvent};
use serde_json::json;
use tokio::net::TcpListener;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

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
) -> CollabClient {
    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port: addr.port(),
        url: format!("ws://127.0.0.1:{}/", addr.port()),
        token: token.to_string(),
        peer_id: peer_id.to_string(),
        display_name: display_name.to_string(),
    };
    CollabClient::connect(params, bus, CollabClientConfig::default())
        .await
        .expect("connect")
}

async fn recv_topic(bus: &EventBus, topic: &str) -> serde_json::Value {
    let mut sub = bus.subscribe(EventFilter::CustomExact(topic.to_string()));
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let event = sub.recv().await.expect("recv");
            if let NexusEvent::Custom {
                type_id, payload, ..
            } = &event.event
            {
                if type_id == topic {
                    return payload.clone();
                }
            }
        }
    })
    .await
    .expect("recv_topic timed out")
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn presence_event_round_trips_between_peers() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice").await;
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob").await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ev = PresenceEvent {
        user_id: "alice".into(),
        display_name: "Alice".into(),
        cursor: Some(PresenceCursor {
            relpath: "notes/today.md".into(),
            block_id: Some("b-7".into()),
            offset: Some(42),
            selection_end: None,
        }),
    };
    let payload = serde_json::to_value(&ev).unwrap();
    bus_a
        .publish_plugin(COLLAB_PLUGIN_ID, PRESENCE_TOPIC, payload.clone())
        .expect("publish presence");

    let received = recv_topic(&bus_b, PRESENCE_TOPIC).await;
    assert_eq!(received, payload);
    let decoded: PresenceEvent = serde_json::from_value(received).unwrap();
    assert_eq!(decoded, ev);
}

#[tokio::test]
async fn peer_joined_fires_on_existing_peers_bus() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice").await;

    // Pre-subscribe to peer-joined on alice's bus before bob connects
    // so we don't race the broadcast.
    let mut sub = bus_a.subscribe(EventFilter::CustomExact(PEER_JOINED_TOPIC.to_string()));

    let bus_b = Arc::new(EventBus::new(64));
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob").await;

    let event = tokio::time::timeout(TEST_TIMEOUT, sub.recv())
        .await
        .expect("peer-joined arrived")
        .expect("non-error");
    match &event.event {
        NexusEvent::Custom {
            type_id,
            emitting_plugin,
            payload,
        } => {
            assert_eq!(type_id, PEER_JOINED_TOPIC);
            assert_eq!(
                emitting_plugin, COLLAB_BRIDGE_PLUGIN_ID,
                "bridge-authored events must use the bridge plugin id so outbound can skip them"
            );
            assert_eq!(payload["peer_id"], "bob");
            assert_eq!(payload["display_name"], "Bob");
        }
        other => panic!("expected Custom, got {other:?}"),
    }
}

#[tokio::test]
async fn peer_left_fires_on_remaining_peers_bus() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice").await;
    let bus_b = Arc::new(EventBus::new(64));
    let b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob").await;
    // Drain alice's peer-joined for bob so the next custom event is
    // the peer-left we're testing for.
    let _ = recv_topic(&bus_a, PEER_JOINED_TOPIC).await;

    let mut sub = bus_a.subscribe(EventFilter::CustomExact(PEER_LEFT_TOPIC.to_string()));
    drop(b);

    let event = tokio::time::timeout(TEST_TIMEOUT, sub.recv())
        .await
        .expect("peer-left arrived")
        .expect("non-error");
    if let NexusEvent::Custom {
        type_id, payload, ..
    } = &event.event
    {
        assert_eq!(type_id, PEER_LEFT_TOPIC);
        assert_eq!(payload["peer_id"], "bob");
    } else {
        panic!("expected Custom");
    }
}

#[tokio::test]
async fn outbound_skips_bridge_authored_events_no_loop() {
    // The "anti-loop" invariant: when inbound republishes a presence
    // (or peer) event on the bus, the outbound subscription must NOT
    // re-ship it to the relay. If this regressed the relay would see
    // the event twice — once from the original author and once from
    // every receiver-bridge — and broadcast it back, producing a
    // proper traffic loop.
    //
    // Instrumenting the relay directly is awkward; instead we use a
    // simple proxy: connect three peers and have peer A publish a
    // presence event. Peer B and peer C each see it exactly once.
    // Without the emitting_plugin skip, each receiver's outbound
    // would forward it back to the relay, the relay would broadcast
    // to the other two, the receivers would republish, etc.
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let bus_c = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "a", "A").await;
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "b", "B").await;
    let _c = connect_client(addr, Arc::clone(&bus_c), "t", "c", "C").await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut sub_b = bus_b.subscribe(EventFilter::CustomExact(PRESENCE_TOPIC.to_string()));
    let mut sub_c = bus_c.subscribe(EventFilter::CustomExact(PRESENCE_TOPIC.to_string()));

    bus_a
        .publish_plugin(
            COLLAB_PLUGIN_ID,
            PRESENCE_TOPIC,
            json!({"user_id": "a", "display_name": "A"}),
        )
        .expect("publish");

    // Each should arrive exactly once and then nothing more.
    let _first_b = tokio::time::timeout(TEST_TIMEOUT, sub_b.recv())
        .await
        .unwrap();
    let _first_c = tokio::time::timeout(TEST_TIMEOUT, sub_c.recv())
        .await
        .unwrap();

    let extra_b = tokio::time::timeout(Duration::from_millis(200), sub_b.recv()).await;
    let extra_c = tokio::time::timeout(Duration::from_millis(200), sub_c.recv()).await;
    assert!(
        extra_b.is_err(),
        "B received a second presence event — loop detected"
    );
    assert!(
        extra_c.is_err(),
        "C received a second presence event — loop detected"
    );
}

#[tokio::test]
async fn presence_event_without_cursor_round_trips() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice").await;
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob").await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ev = PresenceEvent {
        user_id: "alice".into(),
        display_name: "Alice".into(),
        cursor: None,
    };
    let payload = serde_json::to_value(&ev).unwrap();
    bus_a
        .publish_plugin(COLLAB_PLUGIN_ID, PRESENCE_TOPIC, payload.clone())
        .expect("publish presence");

    let received = recv_topic(&bus_b, PRESENCE_TOPIC).await;
    let decoded: PresenceEvent = serde_json::from_value(received).unwrap();
    assert_eq!(decoded, ev);
}
