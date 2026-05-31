//! WebSocket relay server for BL-143 Phase 1.
//!
//! [`RelayServer::serve_listener`] owns a [`tokio::net::TcpListener`] and
//! accepts WebSocket connections from peers. Each peer connection runs
//! two tasks:
//!
//! * a **read task** that parses inbound frames, validates the
//!   handshake on the first frame, and forwards every subsequent
//!   [`ClientMessage::Envelope`] to a shared `broadcast::channel`;
//! * a **write task** that drains the broadcast channel and forwards
//!   matching frames out to the peer's socket. It skips frames that
//!   originated from the same peer so the relay never echoes a peer's
//!   own envelope back.
//!
//! When a peer disconnects, its tasks shut down and the read task
//! sends a [`InternalEvent::Leave`] before exiting so other peers see
//! a [`ServerMessage::PeerLeft`].
//!
//! The server is single-relay (one in-memory broadcast channel for the
//! whole process). Multi-channel / hosted relays are deferred.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::auth::Token;
use crate::protocol::{
    ClientMessage, PeerInfo, ServerMessage, ERR_AUTH, ERR_BAD_FRAME, ERR_HANDSHAKE,
};

/// Bound on the internal broadcast channel's lag tolerance. Bumping
/// this trades memory for tolerance to slow peers; 1024 entries is
/// enough to cover ~10 seconds of busy editing at 100 ops/s before a
/// laggy peer is force-dropped via `RecvError::Lagged`.
const BROADCAST_CAPACITY: usize = 1024;

/// Maximum WebSocket frame size accepted. 16 MiB matches
/// [`nexus_remote::transport::MAX_LINE_BYTES`] so a misbehaving peer
/// cannot OOM the relay.
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Errors the server may surface from its accept loop.
#[derive(Debug, thiserror::Error)]
pub enum RelayServerError {
    /// Accept on the listening socket failed irrecoverably.
    #[error("accept: {0}")]
    Accept(#[from] std::io::Error),
}

/// Internal broadcast envelope. Carries the originator's peer id so
/// each per-peer writer can suppress the echo without parsing the
/// payload.
#[derive(Clone, Debug)]
struct Routed {
    /// Originator peer id (or `None` for relay-authored frames such as
    /// `PeerJoined` / `PeerLeft`, which every peer must receive).
    from: Option<String>,
    /// Pre-serialised wire message ready to forward verbatim.
    payload: String,
}

/// Peer registry shared across connections. Used to seed the snapshot
/// list in [`ServerMessage::Hello`] and to enforce peer-id uniqueness.
#[derive(Default)]
struct PeerRegistry {
    peers: Vec<PeerInfo>,
}

impl PeerRegistry {
    fn snapshot_excluding(&self, peer_id: &str) -> Vec<PeerInfo> {
        self.peers
            .iter()
            .filter(|p| p.peer_id != peer_id)
            .cloned()
            .collect()
    }

    fn contains(&self, peer_id: &str) -> bool {
        self.peers.iter().any(|p| p.peer_id == peer_id)
    }

    fn insert(&mut self, peer: PeerInfo) {
        self.peers.push(peer);
    }

    fn remove(&mut self, peer_id: &str) {
        self.peers.retain(|p| p.peer_id != peer_id);
    }
}

/// In-process WebSocket relay.
///
/// Construct via [`RelayServer::new`], bind a listener with
/// [`tokio::net::TcpListener::bind`], and hand it to
/// [`RelayServer::serve_listener`]. The accept loop runs until the
/// listener errors or [`RelayServer::shutdown`] is called; per-peer
/// tasks live for the duration of their sockets unless `shutdown`
/// aborts them.
pub struct RelayServer {
    token: Token,
    broadcast_tx: broadcast::Sender<Routed>,
    registry: Arc<Mutex<PeerRegistry>>,
    /// One-shot shutdown signal observed by `serve_listener` and the
    /// per-peer handler's `Routed` writer. Receivers re-subscribe per
    /// task; a single `send(())` reaches all of them.
    shutdown: broadcast::Sender<()>,
}

impl RelayServer {
    /// Construct a server bound to the given token. The token is
    /// checked against the handshake frame on every connection.
    #[must_use]
    pub fn new(token: Token) -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (shutdown, _) = broadcast::channel(1);
        Self {
            token,
            broadcast_tx,
            registry: Arc::new(Mutex::new(PeerRegistry::default())),
            shutdown,
        }
    }

