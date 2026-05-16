//! Remote-forge JSON-RPC client (BL-140 Phase 2a).
//!
//! Inverse of [`crate::server::RemoteServer`]. Owns one reader / one
//! writer half of a duplex pipe (in practice: an SSH child's stdout /
//! stdin, but the client is transport-agnostic), runs a background
//! response-router task that demultiplexes inbound frames into:
//!
//! - **Responses** → resolved on the matching per-request oneshot.
//! - **`event` notifications** → fanned out to the subscriber's mpsc
//!   receiver keyed by `subscription_id`.
//!
//! Public API mirrors the server's three methods plus an explicit
//! `shutdown` so callers can drain in-flight requests cleanly.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex, Notify};
use tokio::task::JoinHandle;

use crate::transport::{
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcRequest,
    JsonRpcResponse, TransportError,
};

/// Default per-call timeout. Mirrors the server's
/// [`crate::server::DEFAULT_DISPATCH_TIMEOUT`].
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(600);

/// Errors raised by [`RemoteClient`].
#[derive(Debug, thiserror::Error)]
pub enum RemoteClientError {
    /// I/O failure on the outbound writer.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// The router task has stopped — the inbound stream is gone, so
    /// every subsequent call would hang forever. The caller should
    /// shut down and rebuild.
    #[error("router task stopped — connection is dead")]
    RouterStopped,
    /// The server replied with a JSON-RPC error envelope.
    #[error("server error (code {code}): {message}")]
    Server {
        /// JSON-RPC error code (e.g. -32601 method not found).
        code: i64,
        /// Server-supplied error message.
        message: String,
    },
    /// The per-call deadline elapsed.
    #[error("call timed out after {0:?}")]
    Timeout(Duration),
    /// The server's response is missing both `result` and `error`.
    #[error("server response had neither result nor error")]
    MalformedResponse,
}

/// Inbound `event` notification delivered to a subscriber.
#[derive(Debug, Clone)]
pub struct EventDelivery {
    /// `subscription_id` echoed by the server.
    pub subscription_id: String,
    /// Serde-serialised `PublishedEvent` payload.
    pub event: Value,
}

/// Outbound client over a paired reader/writer.
///
/// Construct via [`RemoteClient::new`]; the spawned router task lives
/// for the client's lifetime. Drop or call [`RemoteClient::shutdown`]
/// to tear everything down.
pub struct RemoteClient {
    writer: Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    subscribers: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<EventDelivery>>>>,
    next_id: AtomicU64,
    router: Mutex<Option<JoinHandle<()>>>,
    default_timeout: Duration,
    /// Fires once when the inbound router task exits (transport EOF /
    /// read error / explicit shutdown). Subscribed to by
    /// [`Self::wait_for_disconnect`] so callers above the client (the
    /// reconnecting runtime, in particular) can react to a dead
    /// connection without polling.
    disconnect_notify: Arc<Notify>,
}

impl RemoteClient {
    /// Wire the client to a paired reader/writer + spawn the response
    /// router.
    ///
    /// `writer` is `Box<dyn AsyncWrite>` so callers don't have to
    /// thread the concrete type (SSH child stdin, duplex half, …)
    /// through every layer.
    pub fn new<R>(
        reader: R,
        writer: Box<dyn AsyncWrite + Unpin + Send>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let subscribers: Arc<
            Mutex<HashMap<String, mpsc::UnboundedSender<EventDelivery>>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        let pending_for_task = Arc::clone(&pending);
        let subscribers_for_task = Arc::clone(&subscribers);
        let disconnect_notify = Arc::new(Notify::new());
        let disconnect_for_task = Arc::clone(&disconnect_notify);

        let router = tokio::spawn(async move {
            run_router(reader, pending_for_task, subscribers_for_task).await;
            // Router exited (EOF / read error / abort) — wake every
            // waiter on the disconnect notify so callers above can react.
            disconnect_for_task.notify_waiters();
        });

        Self {
            writer: Arc::new(Mutex::new(writer)),
            pending,
            subscribers,
            next_id: AtomicU64::new(1),
            router: Mutex::new(Some(router)),
            default_timeout: DEFAULT_CALL_TIMEOUT,
            disconnect_notify,
        }
    }

    /// Resolves the next time the inbound router task exits (transport
    /// EOF, read error, or explicit [`Self::shutdown`]). Callers that
    /// have already observed a disconnect must not re-await this future
    /// — the notify is `notify_waiters`, not `notify_one`, so it fires
    /// exactly once when the router stops. Use it to drive a watchdog
    /// task above the client (BL-146).
    pub fn wait_for_disconnect(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let notify = Arc::clone(&self.disconnect_notify);
        async move { notify.notified().await }
    }

