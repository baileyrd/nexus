//! BL-143 Phase 1.5 — end-to-end reconnect tests.
//!
//! Each test spawns a `ReconnectingClient` against a relay that gets
//! restarted under the supervisor; the test asserts that:
//! * buffered events from the outage window arrive once the relay is
//!   back, and
//! * the `com.nexus.collab.connection` topic surfaces
//!   `connecting` → `connected` → `disconnected` → `connecting` →
//!   `connected` transitions.
//!
//! Relays bind on a specific port (rather than `:0`) so the supervisor
//! can reconnect to the same address after the first relay is dropped.
//! `TcpListener::bind` reuses the port without a `TIME_WAIT` delay on
//! Linux for short-lived TCP sockets we generate here.

use std::sync::Arc;
use std::time::Duration;

use nexus_collab::{
    ConnectParams, ReconnectConfig, ReconnectingClient, RelayServer, Token, COLLAB_PLUGIN_ID,
    CONNECTION_STATE_TOPIC, OPS_TOPIC_PREFIX, PRESENCE_TOPIC,
};
use nexus_kernel::{EventBus, EventFilter, NexusEvent};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-test relay handle: keeps the `Arc<RelayServer>` alive so we
/// can call `shutdown()` and lets the test `await` the accept loop
/// task. Dropping the handle aborts the loop as a safety net.
struct RelayHandle {
    server: Arc<RelayServer>,
    accept_task: Option<JoinHandle<()>>,
}

impl RelayHandle {
    /// Signal shutdown and wait for `serve_listener` to drain (which
    /// includes aborting per-peer tasks). Returns once the listener
    /// is gone and the port is free to rebind.
    async fn stop(mut self) {
        self.server.shutdown();
        if let Some(task) = self.accept_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
        }
    }
}

/// Pick a free ephemeral port by binding `:0`, recording the port,
/// and dropping the listener. Subsequent rebinds on that port
/// succeed because Linux releases ephemeral ports promptly when the
/// listener never accepted a connection.
async fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

async fn start_relay_on(port: u16, token: &str) -> RelayHandle {
    let server = Arc::new(RelayServer::new(Token::new(token).unwrap()));
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let server_for_task = Arc::clone(&server);
    let accept_task = tokio::spawn(async move {
        let _ = server_for_task.serve_listener(listener).await;
    });
    RelayHandle {
        server,
        accept_task: Some(accept_task),
    }
}

