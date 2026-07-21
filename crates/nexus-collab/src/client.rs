//! Kernel-bus ⇄ relay client bridge for BL-143 Phase 1.2.
//!
//! A [`CollabClient`] opens one WebSocket connection to a relay,
//! completes the [`crate::protocol::ClientMessage::Hello`] handshake,
//! and runs two background tasks bridging the local kernel
//! [`nexus_kernel::EventBus`] to the wire:
//!
//! * **Outbound** — subscribes to
//!   `EventFilter::CustomPrefix("com.nexus.editor.ops.")` and forwards
//!   every event as [`crate::protocol::ClientMessage::Envelope`] with
//!   the event's `type_id` as topic and the event payload verbatim.
//! * **Inbound** — receives [`crate::protocol::ServerMessage::Envelope`]
//!   frames and republishes them on the kernel bus via
//!   [`nexus_kernel::EventBus::publish_plugin`] under the editor's
//!   namespace (matching the [`nexus_crdt`] wire convention shipped by
//!   ADR 0026 Phase 3). Site-based self-echo dedup drops any inbound
//!   op whose `op.id.site` equals the configured `local_site_id`.
//!
//! The bridge is intentionally topic-agnostic at the protocol layer;
//! the editor-ops scoping lives in [`CollabClient::connect`]'s filter
//! choice so a future presence / settings bridge can spin up a separate
//! [`CollabClient`] (or extend this one with additional filters)
//! without changing the wire shape.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use nexus_kernel::{EventBus, EventFilter, NexusEvent, RecvError};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::presence::{COLLAB_TOPIC_PREFIX, PEER_JOINED_TOPIC, PEER_LEFT_TOPIC};
use crate::protocol::{ClientMessage, PeerInfo, ServerMessage};

/// Kernel-bus plugin id used when republishing inbound editor-ops
/// envelopes. Matches the editor's namespace so the kernel's
/// `type_id_in_namespace` check accepts the publish; the BL-143
/// trust model is identical to the existing in-bootstrap
/// `CrdtPublisher`, which also publishes on the editor's behalf.
pub const EDITOR_PLUGIN_ID: &str = "com.nexus.editor";

/// Kernel-bus plugin id used for collab-authored events (presence,
/// peer join/leave). Topics in this namespace originate at the
/// relay or the local presence publisher, not the editor.
pub const COLLAB_PLUGIN_ID: &str = "com.nexus.collab";

/// Kernel-bus plugin id stamped on every event the inbound bridge
/// republishes. Distinct from [`COLLAB_PLUGIN_ID`] so the outbound
/// subscriber can distinguish bridge-republished events (skip — they
/// came in over the wire) from locally-authored collab events (forward
/// to the relay). The kernel's `publish_core` path is used for these
/// publishes since it bypasses the namespace anti-spoof check, letting
/// the bridge stamp the editor's `com.nexus.editor.ops.*` topic with
/// this bridge-internal plugin id.
pub const COLLAB_BRIDGE_PLUGIN_ID: &str = "com.nexus.collab.bridge";

/// Prefix the outbound subscription matches. Mirrors
/// `nexus_crdt::wire::OPS_TOPIC_PREFIX`; duplicated here to avoid the
/// nexus-crdt dep (which transitively pulls nexus-editor).
pub const OPS_TOPIC_PREFIX: &str = "com.nexus.editor.ops.";

/// C60 / #413 — prefix matching `com.nexus.comments.*` mutation events
/// so comment thread changes reach collab peers live, the same as
/// editor ops and presence already do.
pub const COMMENTS_TOPIC_PREFIX: &str = "com.nexus.comments.";