    /// Override the default per-call timeout. Per-call `timeout` args
    /// on individual `ipc_call`s still win when supplied.
    #[must_use]
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Issue a remote `ipc_call`. Equivalent to
    /// `PluginContext::ipc_call` against a local kernel.
    ///
    /// # Errors
    /// - [`RemoteClientError::Server`] on `-32xxx` JSON-RPC errors.
    /// - [`RemoteClientError::Timeout`] if no response arrives in
    ///   `timeout`.
    /// - [`RemoteClientError::Transport`] on an outbound write failure.
    /// - [`RemoteClientError::RouterStopped`] when the inbound stream
    ///   is already gone.
    pub async fn ipc_call(
        &self,
        plugin_id: &str,
        command: &str,
        args: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, RemoteClientError> {
        let effective = timeout.unwrap_or(self.default_timeout);
        // Inline `timeout_ms` on the wire only when the caller
        // explicitly overrode; otherwise the server's default applies.
        let params = if let Some(t) = timeout {
            let ms = u64::try_from(t.as_millis()).unwrap_or(u64::MAX);
            json!({
                "plugin_id": plugin_id,
                "command": command,
                "args": args,
                "timeout_ms": ms,
            })
        } else {
            json!({
                "plugin_id": plugin_id,
                "command": command,
                "args": args,
            })
        };
        let response = self
            .request_with_timeout("ipc_call", params, effective)
            .await?;
        result_or_err(response)
    }

    /// Subscribe to events on the remote kernel bus.
    ///
    /// Returns the subscription id (echoed from the server, equal to
    /// the one supplied here) on success. Matching `event`
    /// notifications stream through `sink` until the subscription is
    /// cancelled with [`Self::unsubscribe`] or the connection drops.
    ///
    /// # Errors
    /// - [`RemoteClientError::Server`] if the server rejects the
    ///   subscription (e.g. duplicate id).
    /// - Same transport / router / timeout variants as
    ///   [`Self::ipc_call`].
    pub async fn subscribe(
        &self,
        subscription_id: &str,
        filter: Value,
        sink: mpsc::UnboundedSender<EventDelivery>,
    ) -> Result<String, RemoteClientError> {
        // Register the subscriber before issuing the request so a fast
        // event that arrives before the response can find its sink.
        {
            let mut map = self.subscribers.lock().await;
            map.insert(subscription_id.to_string(), sink);
        }

        let params = json!({
            "subscription_id": subscription_id,
            "filter": filter,
        });
        match self
            .request_with_timeout("event_subscribe", params, self.default_timeout)
            .await
        {
            Ok(response) => {
                let value = result_or_err(response)?;
                let echoed = value
                    .get("subscription_id")
                    .and_then(Value::as_str)
                    .unwrap_or(subscription_id)
                    .to_string();
                Ok(echoed)
            }
            Err(e) => {
                // Roll back the pre-emptive sink registration so a
                // failed subscribe doesn't leave a dangling channel.
                self.subscribers.lock().await.remove(subscription_id);
                Err(e)
            }
        }
    }

    /// Cancel a subscription. The matching `event` notifications stop
    /// arriving after the server acknowledges; in-flight notifications
    /// that already left the server may still arrive briefly.
    ///
    /// # Errors
    /// Same transport / router / timeout variants as
    /// [`Self::ipc_call`]. A server "unknown `subscription_id`" reply is
    /// returned as `Ok(false)` rather than an error so callers can
    /// safely call unsubscribe twice.
    pub async fn unsubscribe(
        &self,
        subscription_id: &str,
    ) -> Result<bool, RemoteClientError> {
        let params = json!({ "subscription_id": subscription_id });
        let response = self
            .request_with_timeout("event_unsubscribe", params, self.default_timeout)
            .await?;
        let value = result_or_err(response)?;
        // Always drop the sink locally — even if the server says it
        // never knew about this id, we should clear our own state.
        self.subscribers.lock().await.remove(subscription_id);
        let ok = value
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(ok)
    }

    /// Stop the router task and flush the outbound writer. After
    /// shutdown every method returns
    /// [`RemoteClientError::RouterStopped`].
    pub async fn shutdown(&self) {
        if let Some(handle) = self.router.lock().await.take() {
            handle.abort();
        }
        // Drop all subscribers + drain pending oneshots so callers
        // waiting on a response wake up immediately with
        // RouterStopped.
        let mut pending = self.pending.lock().await;
        pending.clear();
        let mut subs = self.subscribers.lock().await;
        subs.clear();
        let mut w = self.writer.lock().await;
        let _ = w.flush().await;
        // `abort()` cancels mid-task before the router can fire the
        // notify itself, so fire it here so a watchdog blocked on
        // `wait_for_disconnect` wakes up promptly.
        self.disconnect_notify.notify_waiters();
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<JsonRpcResponse, RemoteClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(params),
        };

        let write_result = {
            let mut w = self.writer.lock().await;
            write_message(&mut *w, &JsonRpcMessage::Request(req)).await
        };
        if let Err(e) = write_result {
            self.pending.lock().await.remove(&id);
            return Err(RemoteClientError::Transport(e));
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_canceled)) => {
                // The router task dropped the oneshot. Connection
                // gone.
                Err(RemoteClientError::RouterStopped)
            }
            Err(_elapsed) => {
                self.pending.lock().await.remove(&id);
                Err(RemoteClientError::Timeout(timeout))
            }
        }
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        // Use try_lock to avoid blocking on a runtime-less Drop.
        if let Ok(mut router) = self.router.try_lock() {
            if let Some(handle) = router.take() {
                handle.abort();
            }
        }
    }
}

