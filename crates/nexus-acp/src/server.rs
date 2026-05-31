//! Inbound ACP surface (BL-145 / Hermes Feature 7).
//!
//! [`AcpServer`] reads line-delimited JSON-RPC 2.0 requests from one
//! reader, dispatches each through a [`nexus_kernel::PluginContext`]
//! to a closed allow-list of `com.nexus.agent` IPC verbs, and writes
//! the result/error envelope to one writer. Pure proxy — no
//! capability re-checks beyond what the host's `ipc_call` boundary
//! already enforces.
//!
//! The transport is the same line-delimited framing as the host
//! ([`crate::transport`]). The binary entry-point `nexus acp serve`
//! is the only intended caller; it wires `stdin`/`stdout` and lets a
//! Hermes-compatible parent process drive Nexus's agent loop via
//! JSON-RPC.
//!
//! # Exposed methods
//!
//! | JSON-RPC method | Routed IPC call |
//! |---|---|
//! | `agent/run` | `com.nexus.agent::session_run` |
//! | `agent/list` | `com.nexus.agent::session_list` |
//! | `agent/get` | `com.nexus.agent::session_get` |
//!
//! Unknown methods return JSON-RPC error `-32601` (method not found).
//! Invalid params (missing required fields) return `-32602`. Underlying
//! `ipc_call` failures return `-32000` (server error) with the kernel
//! error string in `message`.

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::Mutex;

use crate::transport::{
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
    TransportError,
};

/// Default per-`ipc_call` timeout. Generous because `agent/run` can
/// drive an LLM tool loop with many round trips.
pub const DEFAULT_DISPATCH_TIMEOUT: Duration = Duration::from_secs(600);

/// Errors raised by [`AcpServer`].
#[derive(Debug, thiserror::Error)]
pub enum AcpServerError {
    /// Wire-level failure on the inbound stream — the parent process
    /// hung up or sent a malformed frame.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// A response failed to write back — the parent process closed
    /// the outbound pipe.
    #[error("write: {0}")]
    Write(String),
}

/// JSON-RPC stdio server that exposes a fixed subset of Nexus's
/// `com.nexus.agent` IPC surface to an external parent process.
///
/// Stateless: every inbound request takes the routing table, looks up
/// the target verb, blocks on `ipc_call`, and writes the response.
/// Concurrent requests share the outbound writer through a Mutex so
/// responses don't tear bytes mid-line.
///
/// The context is held behind an `Arc` because [`KernelPluginContext`]
/// is not `Clone`; mirrors the construction pattern
/// [`nexus_mcp::NexusMcpServer`] uses.
pub struct AcpServer {
    context: Arc<KernelPluginContext>,
    timeout: Duration,
}

impl AcpServer {
    /// Construct a server bound to `context`. The context is the only
    /// shared state — every method dispatch routes through
    /// `context.ipc_call(...)`.
    #[must_use]
    pub fn new(context: Arc<KernelPluginContext>) -> Self {
        Self {
            context,
            timeout: DEFAULT_DISPATCH_TIMEOUT,
        }
    }

    /// Override the default per-call IPC timeout. Mostly useful for
    /// tests; the binary entry point keeps the default.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Serve requests on the supplied `reader`/`writer` pair until
    /// the reader returns EOF. Each inbound request is dispatched
    /// sequentially so request ordering is preserved on the wire.
    ///
    /// # Errors
    /// - [`AcpServerError::Transport`] only when the reader fails
    ///   irrecoverably (read past EOF returns `Ok(())`).
    /// - [`AcpServerError::Write`] when the outbound writer breaks.
    pub async fn serve<R, W>(&self, reader: R, writer: W) -> Result<(), AcpServerError>
    where
        R: AsyncRead + Unpin + Send,
        W: AsyncWrite + Unpin + Send,
    {
        let mut reader = BufReader::new(reader);
        let writer = Arc::new(Mutex::new(writer));
        loop {
            let msg = match read_message(&mut reader).await {
                Ok(m) => m,
                Err(TransportError::Eof) => return Ok(()),
                Err(e) => return Err(AcpServerError::Transport(e)),
            };
            match msg {
                JsonRpcMessage::Request(req) => {
                    let response = self.dispatch_request(req).await;
                    let mut w = writer.lock().await;
                    write_message(&mut *w, &JsonRpcMessage::Response(response))
                        .await
                        .map_err(|e| AcpServerError::Write(e.to_string()))?;
                }
                JsonRpcMessage::Notification(_) => {
                    // ACP server doesn't subscribe to client-sent
                    // notifications today — silently ignore.
                }
                JsonRpcMessage::Response(_) => {
                    // We don't issue server-initiated requests, so a
                    // response on the inbound stream is unexpected. Log
                    // and continue.
                    tracing::warn!("acp server: unexpected response on inbound stream");
                }
            }
        }
    }