/// Maximum WebSocket frame size accepted by client connections. Match
/// the server-side cap so a bad relay can't OOM the client.
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Default per-handshake timeout. Generous so a slow LAN relay doesn't
/// trip the timer; clients that want a tighter bound can change it via
/// [`CollabClientConfig::handshake_timeout`].
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Errors raised during [`CollabClient::connect`].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// TCP `connect` failed before the WS upgrade.
    #[error("tcp connect: {0}")]
    Tcp(#[from] std::io::Error),
    /// The WS handshake itself failed.
    #[error("ws handshake: {0}")]
    WsHandshake(tokio_tungstenite::tungstenite::Error),
    /// Sending the [`ClientMessage::Hello`] frame failed.
    #[error("hello send: {0}")]
    HelloSend(tokio_tungstenite::tungstenite::Error),
    /// The relay closed the socket before sending a reply.
    #[error("relay closed before hello reply")]
    EarlyClose,
    /// The first reply was not a [`ServerMessage::Hello`].
    #[error("expected hello reply, got: {0}")]
    UnexpectedReply(String),
    /// The relay returned a [`ServerMessage::Error`] frame during
    /// handshake.
    #[error("relay error: {code}: {message}")]
    RelayError {
        /// One of the `ERR_*` codes from [`crate::protocol`].
        code: String,
        /// Human-readable detail.
        message: String,
    },
    /// Reading the reply produced a wire-level error.
    #[error("ws read: {0}")]
    WsRead(tokio_tungstenite::tungstenite::Error),
    /// The reply could not be parsed as a [`ServerMessage`].
    #[error("decode reply: {0}")]
    Decode(serde_json::Error),
    /// Handshake exceeded the configured timeout.
    #[error("handshake timed out")]
    Timeout,
    /// The URL was not parseable.
    #[error("url: {0}")]
    Url(String),
}

/// Connection parameters for [`CollabClient::connect`]. Bundled into
/// one struct so the call site stays readable as more fields land
/// (cursor-color seed, presence cadence, etc.).
#[derive(Clone, Debug)]
pub struct ConnectParams {
    /// TCP host to dial.
    pub host: String,
    /// TCP port to dial.
    pub port: u16,
    /// `ws://…` URL announced in the WebSocket handshake's `Host` /
    /// `Origin` headers. The relay doesn't read it; the field exists
    /// so reverse proxies and load balancers can route by hostname.
    pub url: String,
    /// Shared secret matching the relay's configured token.
    pub token: String,
    /// Caller-chosen peer id; the relay rejects duplicates on a live
    /// connection.
    pub peer_id: String,
    /// Human-readable name for the peers panel.
    pub display_name: String,
}

/// Tunable knobs for [`CollabClient::connect`].
#[derive(Clone, Debug)]
pub struct CollabClientConfig {
    /// Per-handshake timeout. Defaults to [`DEFAULT_HANDSHAKE_TIMEOUT`].
    pub handshake_timeout: Duration,
    /// Subscription filters for the outbound bridge. One outbound
    /// task is spawned per filter and they share the WebSocket sink;
    /// each event matching any filter is forwarded as
    /// [`crate::protocol::ClientMessage::Envelope`].
    ///
    /// Defaults to two filters: `OPS_TOPIC_PREFIX` (CRDT op
    /// envelopes shipped by the editor's `CrdtPublisher`) and
    /// [`COLLAB_TOPIC_PREFIX`] (presence events; the relay-authored
    /// peer-join/leave bus topics also share this prefix but are
    /// authored by the *receiving* client's bridge — see
    /// [`run_inbound`] — so a peer publishing them locally is a
    /// degenerate case the relay's echo suppression already drops).
    pub outbound_filters: Vec<EventFilter>,
    /// `op.id.site` value to drop inbound envelopes from. `None`
    /// disables site-based dedup (useful in unit tests where the
    /// bridge runs without a local CRDT site identity).
    pub local_site_id: Option<String>,
}

impl Default for CollabClientConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            outbound_filters: vec![
                EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string()),
                EventFilter::CustomPrefix(COLLAB_TOPIC_PREFIX.to_string()),
            ],
            local_site_id: None,
        }
    }
}

/// Live connection to a relay.
///
/// Owns the outbound + inbound bridge tasks. Drop it (or call
/// [`CollabClient::shutdown`] explicitly) to tear the connection down.
/// Errors that occur while the bridge is running are logged via the
/// `tracing` crate; production callers that need observability beyond
/// that should subscribe to the connection-lost event added under BL-143
/// Phase 1.5 (reconnect resilience).
pub struct CollabClient {
    own_peer_id: String,
    initial_peers: Vec<PeerInfo>,
    reader_task: Option<JoinHandle<()>>,
    writer_tasks: Vec<JoinHandle<()>>,
    /// Broadcast sender; sending `()` once signals every outbound
    /// task to exit. Dropped from `shutdown` / `Drop`.
    shutdown: Option<broadcast::Sender<()>>,
    /// Shared WebSocket sink. `shutdown_inner` calls `close` on it
    /// once outbound tasks have released their guards, so the relay
    /// sees a proper WS close frame.
    sink: WsSinkMutex,
}

