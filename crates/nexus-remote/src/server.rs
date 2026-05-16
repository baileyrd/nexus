//! Remote-forge JSON-RPC server (BL-140 Phase 1).
//!
//! [`RemoteServer`] reads line-delimited JSON-RPC 2.0 frames from one
//! reader, dispatches each through a [`nexus_kernel::PluginContext`] +
//! the kernel [`EventBus`], and writes results / event notifications to
//! one writer.
//!
//! Concurrent inbound requests each spawn a task that locks the
//! outbound writer briefly to send the response. Event subscriptions
//! each own a long-lived task that forwards matching events as
//! server-pushed `event` notifications until either the client sends
//! `event_unsubscribe` or the inbound stream closes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{EventBus, KernelPluginContext, PluginContext};
use nexus_plugin_api::{EventFilter, PublishedEvent};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::transport::{
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, TransportError,
};

/// Default per-`ipc_call` timeout when the client doesn't override it.
/// Generous because some IPC verbs (agent runs, long-running indexer
/// jobs) legitimately take minutes.
pub const DEFAULT_DISPATCH_TIMEOUT: Duration = Duration::from_secs(600);

/// Hard ceiling on the override the client may request. Prevents a
/// runaway client from pinning a backend slot forever.
pub const MAX_DISPATCH_TIMEOUT: Duration = Duration::from_secs(3600);

/// Errors raised by [`RemoteServer::serve`].
#[derive(Debug, thiserror::Error)]
pub enum RemoteServerError {
    /// Wire-level failure on the inbound stream.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// A response failed to write back — the parent closed the outbound
    /// pipe.
    #[error("write: {0}")]
    Write(String),
}

/// JSON-RPC stdio server that proxies the full kernel IPC + event bus
/// surface to a remote frontend.
///
/// Stateless across requests for `ipc_call`. Event subscriptions are
/// tracked in an internal `HashMap<subscription_id, JoinHandle>` so
/// `event_unsubscribe` can stop the forwarder task; the map is dropped
/// (and every task aborted) when `serve` returns.
pub struct RemoteServer {
    context: Arc<KernelPluginContext>,
    event_bus: Arc<EventBus>,
    timeout: Duration,
}

impl RemoteServer {
    /// Construct a server bound to `context` + `event_bus`.
    #[must_use]
    pub fn new(context: Arc<KernelPluginContext>, event_bus: Arc<EventBus>) -> Self {
        Self {
            context,
            event_bus,
            timeout: DEFAULT_DISPATCH_TIMEOUT,
        }
    }

    /// Override the default per-call IPC timeout (the client may still
    /// request a shorter one via `params.timeout_ms`). Mostly useful
    /// for tests.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Serve requests until the reader returns EOF.
    ///
    /// # Errors
    /// - [`RemoteServerError::Transport`] when the reader fails
    ///   irrecoverably (EOF returns `Ok(())`).
    /// - [`RemoteServerError::Write`] when the outbound writer breaks.
    pub async fn serve<R, W>(
        &self,
        reader: R,
        writer: W,
    ) -> Result<(), RemoteServerError>
    where
        R: AsyncRead + Unpin + Send,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let mut reader = BufReader::new(reader);
        let writer: Arc<Mutex<W>> = Arc::new(Mutex::new(writer));
        let subscriptions: Arc<Mutex<HashMap<String, JoinHandle<()>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        loop {
            let msg = match read_message(&mut reader).await {
                Ok(m) => m,
                Err(TransportError::Eof) => break,
                Err(e) => {
                    // Abort outstanding subscription tasks before
                    // surfacing the error.
                    abort_all(&subscriptions).await;
                    return Err(RemoteServerError::Transport(e));
                }
            };
            match msg {
                JsonRpcMessage::Request(req) => {
                    self.dispatch_request(
                        req,
                        Arc::clone(&writer),
                        Arc::clone(&subscriptions),
                    )
                    .await;
                }
                JsonRpcMessage::Notification(_) => {
                    // Remote-forge server doesn't accept client-pushed
                    // notifications today — silently ignore.
                }
                JsonRpcMessage::Response(_) => {
                    tracing::warn!(
                        "nexus-remote server: unexpected response on inbound stream"
                    );
                }
            }
        }

