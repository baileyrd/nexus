//! One ACP client = one running agent child process.
//!
//! Lifecycle, top-down:
//!
//! 1. [`AcpClient::connect`] spawns the configured executable, opens
//!    stdin/stdout pipes, and starts a reader task that demultiplexes
//!    inbound JSON-RPC messages into pending-request slots and a
//!    notification channel.
//! 2. The caller drives [`AcpClient::send_request`] /
//!    [`AcpClient::send_notification`] for outbound traffic and pulls
//!    pushed events via [`AcpClient::drain_notifications`].
//! 3. On explicit shutdown the client closes stdin and waits up to 5s
//!    for the child to exit; otherwise `kill_on_drop` reaps it.
//!
//! Unlike [`nexus-lsp::client`], ACP carries no document-tracking
//! state and no server-initiated-request reply table — agents don't
//! ask the host for configuration the way rust-analyzer does. If a
//! future ACP draft adds that, mirror the LSP pattern.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::config::AcpAdapterSpec;
use crate::transport::{
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, TransportError,
};

/// Errors raised by [`AcpClient`].
#[derive(Debug, thiserror::Error)]
pub enum AcpClientError {
    /// Failed to spawn the configured executable.
    #[error("spawn '{command}': {source}")]
    Spawn {
        /// Command we tried to run.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Initialize handshake failed (agent returned an error or timed
    /// out before responding).
    #[error("handshake with '{agent}': {reason}")]
    Handshake {
        /// Agent name from the adapter spec.
        agent: String,
        /// Failure summary.
        reason: String,
    },
    /// Wire-level transport failure (broken pipe, malformed frame).
    /// Pool callers should treat this as transient and reconnect.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// Agent returned a JSON-RPC error in response to a request.
    #[error("agent error {code}: {message}")]
    AgentError {
        /// JSON-RPC error code.
        code: i64,
        /// Human-readable summary.
        message: String,
    },
    /// Request did not get a response within the deadline.
    #[error("request '{method}' timed out after {ms}ms")]
    RequestTimeout {
        /// Method that timed out.
        method: String,
        /// Configured deadline in milliseconds.
        ms: u64,
    },
    /// Reader task exited or the child closed the pipe; client is no
    /// longer usable.
    #[error("agent '{agent}' is no longer running")]
    NotRunning {
        /// Agent name.
        agent: String,
    },
}

impl AcpClientError {
    /// Transient errors are recoverable by reconnecting; non-transient
    /// (`Spawn`, `Handshake`, `AgentError`) usually indicate
    /// misconfiguration or a protocol-level refusal and should bubble
    /// up.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            AcpClientError::Transport(_)
                | AcpClientError::NotRunning { .. }
                | AcpClientError::RequestTimeout { .. }
        )
    }
}

/// Outbound notification published on the bus by the core plugin. The
/// reader task forwards every agent-pushed notification into the
/// `events` channel; the core plugin pulls from there and republishes
/// on the kernel event bus as `com.nexus.acp.<method-with-dots>`.
#[derive(Debug, Clone)]
pub struct AgentNotification {
    /// JSON-RPC method (e.g. `"agent/output"`).
    pub method: String,
    /// Notification parameters as raw JSON.
    pub params: serde_json::Value,
}

/// Default per-request deadline. Agent requests can include LLM round
/// trips so the budget is generous compared to LSP's 10s.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Initialize must complete promptly; an agent that can't handshake
/// in 30s is probably wedged on auth.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

type PendingMap =
    Arc<Mutex<HashMap<i64, oneshot::Sender<Result<serde_json::Value, JsonRpcError>>>>>;

/// Live connection to one ACP agent.
pub struct AcpClient {
    agent_name: String,
    spec: AcpAdapterSpec,
    /// Process handle. `None` once shutdown completes.
    child: Option<Child>,
    /// Writer end of the child's stdin. Wrapped in Mutex so request /
    /// notification senders don't tear bytes mid-line.
    stdin: Arc<Mutex<ChildStdin>>,
    /// Reader task — reads child stdout and routes messages.
    _reader: JoinHandle<()>,
    /// Receives agent-pushed notifications.
    notifications: Arc<Mutex<mpsc::UnboundedReceiver<AgentNotification>>>,
    /// Pending request map.
    pending: PendingMap,
    /// Monotonic id for outbound requests.
    next_id: AtomicI64,
    /// Agent capabilities reported in the initialize response,
    /// surfaced through [`AcpClient::server_capabilities`]. `None`
    /// until handshake completes.
    server_capabilities: Mutex<Option<serde_json::Value>>,
}