impl CollabClient {
    /// Connect to the relay described by `params`, complete the
    /// handshake, and spawn the bus bridge tasks.
    ///
    /// # Errors
    /// See [`ConnectError`] variants for the failure modes.
    ///
    /// # Panics
    /// Panics if [`serde_json::to_string`] fails on the canonical
    /// [`ClientMessage::Hello`] payload — unreachable in practice
    /// because every field is a plain `String` and serde-json can
    /// serialise those without I/O.
    pub async fn connect(
        params: ConnectParams,
        bus: Arc<EventBus>,
        config: CollabClientConfig,
    ) -> Result<Self, ConnectError> {
        let ConnectParams {
            host,
            port,
            url,
            token,
            peer_id,
            display_name,
        } = params;
        let stream = TcpStream::connect((host.as_str(), port)).await?;
        let mut ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        ws_config.max_message_size = Some(MAX_FRAME_BYTES);
        ws_config.max_frame_size = Some(MAX_FRAME_BYTES);
        let (ws, _) = tokio_tungstenite::client_async_with_config(&url, stream, Some(ws_config))
            .await
            .map_err(ConnectError::WsHandshake)?;
        let (mut sink, mut stream) = ws.split();

        // Send Hello.
        let hello = ClientMessage::Hello {
            token,
            peer_id,
            display_name,
        };
        let payload = serde_json::to_string(&hello).expect("ClientMessage::Hello serialises");
        sink.send(Message::Text(payload.into()))
            .await
            .map_err(ConnectError::HelloSend)?;

        // Await Hello reply (with timeout).
        let reply_fut = async {
            loop {
                let frame = match stream.next().await {
                    Some(Ok(f)) => f,
                    Some(Err(e)) => return Err(ConnectError::WsRead(e)),
                    None => return Err(ConnectError::EarlyClose),
                };
                match frame {
                    Message::Text(t) => {
                        return serde_json::from_str::<ServerMessage>(t.as_ref())
                            .map_err(ConnectError::Decode);
                    }
                    Message::Close(_) => return Err(ConnectError::EarlyClose),
                    Message::Ping(_)
                    | Message::Pong(_)
                    | Message::Binary(_)
                    | Message::Frame(_) => {}
                }
            }
        };
        let reply = tokio::time::timeout(config.handshake_timeout, reply_fut)
            .await
            .map_err(|_| ConnectError::Timeout)??;

        let (own_peer_id, initial_peers) = match reply {
            ServerMessage::Hello { peer_id, peers } => (peer_id, peers),
            ServerMessage::Error { code, message } => {
                return Err(ConnectError::RelayError { code, message });
            }
            other => {
                return Err(ConnectError::UnexpectedReply(format!("{other:?}")));
            }
        };

        // Spawn bridge tasks.
        let sink = Arc::new(AsyncMutex::new(sink));
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        let writer_tasks: Vec<JoinHandle<()>> = config
            .outbound_filters
            .into_iter()
            .map(|filter| {
                let bus_clone = Arc::clone(&bus);
                let sink_clone = Arc::clone(&sink);
                let shutdown_rx = shutdown_tx.subscribe();
                tokio::spawn(run_outbound(bus_clone, filter, sink_clone, shutdown_rx))
            })
            .collect();
        let reader_task = tokio::spawn(run_inbound(
            stream,
            bus,
            config.local_site_id,
            Arc::clone(&sink),
        ));

        Ok(Self {
            own_peer_id,
            initial_peers,
            reader_task: Some(reader_task),
            writer_tasks,
            shutdown: Some(shutdown_tx),
            sink,
        })
    }

    /// This client's accepted `peer_id` (echoed by the relay in its
    /// [`ServerMessage::Hello`]).
    #[must_use]
    pub fn peer_id(&self) -> &str {
        &self.own_peer_id
    }

