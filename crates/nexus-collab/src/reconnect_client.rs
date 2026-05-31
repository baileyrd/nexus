//! BL-143 Phase 1.5 — auto-reconnecting collab bridge.
//!
//! `ReconnectingClient` is a self-supervising counterpart to
//! [`crate::CollabClient`]: it opens a relay connection, runs the bus
//! bridge inside, and on disconnect (or initial-connect failure)
//! sleeps for an exponential-backoff delay before reconnecting.
//!
//! Outbound events published on the local kernel bus while the relay
//! is unreachable are not lost. The supervisor owns a bounded
//! `VecDeque` filled by a per-filter feeder task; when a session is
//! live, the session task drains the deque to the WebSocket. The
//! buffer caps at [`ReconnectConfig::buffer_capacity`] and drops the
//! oldest entry on overflow, so a long outage can't grow unbounded.
//!
//! Connection lifecycle events are surfaced on
//! [`CONNECTION_STATE_TOPIC`] so the shell (BL-143 Phase 2) can
//! render a "reconnecting" badge.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nexus_kernel::{EventBus, EventFilter, NexusEvent, RecvError};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::client::{bridge_republish, drop_for_self_echo, ConnectParams, COLLAB_BRIDGE_PLUGIN_ID};
use crate::protocol::{ClientMessage, ServerMessage};

/// Bus topic that carries [`ConnectionState`] payloads. The
/// supervisor emits one event per state transition; subscribers can
/// render a connection-status indicator without polling.
pub const CONNECTION_STATE_TOPIC: &str = "com.nexus.collab.connection";

/// Lifecycle of the relay connection, broadcast on
/// [`CONNECTION_STATE_TOPIC`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    /// Initial state; supervisor just started.
    Connecting,
    /// Handshake completed; bridge running.
    Connected,
    /// Disconnected (initial-connect failed or live session ended).
    /// Backoff sleep is about to start.
    Disconnected,
}

impl ConnectionState {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Tunable knobs for [`ReconnectingClient::start`].
#[derive(Clone, Debug)]
pub struct ReconnectConfig {
    /// Sleep before the first reconnect attempt after a session
    /// ends. Defaults to 1 s.
    pub initial_delay: Duration,
    /// Cap on the backoff sleep. Defaults to 30 s.
    pub max_delay: Duration,
    /// Multiplier applied each failure. Defaults to 2.0.
    pub backoff_factor: f32,
    /// Bounded outbound queue size, in number of envelopes. When the
    /// queue is full and the supervisor pushes another envelope, the
    /// oldest entry is dropped. Defaults to 256.
    pub buffer_capacity: usize,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
            buffer_capacity: 256,
        }
    }
}

fn next_backoff(current: Duration, cfg: &ReconnectConfig) -> Duration {
    let scaled = current.mul_f32(cfg.backoff_factor);
    if scaled > cfg.max_delay {
        cfg.max_delay
    } else {
        scaled
    }
}

/// Public handle to the supervisor task. Drop the value (or call
/// [`Self::shutdown`]) to stop reconnecting.
pub struct ReconnectingClient {
    shutdown: Option<oneshot::Sender<()>>,
    supervisor: Option<JoinHandle<()>>,
}

impl ReconnectingClient {
    /// Spawn the supervisor task. Returns immediately; the
    /// supervisor connects asynchronously and retries forever (until
    /// `shutdown`).
    #[must_use]
    pub fn start(
        params: ConnectParams,
        bus: Arc<EventBus>,
        outbound_filters: Vec<EventFilter>,
        local_site_id: Option<String>,
        reconnect: ReconnectConfig,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let supervisor = tokio::spawn(supervise(
            params,
            bus,
            outbound_filters,
            local_site_id,
            reconnect,
            shutdown_rx,
        ));
        Self {
            shutdown: Some(shutdown_tx),
            supervisor: Some(supervisor),
        }
    }

    /// Stop the supervisor and wait briefly for it to drain.
    pub async fn shutdown(mut self) {
        self.shutdown_inner().await;
    }

    async fn shutdown_inner(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.supervisor.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }
    }
}