        abort_all(&subscriptions).await;
        Ok(())
    }

    /// Dispatch one inbound request. Spawns a task so a slow `ipc_call`
    /// doesn't block subsequent requests on the same connection.
    async fn dispatch_request<W>(
        &self,
        req: JsonRpcRequest,
        writer: Arc<Mutex<W>>,
        subscriptions: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    ) where
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let id = req.id.clone();
        let method = req.method.clone();
        let params = req.params.unwrap_or(Value::Null);

        match method.as_str() {
            "ipc_call" => {
                let ctx = Arc::clone(&self.context);
                let default_timeout = self.timeout;
                tokio::spawn(async move {
                    let response = handle_ipc_call(&ctx, params, default_timeout, id).await;
                    write_response(&writer, response).await;
                });
            }
            "event_subscribe" => {
                let bus = Arc::clone(&self.event_bus);
                let writer_for_task = Arc::clone(&writer);
                tokio::spawn(async move {
                    let response = handle_event_subscribe(
                        &bus,
                        params,
                        id,
                        writer_for_task,
                        subscriptions,
                    )
                    .await;
                    write_response(&writer, response).await;
                });
            }
            "event_unsubscribe" => {
                tokio::spawn(async move {
                    let response = handle_event_unsubscribe(params, id, subscriptions).await;
                    write_response(&writer, response).await;
                });
            }
            _ => {
                let response =
                    error_response(id, -32601, format!("method not found: {method}"));
                write_response(&writer, response).await;
            }
        }
    }
}

// ---- ipc_call ---------------------------------------------------------------

async fn handle_ipc_call(
    ctx: &KernelPluginContext,
    params: Value,
    default_timeout: Duration,
    id: Value,
) -> JsonRpcResponse {
    let plugin_id = match params.get("plugin_id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return invalid_params(id, "missing or empty 'plugin_id'"),
    };
    let command = match params.get("command").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return invalid_params(id, "missing or empty 'command'"),
    };
    let args = params.get("args").cloned().unwrap_or(Value::Null);
    let timeout = match parse_timeout_ms(params.get("timeout_ms"), default_timeout) {
        Ok(t) => t,
        Err(msg) => return invalid_params(id, msg),
    };

    match ctx.ipc_call(&plugin_id, &command, args, timeout).await {
        Ok(v) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(v),
            error: None,
        },
        Err(e) => error_response(id, -32000, format!("ipc_call failed: {e}")),
    }
}

/// Parse the optional `timeout_ms` field. Caps at [`MAX_DISPATCH_TIMEOUT`].
fn parse_timeout_ms(
    value: Option<&Value>,
    default: Duration,
) -> Result<Duration, String> {
    let Some(v) = value else { return Ok(default) };
    if v.is_null() {
        return Ok(default);
    }
    let ms = v
        .as_u64()
        .ok_or_else(|| "'timeout_ms' must be a non-negative integer".to_string())?;
    if ms == 0 {
        return Err("'timeout_ms' must be > 0".to_string());
    }
    let d = Duration::from_millis(ms);
    if d > MAX_DISPATCH_TIMEOUT {
        return Ok(MAX_DISPATCH_TIMEOUT);
    }
    Ok(d)
}

// ---- event_subscribe --------------------------------------------------------

async fn handle_event_subscribe<W>(
    bus: &EventBus,
    params: Value,
    id: Value,
    writer: Arc<Mutex<W>>,
    subscriptions: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) -> JsonRpcResponse
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let subscription_id =
        match params.get("subscription_id").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return invalid_params(id, "missing or empty 'subscription_id'"),
        };
    let filter = match params.get("filter") {
        Some(f) => match parse_filter(f) {
            Ok(f) => f,
            Err(msg) => return invalid_params(id, msg),
        },
        None => return invalid_params(id, "missing 'filter'"),
    };

    // Reject duplicate ids so the client can't quietly clobber an
    // existing subscription.
    {
        let map = subscriptions.lock().await;
        if map.contains_key(&subscription_id) {
            return error_response(
                id,
                -32000,
                format!("subscription_id '{subscription_id}' already in use"),
            );
        }
    }

    let mut sub = bus.subscribe(filter);
    let sid_for_task = subscription_id.clone();
    let writer_for_task = Arc::clone(&writer);
    let handle = tokio::spawn(async move {
        while let Ok(event) = sub.recv().await {
            let notif = build_event_notification(&sid_for_task, &event);
            let mut w = writer_for_task.lock().await;
            if write_message(&mut *w, &JsonRpcMessage::Notification(notif))
                .await
                .is_err()
            {
                // Outbound pipe broke — give up on this subscription.
                // The serve loop will hit EOF on the reader and clean
                // up shortly.
                break;
            }
        }
    });

    subscriptions
        .lock()
        .await
        .insert(subscription_id.clone(), handle);

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(json!({ "subscription_id": subscription_id })),
        error: None,
    }
}

