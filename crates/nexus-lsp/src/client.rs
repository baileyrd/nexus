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
    read_message, write_message, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, TransportError,
};

/// Bound on the server-pushed notification channel. A chatty server
/// (rust-analyzer's `$/progress` storm, eslint diagnostics flood) can
/// outpace the consumer; rather than grow without limit, we drop the
/// excess and log a single latched warn per saturation episode so the
/// operator notices but the log isn't flooded.
const LSP_NOTIF_CHANNEL_BOUND: usize = 1024;

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

/// Snapshot of one open document, exported for the
/// `ConnectionPool`'s resync path. Mirrors the private
/// [`DocumentState`] but lives in the public surface so the pool
/// can carry replay state across a reconnect without holding a
/// reference to the dropped client.
#[derive(Debug, Clone)]
pub struct OpenDocument {
    pub uri: String,
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

type PendingMap =
    Arc<Mutex<HashMap<i64, oneshot::Sender<Result<serde_json::Value, JsonRpcError>>>>>;

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
    /// Receives server-pushed notifications. Bounded; see
    /// [`LSP_NOTIF_CHANNEL_BOUND`].
    notifications: Arc<Mutex<mpsc::Receiver<ServerNotification>>>,
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
        let (notif_tx, notif_rx) = mpsc::channel(LSP_NOTIF_CHANNEL_BOUND);
        let pending_for_reader = Arc::clone(&pending);
        // Wrap stdin in the shared `Arc<Mutex<...>>` BEFORE spawning the
        // reader so the reader can write responses back for
        // server-initiated requests (BL-076 follow-up). Pre-fix the
        // reader had no access to stdin and the canned method-not-found
        // response was discarded — servers that rely on
        // `workspace/configuration` (rust-analyzer's settings round-
        // trip) hung waiting.
        let stdin_arc: Arc<Mutex<ChildStdin>> = Arc::new(Mutex::new(stdin));
        let stdin_for_reader = Arc::clone(&stdin_arc);
        let server_label = server_name.to_string();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            // Latched flag — one warn per saturation episode. Resets
            // when a send succeeds, so a slow consumer that catches up
            // and falls behind again gets logged a second time.
            let mut notif_dropped_warned = false;
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
                        let method = n.method.clone();
                        let notif = ServerNotification {
                            method: n.method,
                            params: n.params.unwrap_or(serde_json::Value::Null),
                        };
                        // try_send so a stalled consumer can't block
                        // the reader (which also delivers responses
                        // via the pending map — blocking here would
                        // wedge the whole client).
                        match notif_tx.try_send(notif) {
                            Ok(()) => {
                                notif_dropped_warned = false;
                            }
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                if !notif_dropped_warned {
                                    tracing::warn!(
                                        server = %server_label,
                                        method = %method,
                                        bound = LSP_NOTIF_CHANNEL_BOUND,
                                        "LSP notification channel full; \
                                         consumer is falling behind. \
                                         Dropping notifications until it drains."
                                    );
                                    notif_dropped_warned = true;
                                }
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                // Client dropped its receiver; reader
                                // will exit on next EOF anyway.
                            }
                        }
                    }
                    Ok(JsonRpcMessage::Request(req)) => {
                        // BL-076 — server-initiated requests
                        // (workspace/configuration, window/
                        // showMessageRequest, client/registerCapability,
                        // …). The host returns a spec-compliant
                        // result for the well-known methods so servers
                        // don't hang; unknown methods get a
                        // method-not-found error so the server can
                        // adapt its capability set.
                        let resp_msg = build_server_request_reply(
                            &req.method,
                            req.params.as_ref(),
                            req.id.clone(),
                        );
                        // Acquire stdin briefly to write the response.
                        // Must NOT hold across await points other than
                        // the write itself — the same mutex is used
                        // by `send_request` / `send_notification` on
                        // outbound traffic, and reader-side stalls
                        // would back-pressure the whole client.
                        let mut stdin = stdin_for_reader.lock().await;
                        if let Err(err) = write_message(&mut *stdin, &resp_msg).await {
                            tracing::warn!(
                                server = %server_label,
                                method = %req.method,
                                error = %err,
                                "failed to reply to server-initiated request"
                            );
                        } else {
                            tracing::debug!(
                                server = %server_label,
                                method = %req.method,
                                "replied to server-initiated request"
                            );
                        }
                        drop(stdin);
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
            stdin: stdin_arc,
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
                    ms: u64::try_from(DEFAULT_REQUEST_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
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

    /// Snapshot every open document this client is tracking. Used
    /// by [`crate::pool::ConnectionPool`] to replay `didOpen`
    /// against a fresh connection after a transient failure
    /// triggers a reconnect — without this the new server starts
    /// with an empty document set and stale-diagnostic / no-completions
    /// behaviour persists until the user re-opens each tab.
    pub async fn documents_snapshot(&self) -> Vec<OpenDocument> {
        self.documents
            .lock()
            .await
            .values()
            .map(|d| OpenDocument {
                uri: d.uri.clone(),
                language_id: d.language_id.clone(),
                version: d.version,
                text: d.text.clone(),
            })
            .collect()
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
            // BL-076 — host now answers `workspace/configuration`
            // server-initiated requests (with array of nulls = use
            // defaults). Advertising support unblocks rust-analyzer's
            // settings round-trip and lets servers gate behavior on
            // the response rather than skipping the call.
            "configuration": true,
            "workspaceFolders": true,
            // Dynamic capability registration — host accepts
            // (un)register requests with a null result so servers
            // can finish their startup sequence.
            "didChangeConfiguration": { "dynamicRegistration": true },
        },
        "window": {
            // Host returns null (cancellation) for showMessageRequest
            // — there's no dialog UI today, but advertising support
            // means servers route through this rather than alternative
            // surfaces (e.g., raw publishDiagnostics with action hints).
            "showMessage": { "messageActionItem": { "additionalPropertiesSupport": false } },
            "workDoneProgress": true,
        },
    })
}

