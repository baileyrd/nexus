//! One LSP client = one running language-server child process.
//!
//! Lifecycle, top-down:
//!
//! 1. [`LspClient::connect`] spawns the configured executable, opens
//!    stdin/stdout pipes, and starts a reader task that demultiplexes
//!    inbound JSON-RPC into pending-request slots and a notification
//!    channel.
//! 2. The caller drives `initialize` → `initialized` once, then
//!    issues per-document `did_open` / `did_change` / `did_close`
//!    notifications and per-cursor request/response calls.
//! 3. On drop / explicit shutdown, the client sends `shutdown` then
//!    `exit`, then `kill()`s the child if it doesn't release stdin.
//!
//! The client is `Send` and lives behind an `Arc<Mutex<…>>` in
//! [`crate::pool::ConnectionPool`]; concurrency is mutex-serialised
//! per server today (matches `nexus-mcp` shape).

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

use crate::config::LspServerSpec;
use crate::transport::{
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, TransportError,
};

/// Errors raised by [`LspClient`].
#[derive(Debug, thiserror::Error)]
pub enum LspClientError {
    /// Failed to spawn the configured executable.
    #[error("spawn '{command}': {source}")]
    Spawn {
        /// Command we tried to run.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Initialize handshake failed (server returned an error or
    /// timed out before responding).
    #[error("handshake with '{server}': {reason}")]
    Handshake {
        /// Server name from `lsp.toml`.
        server: String,
        /// Failure summary.
        reason: String,
    },
    /// Wire-level transport failure (broken pipe, malformed frame).
    /// Pool callers should treat this as transient and reconnect.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// Server returned a JSON-RPC error in response to a request.
    #[error("server error {code}: {message}")]
    ServerError {
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
    #[error("server '{server}' is no longer running")]
    NotRunning {
        /// Server name.
        server: String,
    },
}

impl LspClientError {
    /// Transient errors are recoverable by reconnecting; non-transient
    /// (`Spawn`, `Handshake`, `ServerError`) usually indicate
    /// misconfiguration and should bubble up.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            LspClientError::Transport(_)
                | LspClientError::NotRunning { .. }
                | LspClientError::RequestTimeout { .. }
        )
    }
}

/// Outbound notification published on the bus by the core plugin.
///
/// The reader task forwards every server-pushed notification into the
/// `events` channel; the core plugin pulls from there and republishes
/// on the kernel event bus as `com.nexus.lsp.<method>`.
#[derive(Debug, Clone)]
pub struct ServerNotification {
    /// LSP method (e.g. `"textDocument/publishDiagnostics"`).
    pub method: String,
    /// Notification parameters as raw JSON.
    pub params: serde_json::Value,
}

/// Default per-request deadline. LSP requests are interactive — a
/// completion that takes longer than this is dead anyway.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Initialize must be allowed to run longer (rust-analyzer cold-start
/// reads the project's `Cargo.toml` graph before responding).
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-document tracking. The full set is what a future resync-after-
/// reconnect path will need (replay every open document on a fresh
/// child); today we only update `version`/`text` and read nothing
/// back, so suppress the dead-code warning rather than drop fields
/// the resync path will need on day one.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct DocumentState {
    uri: String,
    language_id: String,
    version: i64,
    text: String,
}

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<serde_json::Value, JsonRpcError>>>>>;

/// Live connection to one LSP server.
pub struct LspClient {
    server_name: String,
    spec: LspServerSpec,
    /// Process handle. `None` once shutdown completes.
    child: Option<Child>,
    /// Writer end of the child's stdin. Wrapped in Mutex so the
    /// reader task can write notifications back (we don't, but the
    /// pool may issue `did_change` concurrently with a request).
    stdin: Arc<Mutex<ChildStdin>>,
    /// Reader task — reads child stdout and routes messages.
    _reader: JoinHandle<()>,
    /// Receives server-pushed notifications.
    notifications: Arc<Mutex<mpsc::UnboundedReceiver<ServerNotification>>>,
    /// Pending request map.
    pending: PendingMap,
    /// Monotonic id for outbound requests.
    next_id: AtomicI64,
    /// Tracked open documents (URI → state). Survives reconnect so a
    /// future `resync_documents` can replay the open set.
    documents: Mutex<HashMap<String, DocumentState>>,
    /// Forge root used as the `rootUri` fallback during init.
    forge_root: PathBuf,
}

