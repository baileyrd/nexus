//! End-to-end CollabClient ⇄ relay ⇄ CollabClient tests.
//!
//! Each test boots a real RelayServer on `127.0.0.1:0`, connects two
//! CollabClients with distinct `EventBus`es (modelling two Runtimes on
//! two machines), and exercises the bridge: outbound events from one
//! bus appear on the other bus via the relay, with site-based
//! self-echo dropped.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nexus_collab::{
    CollabClient, CollabClientConfig, ConnectParams, RelayServer, Token, EDITOR_PLUGIN_ID,
    OPS_TOPIC_PREFIX,
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
    config: CollabClientConfig,
) -> CollabClient {
    let params = ConnectParams {
        host: "127.0.0.1".into(),
        port: addr.port(),
        url: format!("ws://127.0.0.1:{}/", addr.port()),
        token: token.to_string(),
        peer_id: peer_id.to_string(),
        display_name: display_name.to_string(),
    };
    CollabClient::connect(params, bus, config).await.expect("connect")
}

/// Wait for the next custom event matching the given topic on the bus.
async fn recv_topic(bus: &EventBus, topic: &str) -> serde_json::Value {
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
    .expect("recv_topic timed out")
}

// ----------------------------------------------------------------------------

#[tokio::test]
async fn handshake_succeeds_and_peer_id_echoed() {
    let addr = start_relay("t").await;
    let bus = Arc::new(EventBus::new(64));
    let client = connect_client(addr, bus, "t", "alice", "Alice", CollabClientConfig::default()).await;
    assert_eq!(client.peer_id(), "alice");
    assert!(client.initial_peers().is_empty());
}

#[tokio::test]
async fn handshake_initial_peers_lists_existing_connections() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, bus_a, "t", "alice", "Alice", CollabClientConfig::default()).await;
    let bus_b = Arc::new(EventBus::new(64));
    let b = connect_client(addr, bus_b, "t", "bob", "Bob", CollabClientConfig::default()).await;
    assert_eq!(b.initial_peers().len(), 1);
    assert_eq!(b.initial_peers()[0].peer_id, "alice");
    assert_eq!(b.initial_peers()[0].display_name, "Alice");
}

#[tokio::test]
async fn outbound_editor_op_reaches_other_peer_bus() {
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice", CollabClientConfig::default()).await;
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob", CollabClientConfig::default()).await;

    // Subscribe on B's bus before A publishes.
    let mut sub_b = bus_b.subscribe(EventFilter::CustomExact(
        "com.nexus.editor.ops.notes/today.md".into(),
    ));

    // Brief tickle so the relay-side outbound subscription is live
    // before we publish — the bus is a broadcast channel and a
    // pre-subscription publish would be lost.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = json!({"op": {"id": {"site": "site-A", "lamport": 1}}});
    bus_a
        .publish_plugin(
            EDITOR_PLUGIN_ID,
            "com.nexus.editor.ops.notes/today.md",
            payload.clone(),
        )
        .expect("publish on A");

    let event = tokio::time::timeout(TEST_TIMEOUT, sub_b.recv())
        .await
        .expect("recv")
        .expect("non-error");
    let received = match &event.event {
        NexusEvent::Custom { payload, .. } => payload.clone(),
        other => panic!("expected Custom event, got {other:?}"),
    };
    assert_eq!(received, payload);
}

#[tokio::test]
async fn self_echo_by_site_id_is_dropped_on_inbound() {
    // B is configured with local_site_id = "site-A"; an op authored by
    // site-A that lands on B's bus via the relay must NOT be
    // republished by B's bridge.
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice", CollabClientConfig::default()).await;
    let cfg_b = CollabClientConfig {
        local_site_id: Some("site-A".into()),
        ..CollabClientConfig::default()
    };
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob", cfg_b).await;

    let mut sub_b = bus_b.subscribe(EventFilter::CustomExact(
        "com.nexus.editor.ops.notes/today.md".into(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = json!({"op": {"id": {"site": "site-A", "lamport": 1}}});
    bus_a
        .publish_plugin(EDITOR_PLUGIN_ID, "com.nexus.editor.ops.notes/today.md", payload)
        .expect("publish on A");

    let res = tokio::time::timeout(Duration::from_millis(300), sub_b.recv()).await;
    assert!(res.is_err(), "self-echo should not appear on B's bus");
}

#[tokio::test]
async fn op_from_other_site_is_published_on_inbound() {
    // Conversely, an op authored by a *different* site than B's local
    // site_id should pass through and land on B's bus.
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice", CollabClientConfig::default()).await;
    let cfg_b = CollabClientConfig {
        local_site_id: Some("site-B".into()),
        ..CollabClientConfig::default()
    };
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob", cfg_b).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = json!({"op": {"id": {"site": "site-A", "lamport": 1}}});
    bus_a
        .publish_plugin(EDITOR_PLUGIN_ID, "com.nexus.editor.ops.notes/today.md", payload.clone())
        .expect("publish on A");

    let received = recv_topic(&bus_b, "com.nexus.editor.ops.notes/today.md").await;
    assert_eq!(received, payload);
}

#[tokio::test]
async fn outbound_filter_scope_is_respected() {
    // A non-ops event on A's bus should NOT cross the relay because
    // the outbound filter pins to OPS_TOPIC_PREFIX.
    let addr = start_relay("t").await;
    let bus_a = Arc::new(EventBus::new(64));
    let bus_b = Arc::new(EventBus::new(64));
    let _a = connect_client(addr, Arc::clone(&bus_a), "t", "alice", "Alice", CollabClientConfig::default()).await;
    let _b = connect_client(addr, Arc::clone(&bus_b), "t", "bob", "Bob", CollabClientConfig::default()).await;

    let mut sub_b = bus_b.subscribe(EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.into()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Publish under the editor's namespace but NOT on the ops topic —
    // it's `com.nexus.editor.changed.*` shaped, distinct from `.ops.`.
    bus_a
        .publish_plugin(
            EDITOR_PLUGIN_ID,
            "com.nexus.editor.changed.notes/today.md",
            json!({"saved": true}),
        )
        .expect("publish on A");

    let res = tokio::time::timeout(Duration::from_millis(300), sub_b.recv()).await;
    assert!(
        res.is_err(),
        "non-ops topic should not be bridged to B (filter pinned to ops prefix)"
    );
}

#[tokio::test]
async fn shutdown_cleanly_tears_down_tasks() {
    let addr = start_relay("t").await;
    let bus = Arc::new(EventBus::new(64));
    let client =
        connect_client(addr, Arc::clone(&bus), "t", "alice", "Alice", CollabClientConfig::default()).await;
    client.shutdown().await;
    // Re-publishing on the bus after shutdown is a no-op for the
    // bridge; no panic or hang implies shutdown completed.
    let _ = bus.publish_plugin(EDITOR_PLUGIN_ID, "com.nexus.editor.ops.x", json!(null));
}