impl AcpClient {
    /// Spawn the agent and run the ACP `initialize` handshake.
    ///
    /// # Errors
    /// - [`AcpClientError::Spawn`] when the executable fails to start.
    /// - [`AcpClientError::Handshake`] when initialize returns an
    ///   error, times out, or fails to parse.
    /// - [`AcpClientError::Transport`] for wire failures during init.
    ///
    /// # Panics
    /// Panics if the spawned `tokio::process::Child` doesn't expose
    /// stdin/stdout/stderr handles — both pipes are explicitly piped
    /// via `Stdio::piped()` above, so this is structurally
    /// unreachable.
    #[allow(clippy::too_many_lines)]
    pub async fn connect(
        agent_name: &str,
        spec: &AcpAdapterSpec,
        forge_root: PathBuf,
    ) -> Result<Self, AcpClientError> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .envs(&spec.env)
            .current_dir(&forge_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| AcpClientError::Spawn {
            command: spec.command.clone(),
            source: e,
        })?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Stderr drainer — surface diagnostic output through tracing
        // rather than blocking the agent's pipe.
        let label = agent_name.to_string();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(agent = %label, "stderr: {}", line);
            }
        });

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();
        let pending_for_reader = Arc::clone(&pending);
        let stdin_arc: Arc<Mutex<ChildStdin>> = Arc::new(Mutex::new(stdin));
        let stdin_for_reader = Arc::clone(&stdin_arc);
        let label = agent_name.to_string();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader).await {
                    Ok(JsonRpcMessage::Response(resp)) => {
                        let Some(id) = resp.id.as_i64() else {
                            tracing::warn!(
                                agent = %label,
                                "response id is not an integer; ignoring"
                            );
                            continue;
                        };
                        let mut map = pending_for_reader.lock().await;
                        if let Some(tx) = map.remove(&id) {
                            let outcome = if let Some(err) = resp.error {
                                Err(err)
                            } else {
                                Ok(resp.result.unwrap_or(serde_json::Value::Null))
                            };
                            let _ = tx.send(outcome);
                        } else {
                            tracing::warn!(
                                agent = %label,
                                id,
                                "response for unknown request id"
                            );
                        }
                    }
                    Ok(JsonRpcMessage::Notification(n)) => {
                        let _ = notif_tx.send(AgentNotification {
                            method: n.method,
                            params: n.params.unwrap_or(serde_json::Value::Null),
                        });
                    }
                    Ok(JsonRpcMessage::Request(req)) => {
                        // ACP agents don't issue server-initiated
                        // requests today, but if one shows up we ack
                        // with `method not found` so the agent doesn't
                        // hang. Mirrors LSP's defensive shape.
                        let resp = JsonRpcMessage::Response(JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id.clone(),
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32601,
                                message: format!(
                                    "host does not implement '{}'",
                                    req.method
                                ),
                                data: None,
                            }),
                        });
                        let mut stdin = stdin_for_reader.lock().await;
                        if let Err(e) = write_message(&mut *stdin, &resp).await {
                            tracing::warn!(
                                agent = %label,
                                method = %req.method,
                                error = %e,
                                "failed to reply to agent-initiated request"
                            );
                        }
                        drop(stdin);
                    }
                    Err(TransportError::Eof) => {
                        tracing::info!(
                            agent = %label,
                            "agent closed stdout — reader task exiting"
                        );
                        let mut map = pending_for_reader.lock().await;
                        for (_, tx) in map.drain() {
                            let _ = tx.send(Err(JsonRpcError {
                                code: -32000,
                                message: format!("agent '{label}' closed"),
                                data: None,
                            }));
                        }
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent = %label,
                            error = %e,
                            "transport error — reader task exiting"
                        );
                        let mut map = pending_for_reader.lock().await;
                        for (_, tx) in map.drain() {
                            let _ = tx.send(Err(JsonRpcError {
                                code: -32001,
                                message: format!("transport: {e}"),
                                data: None,
                            }));
                        }
                        break;
                    }
                }
            }
        });

        let client = Self {
            agent_name: agent_name.to_string(),
            spec: spec.clone(),
            child: Some(child),
            stdin: stdin_arc,
            _reader: reader,
            notifications: Arc::new(Mutex::new(notif_rx)),
            pending,
            next_id: AtomicI64::new(1),
            server_capabilities: Mutex::new(None),
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), AcpClientError> {
        let init_params = json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "nexus-acp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": self.spec.capabilities,
        });
        let response = match timeout(
            INITIALIZE_TIMEOUT,
            self.send_request("initialize", init_params),
        )
        .await
        {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(AcpClientError::Handshake {
                    agent: self.agent_name.clone(),
                    reason: e.to_string(),
                });
            }
            Err(_) => {
                return Err(AcpClientError::Handshake {
                    agent: self.agent_name.clone(),
                    reason: format!(
                        "initialize timed out after {}s",
                        INITIALIZE_TIMEOUT.as_secs()
                    ),
                });
            }
        };
        *self.server_capabilities.lock().await = Some(response.clone());
        tracing::info!(
            agent = %self.agent_name,
            "agent initialized: {}",
            response
                .get("agentInfo")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no agentInfo)")
        );
        Ok(())
    }

    /// Send a JSON-RPC request and await the response.
    ///
    /// # Errors
    /// - [`AcpClientError::Transport`] for wire failures.
    /// - [`AcpClientError::AgentError`] when the agent responds with a
    ///   JSON-RPC error.
    /// - [`AcpClientError::RequestTimeout`] if no response arrives
    ///   within [`DEFAULT_REQUEST_TIMEOUT`].
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AcpClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let req = JsonRpcMessage::Request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(params),
        });
        {
            let mut stdin = self.stdin.lock().await;
            if let Err(e) = write_message(&mut *stdin, &req).await {
                self.pending.lock().await.remove(&id);
                return Err(AcpClientError::Transport(e));
            }
        }
        let outcome = timeout(DEFAULT_REQUEST_TIMEOUT, rx).await;
        match outcome {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(err))) => Err(AcpClientError::AgentError {
                code: err.code,
                message: err.message,
            }),
            Ok(Err(_)) => Err(AcpClientError::NotRunning {
                agent: self.agent_name.clone(),
            }),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(AcpClientError::RequestTimeout {
                    method: method.to_string(),
                    ms: u64::try_from(DEFAULT_REQUEST_TIMEOUT.as_millis())
                        .unwrap_or(u64::MAX),
                })
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    ///
    /// # Errors
    /// - [`AcpClientError::Transport`] on wire failure.
    pub async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), AcpClientError> {
        let msg = JsonRpcMessage::Notification(JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
        });
        let mut stdin = self.stdin.lock().await;
        write_message(&mut *stdin, &msg).await?;
        Ok(())
    }

    /// Drain pending agent-pushed notifications. Returns immediately
    /// with everything queued; never blocks.
    pub async fn drain_notifications(&self) -> Vec<AgentNotification> {
        let mut rx = self.notifications.lock().await;
        let mut out = Vec::new();
        while let Ok(n) = rx.try_recv() {
            out.push(n);
        }
        out
    }

    /// Agent name from the spec.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Spec the client was constructed from.
    #[must_use]
    pub fn spec(&self) -> &AcpAdapterSpec {
        &self.spec
    }

    /// Capabilities the agent reported in the initialize response.
    /// `None` until the handshake completes.
    pub async fn server_capabilities(&self) -> Option<serde_json::Value> {
        self.server_capabilities.lock().await.clone()
    }

    /// `true` while the child process hasn't been reaped.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
    }

    /// Graceful shutdown. Closes stdin (so the reader task sees EOF
    /// and the agent typically exits) and waits up to 5s for the
    /// child; kills it otherwise.
    pub async fn shutdown(&mut self) {
        // Best-effort shutdown notification. Agents are free to ignore.
        let _ = self.send_notification("exit", json!(null)).await;
        if let Ok(mut stdin) = self.stdin.try_lock() {
            let _ = stdin.shutdown().await;
        }
        if let Some(mut child) = self.child.take() {
            match timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => tracing::info!(
                    agent = %self.agent_name,
                    code = ?status.code(),
                    "agent exited"
                ),
                Ok(Err(e)) => tracing::warn!(
                    agent = %self.agent_name,
                    error = %e,
                    "wait failed"
                ),
                Err(_) => {
                    tracing::warn!(
                        agent = %self.agent_name,
                        "agent did not exit in 5s — killing"
                    );
                    let _ = child.kill().await;
                }
            }
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        if self.child.is_some() {
            tracing::debug!(
                agent = %self.agent_name,
                "AcpClient dropped without explicit shutdown"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transient_classifies_correctly() {
        let timeout_err = AcpClientError::RequestTimeout {
            method: "x".into(),
            ms: 10,
        };
        assert!(timeout_err.is_transient());
        let spawn_err = AcpClientError::Spawn {
            command: "x".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no"),
        };
        assert!(!spawn_err.is_transient());
        let agent_err = AcpClientError::AgentError {
            code: -32601,
            message: "method not found".into(),
        };
        assert!(!agent_err.is_transient());
        let handshake_err = AcpClientError::Handshake {
            agent: "x".into(),
            reason: "y".into(),
        };
        assert!(!handshake_err.is_transient());
    }
}