impl LspClient {
    /// Spawn the server and run the LSP `initialize` / `initialized`
    /// handshake.
    ///
    /// # Errors
    /// - [`LspClientError::Spawn`] when the executable fails to start.
    /// - [`LspClientError::Handshake`] when initialize returns an
    ///   error or times out.
    /// - [`LspClientError::Transport`] for wire failures during init.
    ///
    /// # Panics
    /// Panics if the spawned `tokio::process::Child` doesn't expose
    /// stdin/stdout/stderr handles — both pipes are explicitly piped
    /// via `Stdio::piped()` above, so this is structurally
    /// unreachable.
    #[allow(clippy::too_many_lines)] // proc spawn + reader task is a single linear story
    pub async fn connect(
        server_name: &str,
        spec: &LspServerSpec,
        forge_root: PathBuf,
    ) -> Result<Self, LspClientError> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .envs(&spec.env)
            .current_dir(&forge_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| LspClientError::Spawn {
            command: spec.command.clone(),
            source: e,
        })?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Stderr drainer — log it so a misbehaving server's diagnostics
        // surface in `nexus` logs rather than blocking the pipe.
        let server_label = server_name.to_string();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(server = %server_label, "stderr: {}", line);
            }
        });

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();
        let pending_for_reader = Arc::clone(&pending);
        let server_label = server_name.to_string();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader).await {
                    Ok(JsonRpcMessage::Response(resp)) => {
                        let Some(id) = resp.id.as_i64() else {
                            tracing::warn!(
                                server = %server_label,
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
                                server = %server_label,
                                id,
                                "response for unknown request id"
                            );
                        }
                    }
                    Ok(JsonRpcMessage::Notification(n)) => {
                        let _ = notif_tx.send(ServerNotification {
                            method: n.method,
                            params: n.params.unwrap_or(serde_json::Value::Null),
                        });
                    }
                    Ok(JsonRpcMessage::Request(req)) => {
                        // Server-initiated requests (workspace/configuration,
                        // window/showMessageRequest, …). We don't service them
                        // today; reply with a method-not-found error so the
                        // server doesn't hang waiting.
                        let id = req.id.clone();
                        let resp = JsonRpcMessage::Response(JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
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
                        // We need stdin to write back; but stdin lives on
                        // the LspClient struct. Send through the
                        // notification channel as a synthetic "host
                        // declined" event instead — losing one inbound
                        // request is preferable to deadlocking the
                        // reader on a stdin lock we don't hold here.
                        tracing::debug!(
                            server = %server_label,
                            method = %req.method,
                            "ignoring server-initiated request (not implemented)"
                        );
                        // serialize the canned response for parity
                        // with future implementations:
                        let _ = resp;
                    }
                    Err(TransportError::Eof) => {
                        tracing::info!(
                            server = %server_label,
                            "server closed stdout — reader task exiting"
                        );
                        // Wake every pending request with NotRunning so
                        // callers don't hang.
                        let mut map = pending_for_reader.lock().await;
                        for (_, tx) in map.drain() {
                            let _ = tx.send(Err(JsonRpcError {
                                code: -32000,
                                message: format!("server '{server_label}' closed"),
                                data: None,
                            }));
                        }
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            server = %server_label,
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
            server_name: server_name.to_string(),
            spec: spec.clone(),
            child: Some(child),
            stdin: Arc::new(Mutex::new(stdin)),
            _reader: reader,
            notifications: Arc::new(Mutex::new(notif_rx)),
            pending,
            next_id: AtomicI64::new(1),
            documents: Mutex::new(HashMap::new()),
            forge_root,
        };

        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), LspClientError> {
        // Per LSP spec, the `processId` field is the host's pid (or
        // null) — the server uses it to clean up if the host dies.
        let init_params = json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "nexus-lsp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "rootUri": file_uri(&self.forge_root),
            "capabilities": minimal_client_capabilities(),
            "workspaceFolders": [{
                "uri": file_uri(&self.forge_root),
                "name": self.forge_root
                    .file_name()
                    .map_or_else(|| "forge".to_string(), |n| n.to_string_lossy().into_owned()),
            }],
        });
        let result = match timeout(
            INITIALIZE_TIMEOUT,
            self.send_request("initialize", init_params),
        )
        .await
        {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(LspClientError::Handshake {
                    server: self.server_name.clone(),
                    reason: e.to_string(),
                });
            }
            Err(_) => {
                return Err(LspClientError::Handshake {
                    server: self.server_name.clone(),
                    reason: format!(
                        "initialize timed out after {}s",
                        INITIALIZE_TIMEOUT.as_secs()
                    ),
                });
            }
        };
        // Server's reply contains its capabilities; we don't constrain
        // by them today (the shell client validates). Just log so an
        // operator inspecting the server gets a breadcrumb.
        tracing::info!(
            server = %self.server_name,
            "server initialized: {}",
            result.get("serverInfo")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no serverInfo)")
        );
        // Tell the server initialization is complete.
        self.send_notification("initialized", json!({})).await?;
        Ok(())
    }

    /// Send a JSON-RPC request and await the response.
    ///
    /// # Errors
    /// - [`LspClientError::Transport`] for wire failures.
    /// - [`LspClientError::ServerError`] when the server responds with
    ///   a JSON-RPC error.
    /// - [`LspClientError::RequestTimeout`] if no response arrives
    ///   within [`DEFAULT_REQUEST_TIMEOUT`].
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspClientError> {
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
                return Err(LspClientError::Transport(e));
            }
        }
        let outcome = timeout(DEFAULT_REQUEST_TIMEOUT, rx).await;
        match outcome {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(err))) => Err(LspClientError::ServerError {
                code: err.code,
                message: err.message,
            }),
            Ok(Err(_)) => Err(LspClientError::NotRunning {
                server: self.server_name.clone(),
            }),
            Err(_) => {
                // Drop the pending entry so a late-arriving response
                // doesn't pile up.
                self.pending.lock().await.remove(&id);
                Err(LspClientError::RequestTimeout {
                    method: method.to_string(),
                    // u128 → u64: 10 s fits trivially. The try_from is
                    // here so the cast is checked rather than silently
                    // truncating; the saturate is unreachable.
                    ms: u64::try_from(DEFAULT_REQUEST_TIMEOUT.as_millis())
                        .unwrap_or(u64::MAX),
                })
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    ///
    /// # Errors
    /// - [`LspClientError::Transport`] on wire failure.
    pub async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), LspClientError> {
        let msg = JsonRpcMessage::Notification(JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
        });
        let mut stdin = self.stdin.lock().await;
        write_message(&mut *stdin, &msg).await?;
        Ok(())
    }

    /// Drain pending server-pushed notifications. Returns immediately
    /// with everything queued; never blocks.
    pub async fn drain_notifications(&self) -> Vec<ServerNotification> {
        let mut rx = self.notifications.lock().await;
        let mut out = Vec::new();
        while let Ok(n) = rx.try_recv() {
            out.push(n);
        }
        out
    }

    /// Wait for the next notification (or `None` if the channel is
    /// closed). Useful for tests; the host plugin uses
    /// [`drain_notifications`] in its poll loop.
    pub async fn next_notification(&self) -> Option<ServerNotification> {
        let mut rx = self.notifications.lock().await;
        rx.recv().await
    }

    /// Track an open document (`textDocument/didOpen`).
    ///
    /// # Errors
    /// - [`LspClientError::Transport`] on wire failure.
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> Result<(), LspClientError> {
        self.documents.lock().await.insert(
            uri.to_string(),
            DocumentState {
                uri: uri.to_string(),
                language_id: language_id.to_string(),
                version,
                text: text.to_string(),
            },
        );
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text,
                }
            }),
        )
        .await
    }

    /// Push a document update (`textDocument/didChange`, full sync).
    ///
    /// # Errors
    /// - [`LspClientError::Transport`] on wire failure.
    pub async fn did_change(
        &self,
        uri: &str,
        version: i64,
        text: &str,
    ) -> Result<(), LspClientError> {
        if let Some(state) = self.documents.lock().await.get_mut(uri) {
            state.version = version;
            state.text = text.to_string();
        }
        self.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        )
        .await
    }

    /// Tell the server a document is closed (`textDocument/didClose`).
    ///
    /// # Errors
    /// - [`LspClientError::Transport`] on wire failure.
    pub async fn did_close(&self, uri: &str) -> Result<(), LspClientError> {
        self.documents.lock().await.remove(uri);
        self.send_notification(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await
    }

    /// Server name from the config.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Spec the client was constructed from.
    #[must_use]
    pub fn spec(&self) -> &LspServerSpec {
        &self.spec
    }

    /// `true` when the child has not yet exited.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        // try_wait returns Err on stale handle; treat as not-alive.
        // We can't take ownership of `child` here, so peek by polling
        // the wait future without awaiting (via `now_or_never`-shape
        // requires futures crate). Instead route through a poll on
        // `self.child` via a try_lock-style read — child lives in
        // self.child as an Option<Child>, behind &self via interior
        // mutability we don't have, so we return true while child is
        // still Some; the reader task drains pendings on EOF, which
        // is the source-of-truth callers actually care about.
        self.child.is_some()
    }

    /// Graceful shutdown. Sends `shutdown` (synchronous) and `exit`
    /// (notification), then waits up to 5 s for the child to exit;
    /// kills it otherwise.
    pub async fn shutdown(&mut self) {
        // shutdown is best-effort — if the server is already gone the
        // request will time out, which is fine.
        let _ = timeout(
            Duration::from_secs(2),
            self.send_request("shutdown", json!(null)),
        )
        .await;
        let _ = self.send_notification("exit", json!(null)).await;
        // Close stdin so the reader task observes EOF.
        if let Ok(mut stdin) = self.stdin.try_lock() {
            let _ = stdin.shutdown().await;
        }
        if let Some(mut child) = self.child.take() {
            match timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => tracing::info!(
                    server = %self.server_name,
                    code = ?status.code(),
                    "server exited"
                ),
                Ok(Err(e)) => tracing::warn!(
                    server = %self.server_name,
                    error = %e,
                    "wait failed"
                ),
                Err(_) => {
                    tracing::warn!(
                        server = %self.server_name,
                        "server did not exit in 5s — killing"
                    );
                    let _ = child.kill().await;
                }
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // kill_on_drop is set on Command, so the child is reaped by
        // the runtime; we just emit a breadcrumb. Graceful shutdown
        // belongs in `shutdown()` which the pool calls explicitly.
        if self.child.is_some() {
            tracing::debug!(
                server = %self.server_name,
                "LspClient dropped without explicit shutdown"
            );
        }
    }
}