/// Parse the `filter` field of an `event_subscribe` request.
///
/// Wire shape (one of):
/// - `{ "kind": "all" }`
/// - `{ "kind": "variant", "name": "PluginLoaded" }`
/// - `{ "kind": "custom_prefix", "prefix": "com.nexus.editor." }`
/// - `{ "kind": "custom_exact", "type_id": "com.nexus.editor.saved" }`
fn parse_filter(v: &Value) -> Result<EventFilter, String> {
    let kind = v
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "filter missing 'kind' (string)".to_string())?;
    match kind {
        "all" => Ok(EventFilter::All),
        "variant" => {
            let name = v
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "filter kind=variant requires 'name'".to_string())?;
            Ok(EventFilter::Variant(name.to_string()))
        }
        "custom_prefix" => {
            let prefix = v
                .get("prefix")
                .and_then(Value::as_str)
                .ok_or_else(|| "filter kind=custom_prefix requires 'prefix'".to_string())?;
            Ok(EventFilter::CustomPrefix(prefix.to_string()))
        }
        "custom_exact" => {
            let type_id = v.get("type_id").and_then(Value::as_str).ok_or_else(|| {
                "filter kind=custom_exact requires 'type_id'".to_string()
            })?;
            Ok(EventFilter::CustomExact(type_id.to_string()))
        }
        other => Err(format!("unknown filter kind: {other}")),
    }
}

/// Build the `event` JSON-RPC notification payload for one delivered
/// event.
///
/// `event` is a serde-serialised `PublishedEvent` — same shape the
/// shell already consumes from `kernel_subscribe` over Tauri IPC, so
/// remote-forge clients can reuse the existing event decoding.
fn build_event_notification(
    subscription_id: &str,
    event: &Arc<PublishedEvent>,
) -> JsonRpcNotification {
    let event_value =
        serde_json::to_value(event.as_ref()).unwrap_or(Value::Null);
    JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "event".to_string(),
        params: Some(json!({
            "subscription_id": subscription_id,
            "event": event_value,
        })),
    }
}

// ---- event_unsubscribe ------------------------------------------------------

async fn handle_event_unsubscribe(
    params: Value,
    id: Value,
    subscriptions: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) -> JsonRpcResponse {
    let subscription_id =
        match params.get("subscription_id").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return invalid_params(id, "missing or empty 'subscription_id'"),
        };
    let mut map = subscriptions.lock().await;
    let removed = map.remove(&subscription_id);
    let result = if let Some(handle) = removed {
        handle.abort();
        json!({ "ok": true })
    } else {
        json!({ "ok": false, "reason": "unknown subscription_id" })
    };
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

// ---- shared helpers ---------------------------------------------------------

async fn write_response<W>(writer: &Arc<Mutex<W>>, response: JsonRpcResponse)
where
    W: AsyncWrite + Unpin + Send,
{
    let mut w = writer.lock().await;
    if let Err(e) =
        write_message(&mut *w, &JsonRpcMessage::Response(response)).await
    {
        tracing::warn!(error = %e, "nexus-remote: failed to write response");
    }
}

async fn abort_all(
    subscriptions: &Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) {
    let mut map = subscriptions.lock().await;
    for (_, handle) in map.drain() {
        handle.abort();
    }
}

#[must_use]
fn error_response(id: Value, code: i64, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}