    /// Signal every running task — the accept loop and every live
    /// per-peer handler — to stop. Safe to call from any task; the
    /// `serve_listener` future returns shortly after.
    ///
    /// Calling `shutdown` more than once is harmless; the broadcast
    /// channel's `send` failures (no subscribers) are silently
    /// ignored.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(());
    }

    /// Run the accept loop on `listener` until either the listener
    /// errors or [`Self::shutdown`] fires. Per-peer tasks are tracked
    /// in a [`JoinSet`] owned by this future, so on shutdown every
    /// live connection is aborted *before* this function returns —
    /// callers re-binding the same port after `serve_listener` exits
    /// don't race the previous listener's child sockets.
    ///
    /// # Errors
    /// Returns [`RelayServerError::Accept`] if the listener fails
    /// outside of an explicit shutdown. Shutdowns return `Ok(())`.
    pub async fn serve_listener(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<(), RelayServerError> {
        let mut shutdown_rx = self.shutdown.subscribe();
        let mut peers: JoinSet<()> = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    tracing::debug!("nexus-collab: shutdown signal, aborting peer tasks");
                    peers.shutdown().await;
                    return Ok(());
                }
                accept = listener.accept() => {
                    let (stream, addr) = accept?;
                    tracing::debug!(peer = %addr, "nexus-collab: accepted TCP");
                    let server = Arc::clone(&self);
                    peers.spawn(async move {
                        if let Err(e) = server.handle_connection(stream).await {
                            tracing::warn!(peer = %addr, error = ?e, "nexus-collab: connection ended with error");
                        }
                    });
                    // Reap completed tasks opportunistically so the
                    // JoinSet doesn't accumulate Joinables forever
                    // even on a busy relay; we don't care about their
                    // return values.
                    while peers.try_join_next().is_some() {}
                }
            }
        }
    }

    /// Per-connection handler. Public-but-`pub(crate)` so the
    /// integration tests can drive a connection directly without going
    /// through a TCP listener.
    pub(crate) async fn handle_connection(&self, stream: TcpStream) -> Result<(), HandleError> {
        let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        config.max_message_size = Some(MAX_FRAME_BYTES);
        config.max_frame_size = Some(MAX_FRAME_BYTES);
        let ws = tokio_tungstenite::accept_async_with_config(stream, Some(config))
            .await
            .map_err(HandleError::WsHandshake)?;
        self.run_peer(ws).await
    }

    /// Drive one accepted WebSocket through the handshake and message
    /// loop. Generic over the stream so tests can plug an in-memory
    /// duplex pipe in place of `TcpStream`.
    pub(crate) async fn run_peer<S>(&self, ws: WebSocketStream<S>) -> Result<(), HandleError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sink, mut stream) = ws.split();

        // ---- handshake ----
        let first = match stream.next().await {
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(Message::Close(_))) | None => return Ok(()),
            Some(Ok(_)) => {
                send_error(&mut sink, ERR_HANDSHAKE, "first frame must be text Hello").await;
                return Ok(());
            }
            Some(Err(e)) => return Err(HandleError::Ws(e)),
        };
        let hello: ClientMessage = match serde_json::from_str(&first) {
            Ok(m) => m,
            Err(e) => {
                send_error(
                    &mut sink,
                    ERR_BAD_FRAME,
                    format!("invalid first frame: {e}"),
                )
                .await;
                return Ok(());
            }
        };
        let (token, peer_id, display_name) = match hello {
            ClientMessage::Hello {
                token,
                peer_id,
                display_name,
            } => (token, peer_id, display_name),
            ClientMessage::Envelope { .. } => {
                send_error(&mut sink, ERR_HANDSHAKE, "first frame must be Hello").await;
                return Ok(());
            }
        };
        if !self.token.verify(&token) {
            send_error(&mut sink, ERR_AUTH, "invalid token").await;
            return Ok(());
        }
        if peer_id.is_empty() {
            send_error(&mut sink, ERR_HANDSHAKE, "peer_id must not be empty").await;
            return Ok(());
        }

        // ---- peer registration ----
        let snapshot;
        {
            let mut reg = self.registry.lock().await;
            if reg.contains(&peer_id) {
                drop(reg);
                send_error(
                    &mut sink,
                    ERR_HANDSHAKE,
                    format!("peer_id '{peer_id}' already connected"),
                )
                .await;
                return Ok(());
            }
            snapshot = reg.snapshot_excluding(&peer_id);
            reg.insert(PeerInfo {
                peer_id: peer_id.clone(),
                display_name: display_name.clone(),
            });
        }

        // Tell the new arrival who else is here.
        let hello_reply = ServerMessage::Hello {
            peer_id: peer_id.clone(),
            peers: snapshot,
        };
        if send_server(&mut sink, &hello_reply).await.is_err() {
            self.cleanup_peer(&peer_id).await;
            return Ok(());
        }

        // Tell everyone else about the new arrival.
        let joined = ServerMessage::PeerJoined {
            peer: PeerInfo {
                peer_id: peer_id.clone(),
                display_name,
            },
        };
        self.broadcast(None, &joined);

        // ---- spawn the write half (drains the broadcast channel) ----
        let mut rx = self.broadcast_tx.subscribe();
        let self_peer_for_writer = peer_id.clone();
        let writer = tokio::spawn(async move {
            loop {
                let routed = match rx.recv().await {
                    Ok(r) => r,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                };
                if routed.from.as_deref() == Some(&self_peer_for_writer) {
                    continue;
                }
                if sink
                    .send(Message::Text(routed.payload.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        // ---- read loop ----
        let read_result = self.pump_reads(&peer_id, &mut stream).await;

        // Whichever side ends first triggers cleanup. Abort the writer
        // explicitly so a dead peer can't leak the task.
        writer.abort();
        self.cleanup_peer(&peer_id).await;
        read_result
    }

    /// Inbound message pump for a single connected peer.
    async fn pump_reads<S>(
        &self,
        peer_id: &str,
        stream: &mut futures_util::stream::SplitStream<WebSocketStream<S>>,
    ) -> Result<(), HandleError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        while let Some(frame) = stream.next().await {
            let text = match frame {
                Ok(Message::Text(t)) => t,
                Ok(Message::Ping(_) | Message::Pong(_)) => continue,
                Ok(Message::Close(_))
                | Err(
                    tokio_tungstenite::tungstenite::Error::ConnectionClosed
                    | tokio_tungstenite::tungstenite::Error::AlreadyClosed,
                ) => break,
                Ok(Message::Binary(_) | Message::Frame(_)) => {
                    tracing::debug!(peer = %peer_id, "nexus-collab: ignoring non-text frame");
                    continue;
                }
                Err(e) => return Err(HandleError::Ws(e)),
            };
            let msg: ClientMessage = match serde_json::from_str(&text) {
                Ok(m) => m,
                Err(e) => {
                    tracing::debug!(peer = %peer_id, error = %e, "nexus-collab: dropping bad frame");
                    continue;
                }
            };
            match msg {
                ClientMessage::Hello { .. } => {
                    tracing::debug!(peer = %peer_id, "nexus-collab: dropping extra Hello");
                }
                ClientMessage::Envelope { topic, payload } => {
                    let out = ServerMessage::Envelope {
                        from: peer_id.to_string(),
                        topic,
                        payload,
                    };
                    self.broadcast(Some(peer_id.to_string()), &out);
                }
            }
        }
        Ok(())
    }

    /// Serialise a server-authored frame and push it onto the
    /// broadcast channel. `from = Some(peer_id)` suppresses echo to
    /// that peer; `from = None` reaches everyone (used for
    /// `PeerJoined` / `PeerLeft`).
    fn broadcast(&self, from: Option<String>, msg: &ServerMessage) {
        let payload = match serde_json::to_string(msg) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "nexus-collab: failed to serialise broadcast");
                return;
            }
        };
        // Failure here means no subscribers — drop silently.
        let _ = self.broadcast_tx.send(Routed { from, payload });
    }

    async fn cleanup_peer(&self, peer_id: &str) {
        let removed = {
            let mut reg = self.registry.lock().await;
            let was_present = reg.contains(peer_id);
            reg.remove(peer_id);
            was_present
        };
        if removed {
            let left = ServerMessage::PeerLeft {
                peer_id: peer_id.to_string(),
            };
            self.broadcast(None, &left);
        }
    }
}

/// Errors raised inside [`RelayServer::run_peer`]. Surfaced to the
/// accept loop, which logs them and moves on.
#[derive(Debug, thiserror::Error)]
pub(crate) enum HandleError {
    /// WebSocket protocol error after the handshake.
    #[error("ws: {0}")]
    Ws(tokio_tungstenite::tungstenite::Error),
    /// WebSocket handshake itself failed.
    #[error("ws handshake: {0}")]
    WsHandshake(tokio_tungstenite::tungstenite::Error),
}

async fn send_error<S>(
    sink: &mut futures_util::stream::SplitSink<WebSocketStream<S>, Message>,
    code: &str,
    message: impl Into<String>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let msg = ServerMessage::error(code, message);
    let _ = send_server(sink, &msg).await;
    let _ = sink.close().await;
}

async fn send_server<S>(
    sink: &mut futures_util::stream::SplitSink<WebSocketStream<S>, Message>,
    msg: &ServerMessage,
) -> Result<(), tokio_tungstenite::tungstenite::Error>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(msg).expect("ServerMessage serialises");
    sink.send(Message::Text(payload.into())).await
}