/// BL-076 — build a JSON-RPC reply for a server-initiated request.
///
/// The host doesn't actually drive client-side configuration or
/// dialog UI today, but most servers can adapt to no-op replies as
/// long as we acknowledge the request. The dispatch table below
/// covers every method the major LSP servers (rust-analyzer,
/// typescript-language-server, gopls) issue at boot:
///
/// | Method                              | Reply                                                  |
/// |-------------------------------------|--------------------------------------------------------|
/// | `workspace/configuration`           | `[null, null, …]` — one null per requested item        |
/// | `workspace/workspaceFolders`        | `null` — single-root workspace; spec-compliant         |
/// | `window/showMessageRequest`         | `null` — user-canceled (no action selected)            |
/// | `window/showDocument`               | `{ "success": false }` — host can't open arbitrary URIs|
/// | `window/workDoneProgress/create`    | `null` — accept the token, no UI surface                |
/// | `client/registerCapability`         | `null` — accept; host doesn't track dynamic registry   |
/// | `client/unregisterCapability`       | `null` — accept                                         |
/// | `workspace/applyEdit`               | `{ "applied": false }` — host doesn't apply edits      |
/// | `workspace/codeLens/refresh`        | `null` — accept; refresh is a no-op without code-lens  |
/// | `workspace/diagnostic/refresh`      | `null` — accept                                        |
/// | `workspace/inlayHint/refresh`       | `null` — accept                                        |
/// | `workspace/semanticTokens/refresh`  | `null` — accept                                        |
/// | other                                | error `-32601 method not found`                         |
///
/// Pure function — extracted so the dispatch table is unit-testable
/// without spawning a server. The `id` is threaded through verbatim.
fn build_server_request_reply(
    method: &str,
    params: Option<&serde_json::Value>,
    id: serde_json::Value,
) -> JsonRpcMessage {
    if let Some(result) = build_known_reply(method, params) {
        return JsonRpcMessage::Response(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        });
    }
    JsonRpcMessage::Response(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code: -32601,
            message: format!("host does not implement '{method}'"),
            data: None,
        }),
    })
}