    /// Snapshot of peers already connected when the handshake completed.
    /// Subsequent joins arrive as [`ServerMessage::PeerJoined`] events
    /// the bridge does NOT yet surface to callers — Phase 2 adds a
    /// peers-store consumer.
    #[must_use]
    pub fn initial_peers(&self) -> &[PeerInfo] {
        &self.initial_peers
    }

    /// Cleanly shut the bridge down. Closes the WebSocket, aborts the
    /// reader task, and signals the writer task to exit.
    pub async fn shutdown(mut self) {
        self.shutdown_inner().await;
    }

    async fn shutdown_inner(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        for handle in self.writer_tasks.drain(..) {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }
        // Send a clean WS close frame now that no outbound task is
        // contending for the sink lock. The reader task wakes on the
        // peer's close-frame echo and exits naturally; we still
        // `abort()` below as a belt-and-braces guard.
        {
            let mut s = self.sink.lock().await;
            let _ = s.close().await;
        }
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
    }
}

impl Drop for CollabClient {
    fn drop(&mut self) {
        // Drop signals shutdown but does not await — the tasks observe
        // either the broadcast or the socket close + exit on their own
        // schedule.
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
        for handle in self.writer_tasks.drain(..) {
            handle.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge tasks
// ---------------------------------------------------------------------------

type WsSinkMutex = Arc<AsyncMutex<SplitSink<WebSocketStream<TcpStream>, Message>>>;
type WsStream = SplitStream<WebSocketStream<TcpStream>>;

async fn run_outbound(
    bus: Arc<EventBus>,
    filter: EventFilter,
    sink: WsSinkMutex,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut sub = bus.subscribe(filter);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.recv() => {
                tracing::debug!("nexus-collab: outbound shutdown");
                return;
            }
            recv = sub.recv() => {
                match recv {
                    Ok(event) => {
                        let (topic, payload) = match &event.event {
                            NexusEvent::Custom { type_id, emitting_plugin, payload } => {
                                // Anti-loop: skip events the inbound
                                // bridge republished. Inbound uses
                                // `publish_core(COLLAB_PLUGIN_ID, …)`
                                // and stamps `emitting_plugin =
                                // COLLAB_PLUGIN_ID` for every payload
                                // it forwards back onto the bus, so
                                // this single check breaks the cycle
                                // for ops, presence, and peer events
                                // uniformly without per-topic special
                                // casing.
                                if emitting_plugin == COLLAB_BRIDGE_PLUGIN_ID {
                                    continue;
                                }
                                (type_id.clone(), payload.clone())
                            }
                            // The subscription filter pins us to Custom
                            // topics; non-Custom variants should never
                            // reach here. Defensive ignore in case the
                            // filter API ever broadens.
                            _ => continue,
                        };
                        let frame = ClientMessage::Envelope { topic, payload };
                        let body = match serde_json::to_string(&frame) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!(error = %e, "nexus-collab: outbound serialise failed");
                                continue;
                            }
                        };
                        let mut s = sink.lock().await;
                        if let Err(e) = s.send(Message::Text(body.into())).await {
                            tracing::debug!(error = %e, "nexus-collab: outbound send failed, exiting");
                            return;
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "nexus-collab: outbound subscription lagged");
                    }
                    Err(RecvError::Closed) => {
                        tracing::debug!("nexus-collab: outbound bus closed");
                        return;
                    }
                }
            }
        }
    }
}

async fn run_inbound(
    mut stream: WsStream,
    bus: Arc<EventBus>,
    local_site_id: Option<String>,
    sink: WsSinkMutex,
) {
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
            ) => break,
            Err(e) => {
                tracing::debug!(error = %e, "nexus-collab: inbound read error, exiting");
                break;
            }
        };
        let msg: ServerMessage = match serde_json::from_str(text.as_ref()) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(error = %e, "nexus-collab: dropping malformed inbound frame");
                continue;
            }
        };
        match msg {
            ServerMessage::Envelope { topic, payload, .. } => {
                if drop_for_self_echo(&payload, local_site_id.as_deref()) {
                    tracing::trace!(topic = %topic, "nexus-collab: dropping self-echoed op");
                    continue;
                }
                bridge_republish(&bus, &topic, payload);
            }
            ServerMessage::PeerJoined { peer } => {
                bridge_republish(&bus, PEER_JOINED_TOPIC, peer_info_payload(&peer));
            }
            ServerMessage::PeerLeft { peer_id } => {
                bridge_republish(
                    &bus,
                    PEER_LEFT_TOPIC,
                    serde_json::json!({ "peer_id": peer_id }),
                );
            }
            ServerMessage::Hello { .. } => {
                // A second Hello on a live connection is a protocol
                // bug — the relay only sends one in response to the
                // client's own Hello. Trace and ignore.
                tracing::trace!("nexus-collab: unexpected mid-stream Hello, ignoring");
            }
            ServerMessage::Error { code, message } => {
                tracing::warn!(%code, %message, "nexus-collab: relay error frame");
            }
        }
    }
    // Best-effort close on exit so the writer task can observe the
    // socket teardown via send-failure.
    let mut s = sink.lock().await;
    let _ = s.close().await;
}