fn params_for(port: u16) -> ConnectParams {
    ConnectParams {
        host: "127.0.0.1".into(),
        port,
        url: format!("ws://127.0.0.1:{port}/"),
        token: "t".into(),
        peer_id: "alice".into(),
        display_name: "Alice".into(),
    }
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn supervisor_emits_connecting_then_connected_on_initial_handshake() {
    let port = reserve_port().await;
    let _relay = start_relay_on(port, "t").await;
    let bus = Arc::new(EventBus::new(64));
    let mut state_sub = bus.subscribe(EventFilter::CustomExact(CONNECTION_STATE_TOPIC.to_string()));
    let _client = ReconnectingClient::start(
        params_for(port),
        Arc::clone(&bus),
        vec![EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string())],
        None,
        ReconnectConfig {
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(500),
            backoff_factor: 2.0,
            buffer_capacity: 32,
        },
    );

    // First state event is `connecting`, immediately followed by
    // `connected` once the relay accepts the handshake.
    let mut seen_connecting = false;
    let mut seen_connected = false;
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, state_sub.recv()).await {
            Ok(Ok(event)) => {
                if let NexusEvent::Custom { payload, .. } = &event.event {
                    match payload["state"].as_str().unwrap_or("") {
                        "connecting" => seen_connecting = true,
                        "connected" => {
                            seen_connected = true;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            _ => break,
        }
    }
    assert!(seen_connecting, "expected `connecting` state event");
    assert!(seen_connected, "expected `connected` state event");
}

/// Drain a pre-existing connection-state subscription until the
/// supervisor reports `target`. Callers must subscribe *before* the
/// supervisor that produces the event starts; otherwise the
/// transition can fire on an empty broadcast channel and be lost.
async fn wait_for_state(sub: &mut nexus_kernel::EventSubscription, target: &str) {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let event = sub.recv().await.expect("recv state");
            if let NexusEvent::Custom { payload, .. } = &event.event {
                if payload["state"] == target {
                    return;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("did not observe state={target} within timeout"));
}

#[tokio::test]
async fn reconnect_replays_buffered_events_after_relay_outage() {
    // Boot a relay on port P; connect alice (reconnecting) and bob
    // (also reconnecting). Wait for both connected. Call
    // `RelayServer::shutdown` — this closes alice's and bob's
    // sockets, so each supervisor sees `disconnected`. Publish a
    // presence event on alice's bus during the outage; the
    // supervisor's feeder buffers it because no session is
    // draining. Bring a fresh relay up on the same port. Both
    // supervisors reconnect (per their backoff); alice's buffered
    // event flushes to the relay, the relay broadcasts to bob, bob
    // republishes on his bus. Assert the marker shows up exactly
    // once on bob's `PRESENCE_TOPIC` subscription.
    let port = reserve_port().await;
    let relay1 = start_relay_on(port, "t").await;

    let bus_alice = Arc::new(EventBus::new(256));
    let bus_bob = Arc::new(EventBus::new(256));

    // Subscribe to connection-state BEFORE starting the supervisor
    // so we never miss a transition; broadcast events with no
    // subscribers are dropped.
    let mut alice_state =
        bus_alice.subscribe(EventFilter::CustomExact(CONNECTION_STATE_TOPIC.to_string()));
    let mut bob_state =
        bus_bob.subscribe(EventFilter::CustomExact(CONNECTION_STATE_TOPIC.to_string()));

    let reconnect_cfg = ReconnectConfig {
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(300),
        backoff_factor: 2.0,
        buffer_capacity: 32,
    };
    let _alice = ReconnectingClient::start(
        params_for(port),
        Arc::clone(&bus_alice),
        vec![EventFilter::CustomExact(PRESENCE_TOPIC.to_string())],
        None,
        reconnect_cfg.clone(),
    );
    let mut bob_params = params_for(port);
    bob_params.peer_id = "bob".into();
    bob_params.display_name = "Bob".into();
    let _bob = ReconnectingClient::start(
        bob_params,
        Arc::clone(&bus_bob),
        vec![EventFilter::CustomExact(PRESENCE_TOPIC.to_string())],
        None,
        reconnect_cfg,
    );

    wait_for_state(&mut alice_state, "connected").await;
    wait_for_state(&mut bob_state, "connected").await;

    // Subscribe on bob's bus *before* the outage so the buffered
    // event isn't lost between republish and the test's recv.
    let mut sub_bob = bus_bob.subscribe(EventFilter::CustomExact(PRESENCE_TOPIC.to_string()));

    // Kill the relay — peer sockets close because `shutdown.await`
    // aborts every per-peer task in the JoinSet before
    // `serve_listener` returns.
    relay1.stop().await;

    wait_for_state(&mut alice_state, "disconnected").await;
    wait_for_state(&mut bob_state, "disconnected").await;

    // Author a presence event during the outage. Alice's feeder
    // buffers it; no session is draining.
    bus_alice
        .publish_plugin(
            COLLAB_PLUGIN_ID,
            PRESENCE_TOPIC,
            json!({
                "user_id": "alice",
                "display_name": "Alice",
                "marker": "during-outage",
            }),
        )
        .expect("publish during outage");

    // Bring the relay back. Same token + port, fresh state.
    let _relay2 = start_relay_on(port, "t").await;

    // Both supervisors will reconnect within a few backoff cycles.
    // The buffered event flushes from alice → relay → bob.
    let event = tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let event = sub_bob.recv().await.expect("recv on bob");
            if let NexusEvent::Custom { payload, .. } = &event.event {
                if payload["marker"] == "during-outage" {
                    return event;
                }
            }
        }
    })
    .await
    .expect("buffered event arrived after reconnect");
    if let NexusEvent::Custom { payload, .. } = &event.event {
        assert_eq!(payload["user_id"], "alice");
        assert_eq!(payload["marker"], "during-outage");
    }
}

#[tokio::test]
async fn handshake_failure_with_bad_token_retries_with_backoff() {
    let port = reserve_port().await;
    let _relay = start_relay_on(port, "correct").await;
    let bus = Arc::new(EventBus::new(64));
    let mut state_sub = bus.subscribe(EventFilter::CustomExact(CONNECTION_STATE_TOPIC.to_string()));
    let mut params = params_for(port);
    params.token = "wrong".into();
    let _client = ReconnectingClient::start(
        params,
        Arc::clone(&bus),
        vec![EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string())],
        None,
        ReconnectConfig {
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(200),
            backoff_factor: 2.0,
            buffer_capacity: 16,
        },
    );

    // With a wrong token we should see at least two
    // `connecting`/`disconnected` cycles within a second, without
    // ever transitioning to `connected`.
    let mut connecting = 0;
    let mut connected = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, state_sub.recv()).await {
            Ok(Ok(event)) => {
                if let NexusEvent::Custom { payload, .. } = &event.event {
                    match payload["state"].as_str().unwrap_or("") {
                        "connecting" => connecting += 1,
                        "connected" => connected += 1,
                        _ => {}
                    }
                }
            }
            _ => break,
        }
    }
    assert_eq!(connected, 0, "bad token must never reach connected state");
    assert!(
        connecting >= 2,
        "expected at least 2 retry attempts, got {connecting}"
    );
}