/// Inner table — returns `Some(result)` for methods we know how to
/// answer with a no-op-shaped value, `None` to signal "fall through
/// to method-not-found error".
fn build_known_reply(
    method: &str,
    params: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    match method {
        "workspace/configuration" => {
            // Spec: response is `LSPAny[]` with one entry per
            // requested item, in the order the items appeared. The
            // host has no per-section overrides, so the spec-compliant
            // "use defaults" reply is an array of nulls.
            let count = params
                .and_then(|p| p.get("items"))
                .and_then(|v| v.as_array())
                .map_or(0, Vec::len);
            // Match the count exactly. A zero-length items array
            // (which some servers emit defensively) yields an empty
            // array; rust-analyzer treats `null` as a hard failure.
            Some(serde_json::Value::Array(vec![
                serde_json::Value::Null;
                count
            ]))
        }
        // Single-root workspace — null is the spec-compliant "no
        // additional folders" reply.
        "workspace/workspaceFolders" => Some(serde_json::Value::Null),
        // No dialog UI in the host. The user can't pick an action,
        // so report cancellation.
        "window/showMessageRequest" => Some(serde_json::Value::Null),
        // Host can't open arbitrary URIs from a server's request.
        // `{ success: false }` lets the server fall back gracefully
        // (e.g., log a hint instead of waiting on a viewer to focus).
        "window/showDocument" => Some(serde_json::json!({ "success": false })),
        // Progress token registration — accept with null. We don't
        // surface progress UI but the server expects acknowledgement
        // before it starts publishing `$/progress` notifications.
        "window/workDoneProgress/create" => Some(serde_json::Value::Null),
        // Dynamic capability (un)registration — accept with null.
        // The host doesn't track the registry but accepting unblocks
        // servers that gate behavior on a successful registration
        // (rust-analyzer, vscode-html-languageserver).
        "client/registerCapability" | "client/unregisterCapability" => {
            Some(serde_json::Value::Null)
        }
        // Servers occasionally try to drive workspace edits. The host
        // can't apply them without going through the editor IPC layer
        // (and the BL-077 WorkspaceEdit applier already covers the
        // shell-driven path), so report not-applied and let the
        // server retry through user-initiated commands.
        "workspace/applyEdit" => Some(serde_json::json!({ "applied": false })),
        // Refresh requests — accept with null. The host doesn't
        // cache anything that needs invalidation; the server's
        // expectation is "I will resend on next pull."
        "workspace/codeLens/refresh"
        | "workspace/diagnostic/refresh"
        | "workspace/inlayHint/refresh"
        | "workspace/semanticTokens/refresh"
        | "workspace/foldingRange/refresh" => Some(serde_json::Value::Null),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_includes_scheme() {
        let uri = file_uri(std::path::Path::new("/tmp/foo"));
        assert_eq!(uri, "file:///tmp/foo");
    }

    // ── BL-076 server-initiated request reply table ────────────────

    /// Helper for the dispatch-table tests: build the reply, assert
    /// the wire shape, and pull out the JSON-RPC `result` (or panic
    /// on error). Centralised so each method's test stays focused on
    /// the result payload, not the envelope plumbing.
    fn reply_result_for(method: &str, params: serde_json::Value) -> serde_json::Value {
        let msg = build_server_request_reply(method, Some(&params), serde_json::json!(42));
        let JsonRpcMessage::Response(resp) = msg else {
            panic!("expected Response, got {msg:?}")
        };
        assert_eq!(
            resp.id,
            serde_json::json!(42),
            "id must round-trip verbatim"
        );
        assert!(
            resp.error.is_none(),
            "expected success result, got error: {:?}",
            resp.error
        );
        resp.result.expect("success result")
    }

    fn reply_error_for(method: &str) -> JsonRpcError {
        let msg = build_server_request_reply(method, None, serde_json::json!(7));
        let JsonRpcMessage::Response(resp) = msg else {
            panic!("expected Response")
        };
        resp.error.expect("expected error")
    }

    #[test]
    fn workspace_configuration_returns_array_of_nulls_matching_item_count() {
        // Three items requested → three nulls. Matching the count is
        // a hard contract — rust-analyzer treats a length mismatch
        // as a fatal protocol error and disconnects.
        let result = reply_result_for(
            "workspace/configuration",
            serde_json::json!({
                "items": [
                    { "scopeUri": "file:///a", "section": "rust-analyzer.cargo" },
                    { "scopeUri": "file:///a", "section": "rust-analyzer.checkOnSave" },
                    { "section": "editor" }
                ]
            }),
        );
        assert_eq!(result, serde_json::json!([null, null, null]));
    }

    #[test]
    fn workspace_configuration_handles_empty_items_array() {
        let result = reply_result_for(
            "workspace/configuration",
            serde_json::json!({ "items": [] }),
        );
        assert_eq!(result, serde_json::json!([]));
    }

    #[test]
    fn workspace_configuration_handles_missing_items_field() {
        // Defensive — a malformed request gets back a zero-length
        // array rather than a server-killing protocol error.
        let result = reply_result_for("workspace/configuration", serde_json::json!({}));
        assert_eq!(result, serde_json::json!([]));
    }

    #[test]
    fn workspace_workspace_folders_returns_null() {
        let result = reply_result_for("workspace/workspaceFolders", serde_json::json!({}));
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn show_message_request_returns_null_canceled() {
        let result = reply_result_for(
            "window/showMessageRequest",
            serde_json::json!({
                "type": 1,
                "message": "do you want to enable foo?",
                "actions": [{ "title": "Yes" }, { "title": "No" }]
            }),
        );
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn show_document_returns_success_false() {
        let result = reply_result_for(
            "window/showDocument",
            serde_json::json!({ "uri": "file:///x.rs", "external": false }),
        );
        assert_eq!(result, serde_json::json!({ "success": false }));
    }

    #[test]
    fn work_done_progress_create_returns_null() {
        let result = reply_result_for(
            "window/workDoneProgress/create",
            serde_json::json!({ "token": "rustAnalyzer/Indexing" }),
        );
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn register_and_unregister_capability_return_null() {
        let result = reply_result_for(
            "client/registerCapability",
            serde_json::json!({
                "registrations": [{
                    "id": "rust-analyzer-textDocument-completion",
                    "method": "textDocument/completion",
                }]
            }),
        );
        assert_eq!(result, serde_json::Value::Null);
        let result = reply_result_for(
            "client/unregisterCapability",
            serde_json::json!({ "unregisterations": [] }),
        );
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn workspace_apply_edit_reports_not_applied() {
        let result = reply_result_for(
            "workspace/applyEdit",
            serde_json::json!({
                "edit": { "changes": {} }
            }),
        );
        assert_eq!(result, serde_json::json!({ "applied": false }));
    }

    #[test]
    fn refresh_requests_return_null() {
        for method in [
            "workspace/codeLens/refresh",
            "workspace/diagnostic/refresh",
            "workspace/inlayHint/refresh",
            "workspace/semanticTokens/refresh",
            "workspace/foldingRange/refresh",
        ] {
            let result = reply_result_for(method, serde_json::json!({}));
            assert_eq!(
                result,
                serde_json::Value::Null,
                "refresh method '{method}' must return null",
            );
        }
    }

    #[test]
    fn unknown_method_returns_method_not_found_error() {
        // Anything we don't recognise gets the spec-compliant
        // -32601 error. Servers MAY adapt their capability set
        // based on this; they MUST NOT hang waiting.
        let err = reply_error_for("totally/made/up");
        assert_eq!(err.code, -32601);
        assert!(
            err.message.contains("totally/made/up"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn build_known_reply_returns_none_for_unknown_method() {
        // Direct test of the inner table — used by the fall-through
        // arm in `build_server_request_reply`.
        assert!(build_known_reply("totally/made/up", None).is_none());
    }

    #[test]
    fn id_is_preserved_for_string_and_integer_forms() {
        // JSON-RPC ids can be integers, strings, or null. Servers may
        // emit any of these for their own requests; the reply must
        // echo whatever we received.
        let int_id = serde_json::json!(99);
        let str_id = serde_json::json!("rustAnalyzer-5");

        let msg = build_server_request_reply(
            "workspace/configuration",
            Some(&serde_json::json!({ "items": [] })),
            int_id.clone(),
        );
        if let JsonRpcMessage::Response(r) = msg {
            assert_eq!(r.id, int_id);
        } else {
            panic!("expected Response");
        }

        let msg = build_server_request_reply(
            "client/registerCapability",
            Some(&serde_json::json!({})),
            str_id.clone(),
        );
        if let JsonRpcMessage::Response(r) = msg {
            assert_eq!(r.id, str_id);
        } else {
            panic!("expected Response");
        }
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