impl Drop for ReconnectingClient {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.supervisor.take() {
            handle.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Internal pieces
// ---------------------------------------------------------------------------

/// Outbound envelope ready for forwarding to the relay. Topic +
/// payload only; the supervisor stamps these into
/// [`ClientMessage::Envelope`] just before write.
type OutboundItem = (String, Value);

/// Bounded queue shared between the feeder task and the per-session
/// drain. The `Notify` half wakes the drain when new items arrive
/// while a session is live.
struct OutboundBuffer {
    queue: AsyncMutex<VecDeque<OutboundItem>>,
    capacity: usize,
    notify: Notify,
}

impl OutboundBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            queue: AsyncMutex::new(VecDeque::with_capacity(capacity.min(1024))),
            capacity: capacity.max(1),
            notify: Notify::new(),
        }
    }

    async fn push(&self, item: OutboundItem) {
        let mut q = self.queue.lock().await;
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(item);
        drop(q);
        self.notify.notify_one();
    }

    /// Pop the next item, waiting via `Notify` if the queue is
    /// currently empty.
    async fn pop_wait(&self) -> OutboundItem {
        loop {
            {
                let mut q = self.queue.lock().await;
                if let Some(item) = q.pop_front() {
                    return item;
                }
            }
            // Wait for a producer notify. The producer always calls
            // notify_one *after* push, so if we miss a notify because
            // the queue happened to be empty during our previous
            // lock, the next push wakes us.
            self.notify.notified().await;
        }
    }
}

async fn supervise(
    params: ConnectParams,
    bus: Arc<EventBus>,
    outbound_filters: Vec<EventFilter>,
    local_site_id: Option<String>,
    cfg: ReconnectConfig,
    mut shutdown: oneshot::Receiver<()>,
) {
    let buffer = Arc::new(OutboundBuffer::new(cfg.buffer_capacity));
    // Spawn one feeder per outbound filter. All push into the shared
    // buffer. Feeders run for the supervisor's whole lifetime.
    let feeder_tasks: Vec<_> = outbound_filters
        .iter()
        .cloned()
        .map(|filter| {
            let bus_clone = Arc::clone(&bus);
            let buf = Arc::clone(&buffer);
            tokio::spawn(run_feeder(bus_clone, filter, buf))
        })
        .collect();

    publish_state(&bus, ConnectionState::Connecting);

    let mut backoff = cfg.initial_delay;
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            outcome = run_one_session(
                &params,
                &bus,
                local_site_id.as_deref(),
                Arc::clone(&buffer),
            ) => {
                match outcome {
                    SessionOutcome::HandshakeFailed => {
                        // Cap is mostly relevant to live-session
                        // failures; for initial-connect we still want
                        // exponential delay so a flapping relay
                        // doesn't get hammered.
                    }
                    SessionOutcome::SessionEnded => {
                        // The session ran. Reset backoff so a
                        // long-lived session that finally disconnects
                        // doesn't inherit a stale max-delay.
                        backoff = cfg.initial_delay;
                    }
                }
                publish_state(&bus, ConnectionState::Disconnected);
            }
        }
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            () = tokio::time::sleep(backoff) => {}
        }
        publish_state(&bus, ConnectionState::Connecting);
        backoff = next_backoff(backoff, &cfg);
    }

    for f in feeder_tasks {
        f.abort();
    }
}

/// Outcome of [`run_one_session`]. Drives the supervisor's backoff
/// reset policy.
enum SessionOutcome {
    /// Handshake or initial connect failed; never reached the
    /// drain/inbound loops.
    HandshakeFailed,
    /// Session ran (handshake completed) and later ended.
    SessionEnded,
}