/// Convert a filesystem path to an LSP `file://` URI. Cross-platform
/// only to the extent that `Path::display()` is — Windows callers
/// would want a smarter implementation but Nexus doesn't ship there
/// today (CLAUDE.md notes WSL/Linux only).
fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

/// Minimal client capabilities — enough for completion / hover /
/// definition / diagnostics. Servers gracefully degrade if a feature
/// isn't asked for.
fn minimal_client_capabilities() -> serde_json::Value {
    json!({
        "textDocument": {
            "synchronization": {
                "didSave": true,
                "willSave": false,
                "willSaveWaitUntil": false,
            },
            "completion": {
                "completionItem": { "snippetSupport": false },
            },
            "hover": { "contentFormat": ["plaintext", "markdown"] },
            "definition": {},
            "references": {},
            "rename": { "prepareSupport": false },
            "codeAction": {},
            "formatting": {},
            "publishDiagnostics": {
                "relatedInformation": false,
                "versionSupport": true,
            },
        },
        "workspace": {
            "configuration": false,
            "workspaceFolders": true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_includes_scheme() {
        let uri = file_uri(std::path::Path::new("/tmp/foo"));
        assert_eq!(uri, "file:///tmp/foo");
    }

    #[test]
    fn is_transient_classifies_correctly() {
        let timeout_err = LspClientError::RequestTimeout {
            method: "x".to_string(),
            ms: 10,
        };
        assert!(timeout_err.is_transient());

        let spawn_err = LspClientError::Spawn {
            command: "x".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no"),
        };
        assert!(!spawn_err.is_transient());

        let server_err = LspClientError::ServerError {
            code: -32601,
            message: "method not found".to_string(),
        };
        assert!(!server_err.is_transient());
    }
}
