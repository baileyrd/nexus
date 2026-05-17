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
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::protocol::{ClientMessage, PeerInfo, ServerMessage};

/// Kernel-bus plugin id used when republishing inbound editor-ops
/// envelopes. Matches the editor's namespace so the kernel's
/// `type_id_in_namespace` check accepts the publish; the BL-143
/// trust model is identical to the existing in-bootstrap
/// `CrdtPublisher`, which also publishes on the editor's behalf.
pub const EDITOR_PLUGIN_ID: &str = "com.nexus.editor";

/// Prefix the outbound subscription matches. Mirrors
/// `nexus_crdt::wire::OPS_TOPIC_PREFIX`; duplicated here to avoid the
/// nexus-crdt dep (which transitively pulls nexus-editor).
pub const OPS_TOPIC_PREFIX: &str = "com.nexus.editor.ops.";

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
    /// Subscription filter for the outbound bridge. Defaults to
    /// `EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string())` —
    /// every editor-ops event flies to the relay. Tests narrow this
    /// to limit chatter.
    pub outbound_filter: EventFilter,
    /// `op.id.site` value to drop inbound envelopes from. `None`
    /// disables site-based dedup (useful in unit tests where the
    /// bridge runs without a local CRDT site identity).
    pub local_site_id: Option<String>,
}

impl Default for CollabClientConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            outbound_filter: EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string()),
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
    writer_task: Option<JoinHandle<()>>,
    shutdown: Option<oneshot::Sender<()>>,
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
        let mut ws_config =
            tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
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
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let writer_task = tokio::spawn(run_outbound(
            Arc::clone(&bus),
            config.outbound_filter,
            Arc::clone(&sink),
            shutdown_rx,
        ));
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
            writer_task: Some(writer_task),
            shutdown: Some(shutdown_tx),
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
        if let Some(handle) = self.writer_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
    }
}

impl Drop for CollabClient {
    fn drop(&mut self) {
        // Drop signals shutdown but does not await — the tasks observe
        // either the oneshot or the socket close + exit on their own
        // schedule.
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
        if let Some(handle) = self.writer_task.take() {
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
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut sub = bus.subscribe(filter);
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                tracing::debug!("nexus-collab: outbound shutdown");
                let mut s = sink.lock().await;
                let _ = s.close().await;
                return;
            }
            recv = sub.recv() => {
                match recv {
                    Ok(event) => {
                        let (topic, payload) = match &event.event {
                            NexusEvent::Custom { type_id, payload, .. } => {
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
            Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_),
            ) => continue,
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
                let plugin_id = republish_plugin_id(&topic);
                if let Err(e) = bus.publish_plugin(plugin_id, &topic, payload) {
                    tracing::warn!(error = %e, topic = %topic, "nexus-collab: inbound republish failed");
                }
            }
            ServerMessage::PeerJoined { .. }
            | ServerMessage::PeerLeft { .. }
            | ServerMessage::Hello { .. } => {
                // Presence surfacing arrives under BL-143 Phase 1.3 (a
                // dedicated subscriber + `com.nexus.collab.presence`
                // bus topic). Until then, just trace.
                tracing::trace!(?msg, "nexus-collab: presence frame ignored");
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
fn drop_for_self_echo(payload: &Value, local_site_id: Option<&str>) -> bool {
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

/// Pick the kernel `source_plugin_id` to publish an inbound envelope
/// under. The kernel's namespace-anti-spoof check requires the topic
/// to live under `<source_plugin_id>.…`, so we route by topic prefix.
fn republish_plugin_id(topic: &str) -> &'static str {
    if topic.starts_with(OPS_TOPIC_PREFIX) {
        EDITOR_PLUGIN_ID
    } else {
        // Fall back to the editor namespace for now — Phase 1.3 adds
        // `com.nexus.collab.*` once presence ships, and the table
        // grows then.
        EDITOR_PLUGIN_ID
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
    fn republish_plugin_id_routes_ops_topics_to_editor() {
        assert_eq!(
            republish_plugin_id("com.nexus.editor.ops.notes/today.md"),
            EDITOR_PLUGIN_ID
        );
    }
}