/// Inspect an [`OpEnvelope`]-shaped payload (`{"op":{"id":{"site":..}}}`)
/// and return `true` if it originated at `local_site_id`.
///
/// Returns `false` whenever the payload doesn't fit the expected shape
/// — non-CRDT topics (presence, settings) flow through untouched.
pub(crate) fn drop_for_self_echo(payload: &Value, local_site_id: Option<&str>) -> bool {
    let Some(local) = local_site_id else {
        return false;
    };
    let site = payload
        .get("op")
        .and_then(|o| o.get("id"))
        .and_then(|i| i.get("site"))
        .and_then(Value::as_str);
    matches!(site, Some(s) if s == local)
}

/// Encode a [`PeerInfo`] as the JSON object the [`PEER_JOINED_TOPIC`]
/// payload exposes. Inlined because `PeerInfo` already derives
/// `Serialize` and the field layout is exactly what we want.
fn peer_info_payload(peer: &PeerInfo) -> Value {
    serde_json::to_value(peer).unwrap_or(Value::Null)
}

/// Republish an inbound payload on the local bus, tagging the event's
/// `emitting_plugin` as [`COLLAB_PLUGIN_ID`]. This serves two roles:
///
/// * Bypasses the kernel's namespace anti-spoof check (the collab
///   bridge legitimately republishes events on the editor's topics —
///   `com.nexus.editor.ops.*` — and `publish_core` is the kernel's
///   trusted-core-plugin escape hatch).
/// * Lets the outbound subscriber detect "bridge-authored" events by
///   inspecting `emitting_plugin` and skip them, breaking the
///   relay-loop without per-topic special casing.
pub(crate) fn bridge_republish(bus: &EventBus, topic: &str, payload: Value) {
    let event = NexusEvent::Custom {
        type_id: topic.to_string(),
        emitting_plugin: COLLAB_BRIDGE_PLUGIN_ID.to_string(),
        payload,
    };
    if let Err(e) = bus.publish_core(COLLAB_BRIDGE_PLUGIN_ID, event) {
        tracing::warn!(error = %e, %topic, "nexus-collab: bridge republish failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drop_for_self_echo_returns_false_when_no_local_site() {
        let payload = json!({"op": {"id": {"site": "abc"}}});
        assert!(!drop_for_self_echo(&payload, None));
    }

    #[test]
    fn drop_for_self_echo_returns_true_on_site_match() {
        let payload = json!({"op": {"id": {"site": "abc"}}});
        assert!(drop_for_self_echo(&payload, Some("abc")));
    }

    #[test]
    fn drop_for_self_echo_returns_false_on_site_mismatch() {
        let payload = json!({"op": {"id": {"site": "abc"}}});
        assert!(!drop_for_self_echo(&payload, Some("xyz")));
    }

    #[test]
    fn drop_for_self_echo_returns_false_on_malformed_payload() {
        let payload = json!({"presence": {"cursor": 7}});
        assert!(!drop_for_self_echo(&payload, Some("abc")));
        let payload2 = json!(null);
        assert!(!drop_for_self_echo(&payload2, Some("abc")));
    }

    #[test]
    fn peer_info_payload_round_trips() {
        let peer = PeerInfo {
            peer_id: "alice".into(),
            display_name: "Alice".into(),
        };
        let v = peer_info_payload(&peer);
        assert_eq!(v["peer_id"], "alice");
        assert_eq!(v["display_name"], "Alice");
    }
}