async fn run_one_session(
    params: &ConnectParams,
    bus: &Arc<EventBus>,
    local_site_id: Option<&str>,
    buffer: Arc<OutboundBuffer>,
) -> SessionOutcome {
    // Connect + handshake.
    let stream = match TcpStream::connect((params.host.as_str(), params.port)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "nexus-collab/reconnect: tcp connect failed");
            return SessionOutcome::HandshakeFailed;
        }
    };
    let mut ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    ws_config.max_message_size = Some(16 * 1024 * 1024);
    ws_config.max_frame_size = Some(16 * 1024 * 1024);
    let (ws, _) =
        match tokio_tungstenite::client_async_with_config(&params.url, stream, Some(ws_config))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "nexus-collab/reconnect: ws handshake failed");
                return SessionOutcome::HandshakeFailed;
            }
        };
    let (mut sink, mut stream) = ws.split();

    // Send Hello.
    let hello = ClientMessage::Hello {
        token: params.token.clone(),
        peer_id: params.peer_id.clone(),
        display_name: params.display_name.clone(),
    };
    let payload = serde_json::to_string(&hello).expect("Hello serialises");
    if sink.send(Message::Text(payload.into())).await.is_err() {
        return SessionOutcome::HandshakeFailed;
    }

    // Await Hello reply (no per-attempt timeout — the supervisor's
    // outer cycle bounds total wait time via backoff sleeps).
    let reply = loop {
        match stream.next().await {
            Some(Ok(Message::Text(t))) => match serde_json::from_str::<ServerMessage>(t.as_ref()) {
                Ok(m) => break m,
                Err(e) => {
                    tracing::debug!(error = %e, "nexus-collab/reconnect: bad hello reply");
                    return SessionOutcome::HandshakeFailed;
                }
            },
            Some(Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_),
            )) => {}
            None | Some(Ok(Message::Close(_)) | Err(_)) => return SessionOutcome::HandshakeFailed,
        }
    };
    match reply {
        ServerMessage::Hello { .. } => {}
        ServerMessage::Error { code, message } => {
            tracing::warn!(%code, %message, "nexus-collab/reconnect: relay rejected handshake");
            return SessionOutcome::HandshakeFailed;
        }
        other => {
            tracing::warn!(reply = ?other, "nexus-collab/reconnect: unexpected handshake reply");
            return SessionOutcome::HandshakeFailed;
        }
    }
    publish_state(bus, ConnectionState::Connected);

    // Wrap the sink for the drain task.
    let sink = Arc::new(AsyncMutex::new(sink));
    // Spawn the outbound drain. It pops from `buffer` and writes to
    // `sink` until either an I/O error occurs or it's aborted.
    let drain_handle = {
        let buf = Arc::clone(&buffer);
        let sink = Arc::clone(&sink);
        tokio::spawn(async move {
            run_drain(buf, sink).await;
        })
    };

    // Inbound loop runs inline. On any close / error, return so the
    // supervisor's outer loop sleeps + reconnects.
    let local_site_id_owned = local_site_id.map(str::to_string);
    let inbound_outcome = run_inbound(stream, Arc::clone(bus), local_site_id_owned).await;
    drain_handle.abort();
    tracing::debug!(?inbound_outcome, "nexus-collab/reconnect: session ended");

    SessionOutcome::SessionEnded
}

async fn run_feeder(bus: Arc<EventBus>, filter: EventFilter, buffer: Arc<OutboundBuffer>) {
    let mut sub = bus.subscribe(filter);
    loop {
        match sub.recv().await {
            Ok(event) => {
                if let NexusEvent::Custom {
                    type_id,
                    emitting_plugin,
                    payload,
                } = &event.event
                {
                    if emitting_plugin == COLLAB_BRIDGE_PLUGIN_ID {
                        continue;
                    }
                    buffer.push((type_id.clone(), payload.clone())).await;
                }
            }
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(lost = n, "nexus-collab/reconnect: feeder lagged");
            }
            Err(RecvError::Closed) => {
                tracing::debug!("nexus-collab/reconnect: feeder bus closed");
                return;
            }
        }
    }
}

async fn run_drain(
    buffer: Arc<OutboundBuffer>,
    sink: Arc<AsyncMutex<futures_util::stream::SplitSink<WebSocketStream<TcpStream>, Message>>>,
) {
    loop {
        let (topic, payload) = buffer.pop_wait().await;
        // Build the wire body separately from the buffer item so the
        // failure branch can push the original tuple back without
        // re-parsing. Cloning `payload` is the price of replay
        // durability — bounded by message size, which the relay's
        // 16 MiB cap already constrains.
        let frame = ClientMessage::Envelope {
            topic: topic.clone(),
            payload: payload.clone(),
        };
        let body = match serde_json::to_string(&frame) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "nexus-collab/reconnect: drain serialise failed");
                continue;
            }
        };
        let mut s = sink.lock().await;
        if s.send(Message::Text(body.into())).await.is_err() {
            // Connection dropped — push the original tuple back so
            // the next session re-sends it. The supervisor's outer
            // loop aborts this task shortly; this push is best-effort
            // durability for the in-flight item.
            drop(s);
            buffer.push((topic, payload)).await;
            return;
        }
    }
}

/// Outcome reported back by [`run_inbound`].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InboundOutcome {
    /// The peer (or local close call) ended the connection cleanly.
    PeerClosed,
    /// A wire-level error tore the connection down.
    Error,
}