    async fn dispatch_request(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone();
        let params = req.params.unwrap_or(Value::Null);
        let routed = route_method(&req.method);
        match routed {
            RoutedMethod::Unknown => {
                error_response(id, -32601, format!("method not found: {}", req.method))
            }
            RoutedMethod::Known { plugin_id, command } => match self
                .context
                .ipc_call(plugin_id, command, params, self.timeout)
                .await
            {
                Ok(v) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(v),
                    error: None,
                },
                Err(e) => error_response(id, -32000, format!("server error: {e}")),
            },
        }
    }
}

/// Method routing table. Pure function — separated from
/// `dispatch_request` so unit tests don't need a live runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedMethod {
    /// Method maps to a known `(plugin_id, command)` pair.
    Known {
        /// Reverse-DNS id of the target plugin.
        plugin_id: &'static str,
        /// IPC command name.
        command: &'static str,
    },
    /// Method is not on the allow-list. Server returns
    /// `-32601 method not found`.
    Unknown,
}

#[must_use]
pub fn route_method(method: &str) -> RoutedMethod {
    match method {
        "agent/run" => RoutedMethod::Known {
            plugin_id: "com.nexus.agent",
            command: "session_run",
        },
        "agent/list" => RoutedMethod::Known {
            plugin_id: "com.nexus.agent",
            command: "session_list",
        },
        "agent/get" => RoutedMethod::Known {
            plugin_id: "com.nexus.agent",
            command: "session_get",
        },
        _ => RoutedMethod::Unknown,
    }
}

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

/// Construct a JSON-RPC `-32602 invalid params` response. Exposed for
/// callers that pre-validate inbound params before reaching the IPC
/// boundary.
#[must_use]
pub fn invalid_params_response(id: Value, message: impl Into<String>) -> JsonRpcResponse {
    error_response(id, -32602, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn route_method_table_covers_three_documented_verbs() {
        assert_eq!(
            route_method("agent/run"),
            RoutedMethod::Known {
                plugin_id: "com.nexus.agent",
                command: "session_run",
            },
        );
        assert_eq!(
            route_method("agent/list"),
            RoutedMethod::Known {
                plugin_id: "com.nexus.agent",
                command: "session_list",
            },
        );
        assert_eq!(
            route_method("agent/get"),
            RoutedMethod::Known {
                plugin_id: "com.nexus.agent",
                command: "session_get",
            },
        );
        assert_eq!(route_method("agent/nope"), RoutedMethod::Unknown);
        assert_eq!(route_method(""), RoutedMethod::Unknown);
        assert_eq!(route_method("session_run"), RoutedMethod::Unknown);
    }

    #[test]
    fn invalid_params_response_shape_is_jsonrpc_compliant() {
        let r = invalid_params_response(json!(7), "missing 'goal'");
        assert_eq!(r.jsonrpc, "2.0");
        assert_eq!(r.id, json!(7));
        assert!(r.result.is_none());
        let err = r.error.unwrap();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing"));
    }

    #[test]
    fn error_response_serialises_without_result_field() {
        let r = error_response(json!(1), -32601, "method not found: x".into());
        let body = serde_json::to_string(&r).unwrap();
        // `result` is `None` and `skip_serializing_if`d.
        assert!(!body.contains("\"result\""));
        assert!(body.contains("\"error\""));
        assert!(body.contains("-32601"));
    }
}