/// Pull a result out of a response envelope. Surfaces JSON-RPC errors
/// as [`RemoteClientError::Server`].
fn result_or_err(response: JsonRpcResponse) -> Result<Value, RemoteClientError> {
    if let Some(err) = response.error {
        return Err(RemoteClientError::Server {
            code: err.code,
            message: err.message,
        });
    }
    response
        .result
        .ok_or(RemoteClientError::MalformedResponse)
}

async fn run_router<R>(
    reader: R,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    subscribers: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<EventDelivery>>>>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    loop {
        let msg = match read_message(&mut reader).await {
            Ok(m) => m,
            Err(TransportError::Eof) => break,
            Err(e) => {
                tracing::warn!(error = %e, "nexus-remote client: inbound read failed");
                break;
            }
        };
        match msg {
            JsonRpcMessage::Response(resp) => {
                let id = resp.id.as_u64();
                if let Some(id) = id {
                    let sender = pending.lock().await.remove(&id);
                    if let Some(sender) = sender {
                        let _ = sender.send(resp);
                    } else {
                        tracing::warn!(
                            id,
                            "nexus-remote client: response for unknown id"
                        );
                    }
                } else {
                    tracing::warn!(
                        "nexus-remote client: response with non-integer id"
                    );
                }
            }
            JsonRpcMessage::Notification(n) if n.method == "event" => {
                let Some(params) = n.params else { continue };
                let Some(sid) = params
                    .get("subscription_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    continue;
                };
                let event = params
                    .get("event")
                    .cloned()
                    .unwrap_or(Value::Null);
                let sink = subscribers.lock().await.get(&sid).cloned();
                if let Some(sink) = sink {
                    let _ = sink.send(EventDelivery {
                        subscription_id: sid,
                        event,
                    });
                }
            }
            JsonRpcMessage::Notification(other) => {
                tracing::debug!(
                    method = %other.method,
                    "nexus-remote client: ignoring non-event notification"
                );
            }
            JsonRpcMessage::Request(req) => {
                // Server-initiated requests aren't part of the Phase 2a
                // contract. Reply with method-not-found so a future
                // protocol extension surfaces clearly.
                tracing::warn!(
                    method = %req.method,
                    "nexus-remote client: ignoring server-initiated request"
                );
                let _ = JsonRpcError {
                    code: -32601,
                    message: format!("method not found: {}", req.method),
                    data: None,
                };
            }
        }
    }
    // Stream ended — wake everyone waiting on a response so they don't
    // hang forever.
    let mut p = pending.lock().await;
    p.clear();
    let mut s = subscribers.lock().await;
    s.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_or_err_surfaces_error_envelope() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: "boom".to_string(),
                data: None,
            }),
        };
        let err = result_or_err(resp).unwrap_err();
        match err {
            RemoteClientError::Server { code, message } => {
                assert_eq!(code, -32000);
                assert_eq!(message, "boom");
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }

    #[test]
    fn result_or_err_returns_value() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            result: Some(json!({"x": 1})),
            error: None,
        };
        let v = result_or_err(resp).unwrap();
        assert_eq!(v["x"], json!(1));
    }

    #[test]
    fn result_or_err_rejects_empty_response() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            result: None,
            error: None,
        };
        assert!(matches!(
            result_or_err(resp),
            Err(RemoteClientError::MalformedResponse)
        ));
    }
}