async fn run_inbound(
    mut stream: futures_util::stream::SplitStream<WebSocketStream<TcpStream>>,
    bus: Arc<EventBus>,
    local_site_id: Option<String>,
) -> InboundOutcome {
    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(Message::Text(t)) => t,
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_)) => {
                continue
            }
            Ok(Message::Close(_))
            | Err(
                tokio_tungstenite::tungstenite::Error::ConnectionClosed
                | tokio_tungstenite::tungstenite::Error::AlreadyClosed,
            ) => return InboundOutcome::PeerClosed,
            Err(e) => {
                tracing::debug!(error = %e, "nexus-collab/reconnect: inbound read error");
                return InboundOutcome::Error;
            }
        };
        let msg: ServerMessage = match serde_json::from_str(text.as_ref()) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(error = %e, "nexus-collab/reconnect: bad inbound frame");
                continue;
            }
        };
        match msg {
            ServerMessage::Envelope { topic, payload, .. } => {
                if drop_for_self_echo(&payload, local_site_id.as_deref()) {
                    continue;
                }
                bridge_republish(&bus, &topic, payload);
            }
            ServerMessage::PeerJoined { peer } => {
                bridge_republish(
                    &bus,
                    crate::presence::PEER_JOINED_TOPIC,
                    serde_json::to_value(&peer).unwrap_or(Value::Null),
                );
            }
            ServerMessage::PeerLeft { peer_id } => {
                bridge_republish(
                    &bus,
                    crate::presence::PEER_LEFT_TOPIC,
                    json!({ "peer_id": peer_id }),
                );
            }
            ServerMessage::Hello { .. } => {
                tracing::trace!("nexus-collab/reconnect: ignoring mid-stream Hello");
            }
            ServerMessage::Error { code, message } => {
                tracing::warn!(%code, %message, "nexus-collab/reconnect: relay error frame");
            }
        }
    }
    InboundOutcome::PeerClosed
}

fn publish_state(bus: &EventBus, state: ConnectionState) {
    let payload = json!({ "state": state.as_wire() });
    let event = NexusEvent::Custom {
        type_id: CONNECTION_STATE_TOPIC.to_string(),
        emitting_plugin: COLLAB_BRIDGE_PLUGIN_ID.to_string(),
        payload,
    };
    if let Err(e) = bus.publish_core(COLLAB_BRIDGE_PLUGIN_ID, event) {
        tracing::warn!(error = %e, "nexus-collab/reconnect: publish connection-state failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_backoff_doubles_until_max() {
        let cfg = ReconnectConfig {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            backoff_factor: 2.0,
            buffer_capacity: 16,
        };
        // mul_f32 introduces sub-millisecond float noise; compare to
        // the integer doubling within a 1ms tolerance.
        let approx = |d: Duration, target_ms: u128| {
            let diff = d.as_millis().abs_diff(target_ms);
            assert!(diff <= 1, "{d:?} vs {target_ms}ms (diff {diff})");
        };
        let b1 = next_backoff(Duration::from_millis(100), &cfg);
        let b2 = next_backoff(b1, &cfg);
        let b3 = next_backoff(b2, &cfg);
        let b4 = next_backoff(b3, &cfg);
        approx(b1, 200);
        approx(b2, 400);
        approx(b3, 800);
        assert_eq!(b4, Duration::from_secs(1), "capped at max_delay");
    }

    #[tokio::test]
    async fn outbound_buffer_drops_oldest_on_overflow() {
        let buf = OutboundBuffer::new(2);
        buf.push(("t".into(), json!(1))).await;
        buf.push(("t".into(), json!(2))).await;
        buf.push(("t".into(), json!(3))).await;
        let first = buf.pop_wait().await;
        let second = buf.pop_wait().await;
        assert_eq!(first.1, json!(2), "oldest dropped, 2 is the new front");
        assert_eq!(second.1, json!(3));
    }

    #[tokio::test]
    async fn outbound_buffer_pop_wait_unblocks_on_push() {
        let buf = Arc::new(OutboundBuffer::new(4));
        let buf_clone = Arc::clone(&buf);
        let producer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            buf_clone.push(("t".into(), json!("hi"))).await;
        });
        let got = tokio::time::timeout(Duration::from_secs(1), buf.pop_wait())
            .await
            .expect("pop_wait timed out");
        assert_eq!(got.1, json!("hi"));
        producer.await.unwrap();
    }

    #[test]
    fn connection_state_wire_codes_are_stable() {
        assert_eq!(ConnectionState::Connecting.as_wire(), "connecting");
        assert_eq!(ConnectionState::Connected.as_wire(), "connected");
        assert_eq!(ConnectionState::Disconnected.as_wire(), "disconnected");
    }
}