#[must_use]
fn invalid_params(id: Value, message: impl Into<String>) -> JsonRpcResponse {
    error_response(id, -32602, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_filter_all() {
        assert!(matches!(
            parse_filter(&json!({"kind": "all"})).unwrap(),
            EventFilter::All
        ));
    }

    #[test]
    fn parse_filter_variant() {
        let f = parse_filter(&json!({"kind": "variant", "name": "PluginLoaded"})).unwrap();
        match f {
            EventFilter::Variant(n) => assert_eq!(n, "PluginLoaded"),
            other => panic!("expected Variant, got {other:?}"),
        }
    }

    #[test]
    fn parse_filter_custom_prefix() {
        let f = parse_filter(&json!({"kind": "custom_prefix", "prefix": "com.nexus."}))
            .unwrap();
        match f {
            EventFilter::CustomPrefix(p) => assert_eq!(p, "com.nexus."),
            other => panic!("expected CustomPrefix, got {other:?}"),
        }
    }

    #[test]
    fn parse_filter_custom_exact() {
        let f = parse_filter(&json!({"kind": "custom_exact", "type_id": "com.x.y"})).unwrap();
        match f {
            EventFilter::CustomExact(t) => assert_eq!(t, "com.x.y"),
            other => panic!("expected CustomExact, got {other:?}"),
        }
    }

    #[test]
    fn parse_filter_rejects_missing_kind() {
        let err = parse_filter(&json!({})).unwrap_err();
        assert!(err.contains("kind"));
    }

    #[test]
    fn parse_filter_rejects_unknown_kind() {
        let err = parse_filter(&json!({"kind": "frobnicate"})).unwrap_err();
        assert!(err.contains("frobnicate"));
    }

    #[test]
    fn parse_filter_variant_requires_name() {
        let err = parse_filter(&json!({"kind": "variant"})).unwrap_err();
        assert!(err.contains("name"));
    }

    #[test]
    fn parse_timeout_ms_defaults_when_absent() {
        let d = parse_timeout_ms(None, Duration::from_secs(7)).unwrap();
        assert_eq!(d, Duration::from_secs(7));
    }

    #[test]
    fn parse_timeout_ms_defaults_on_null() {
        let d = parse_timeout_ms(Some(&Value::Null), Duration::from_secs(7)).unwrap();
        assert_eq!(d, Duration::from_secs(7));
    }

    #[test]
    fn parse_timeout_ms_accepts_positive() {
        let d = parse_timeout_ms(Some(&json!(2500)), Duration::from_secs(7)).unwrap();
        assert_eq!(d, Duration::from_millis(2500));
    }

    #[test]
    fn parse_timeout_ms_caps_at_max() {
        let cap_ms =
            u64::try_from(MAX_DISPATCH_TIMEOUT.as_millis()).expect("cap fits in u64");
        let huge = cap_ms.saturating_mul(10);
        let d = parse_timeout_ms(Some(&json!(huge)), Duration::from_secs(7)).unwrap();
        assert_eq!(d, MAX_DISPATCH_TIMEOUT);
    }

    #[test]
    fn parse_timeout_ms_rejects_zero() {
        let err = parse_timeout_ms(Some(&json!(0)), Duration::from_secs(7)).unwrap_err();
        assert!(err.contains("> 0"));
    }

    #[test]
    fn parse_timeout_ms_rejects_non_numeric() {
        let err = parse_timeout_ms(Some(&json!("oops")), Duration::from_secs(7))
            .unwrap_err();
        assert!(err.contains("non-negative integer"));
    }

    #[test]
    fn error_response_serialises_without_result_field() {
        let r = error_response(json!(1), -32601, "method not found: x".into());
        let body = serde_json::to_string(&r).unwrap();
        assert!(!body.contains("\"result\""));
        assert!(body.contains("\"error\""));
        assert!(body.contains("-32601"));
    }

    #[test]
    fn invalid_params_response_shape_is_jsonrpc_compliant() {
        let r = invalid_params(json!(7), "missing 'plugin_id'");
        assert_eq!(r.jsonrpc, "2.0");
        assert_eq!(r.id, json!(7));
        assert!(r.result.is_none());
        let err = r.error.unwrap();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing"));
    }

    #[test]
    fn build_event_notification_carries_subscription_id_and_event() {
        use nexus_plugin_api::{EventMetadata, NexusEvent};
        let ev = Arc::new(PublishedEvent {
            metadata: EventMetadata {
                event_id: uuid::Uuid::nil(),
                timestamp: chrono::Utc::now(),
                source_plugin_id: "kernel".to_string(),
                span_id: None,
            },
            event: NexusEvent::PluginStarted {
                plugin_id: "com.test".to_string(),
            },
        });
        let n = build_event_notification("sub-1", &ev);
        assert_eq!(n.method, "event");
        let p = n.params.unwrap();
        assert_eq!(p["subscription_id"], json!("sub-1"));
        assert!(p["event"].is_object());
    }
}
