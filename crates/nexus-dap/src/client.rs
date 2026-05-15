//! One DAP client = one running debug-adapter child process.
//!
//! Lifecycle, top-down:
//!
//! 1. [`DapClient::connect`] spawns the configured executable, opens
//!    stdin/stdout pipes, and starts a reader task that demultiplexes
//!    inbound messages into pending-request slots and an event channel.
//! 2. The caller drives `initialize` to negotiate capabilities, then
//!    `launch` / `attach`, per-source `setBreakpoints`,
//!    `configurationDone`, and execution control verbs.
//! 3. On drop / explicit shutdown, the client sends `disconnect` and
//!    `kill()`s the child if it doesn't release stdin.
//!
//! Concurrency: the client is `Send + Sync` and lives behind an
//! `Arc<Mutex<…>>` in [`crate::pool::ConnectionPool`]. Outbound
//! traffic serialises on a stdin mutex; inbound traffic is owned by
//! the reader task. Pending requests use oneshot channels, so a
//! request and an event can be in flight concurrently without
//! contention.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::config::DapAdapterSpec;
use crate::protocol::{ProtocolMessage, ProtocolRequest, ProtocolResponse};
use crate::transport::{read_message, write_message, TransportError};

/// Errors raised by [`DapClient`].
#[derive(Debug, thiserror::Error)]
pub enum DapClientError {
    /// Failed to spawn the configured executable.
    #[error("spawn '{command}': {source}")]
    Spawn {
        /// Command we tried to run.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Initialize handshake failed (adapter returned `success: false`
    /// or timed out).
    #[error("handshake with '{adapter}': {reason}")]
    Handshake {
        /// Adapter name from `dap.toml`.
        adapter: String,
        /// Failure summary.
        reason: String,
    },
    /// Wire-level transport failure. Pool callers should treat as
    /// transient and reconnect.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    /// Adapter returned `success: false` for a request.
    #[error("adapter error for '{command}': {message}")]
    AdapterError {
        /// Command that failed.
        command: String,
        /// `message` field from the response.
        message: String,
    },
    /// Request did not get a response within the deadline.
    #[error("request '{command}' timed out after {ms}ms")]
    RequestTimeout {
        /// Command that timed out.
        command: String,
        /// Configured deadline in milliseconds.
        ms: u64,
    },
    /// Reader task exited or the child closed the pipe; client is no
    /// longer usable.
    #[error("adapter '{adapter}' is no longer running")]
    NotRunning {
        /// Adapter name.
        adapter: String,
    },
}

impl DapClientError {
    /// Transient errors are recoverable by reconnecting; non-transient
    /// (`Spawn`, `Handshake`, `AdapterError`) typically indicate
    /// misconfiguration and should bubble up.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            DapClientError::Transport(_)
                | DapClientError::NotRunning { .. }
                | DapClientError::RequestTimeout { .. }
        )
    }
}

/// Outbound event published on the bus by the core plugin.
#[derive(Debug, Clone)]
pub struct AdapterEvent {
    /// DAP event name (`"stopped"`, `"output"`, `"terminated"`, …).
    pub event: String,
    /// Event payload as raw JSON.
    pub body: serde_json::Value,
}

/// Capabilities returned by the adapter's `initialize` response.
///
/// We only typify the booleans that affect host behaviour today;
/// everything else stays on `raw` for callers that want the full
/// surface. Five booleans is intentional — they're independent flags
/// the DAP spec defines individually, not a state machine that
/// should be modelled as an enum.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct AdapterCapabilities {
    /// `true` if the adapter supports `configurationDone`.
    pub supports_configuration_done: bool,
    /// `true` if the adapter supports `function` breakpoints.
    pub supports_function_breakpoints: bool,
    /// `true` if the adapter supports `conditional` breakpoints.
    pub supports_conditional_breakpoints: bool,
    /// `true` if the adapter supports the `terminate` request.
    pub supports_terminate_request: bool,
    /// `true` if the adapter supports the `setExceptionBreakpoints`
    /// request.
    pub supports_exception_options: bool,
    /// Full unmodified capability payload from the adapter.
    pub raw: serde_json::Value,
}

impl AdapterCapabilities {
    fn from_response(body: Option<&serde_json::Value>) -> Self {
        let Some(body) = body else {
            return Self::default();
        };
        Self {
            supports_configuration_done: body
                .get("supportsConfigurationDoneRequest")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            supports_function_breakpoints: body
                .get("supportsFunctionBreakpoints")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            supports_conditional_breakpoints: body
                .get("supportsConditionalBreakpoints")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            supports_terminate_request: body
                .get("supportsTerminateRequest")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            supports_exception_options: body
                .get("supportsExceptionOptions")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            raw: body.clone(),
        }
    }
}

/// One source breakpoint as cached for resync.
#[derive(Debug, Clone)]
pub struct SourceBreakpointSpec {
    /// 1-based line number.
    pub line: i64,
    /// Optional adapter-evaluated condition expression.
    pub condition: Option<String>,
    /// Optional hit-count expression.
    pub hit_condition: Option<String>,
    /// Optional logpoint message (turns the breakpoint into a logpoint).
    pub log_message: Option<String>,
}

/// Default per-request deadline. DAP requests are interactive — a
/// `variables` reply that takes longer than this is effectively dead.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Initialize must allow extra time for adapters that scan the
/// workspace at startup.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(20);

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<ProtocolResponse>>>>;

/// Live connection to one DAP adapter.
pub struct DapClient {
    adapter_name: String,
    spec: DapAdapterSpec,
    child: Option<Child>,
    /// Writer end of the child's stdin, shared with the reader task in
    /// case the adapter issues a `runInTerminal` request (rare; the
    /// host replies `success: false` and the adapter falls back).
    stdin: Arc<Mutex<ChildStdin>>,
    _reader: JoinHandle<()>,
    events: Arc<Mutex<mpsc::UnboundedReceiver<AdapterEvent>>>,
    pending: PendingMap,
    /// Outbound `seq` counter. DAP requires monotonic-per-direction.
    next_seq: AtomicI64,
    /// Last-known breakpoint set per source path, kept for resync
    /// across reconnects.
    breakpoints: Mutex<HashMap<String, Vec<SourceBreakpointSpec>>>,
    /// Capabilities captured from the `initialize` response.
    capabilities: Mutex<AdapterCapabilities>,
}

impl DapClient {
    /// Spawn the adapter and run the `initialize` handshake.
    ///
    /// # Errors
    /// - [`DapClientError::Spawn`] when the executable fails to start.
    /// - [`DapClientError::Handshake`] when initialize fails or times out.
    /// - [`DapClientError::Transport`] for wire failures during init.
    ///
    /// # Panics
    /// Panics if the spawned `tokio::process::Child` doesn't expose
    /// stdin/stdout/stderr — they're explicitly piped via
    /// `Stdio::piped()` above, so this is structurally unreachable.
    #[allow(clippy::too_many_lines)]
    pub async fn connect(
        adapter_name: &str,
        spec: &DapAdapterSpec,
    ) -> Result<Self, DapClientError> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| DapClientError::Spawn {
            command: spec.command.clone(),
            source: e,
        })?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Drain stderr to keep the child's pipe from blocking.
        let adapter_label = adapter_name.to_string();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(adapter = %adapter_label, "stderr: {}", line);
            }
        });

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let pending_for_reader = Arc::clone(&pending);
        let stdin_arc: Arc<Mutex<ChildStdin>> = Arc::new(Mutex::new(stdin));
        let stdin_for_reader = Arc::clone(&stdin_arc);
        let adapter_label = adapter_name.to_string();

        // Seq generator for server-initiated request replies. The
        // client's outbound seq counter ([`next_seq`]) is dedicated to
        // requests we originate; replies use a separate counter so we
        // never collide with caller-tracked ids.
        let reader_reply_seq = Arc::new(AtomicI64::new(1));
        let reader_reply_seq_for_reader = Arc::clone(&reader_reply_seq);

        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader).await {
                    Ok(ProtocolMessage::Response(resp)) => {
                        let mut map = pending_for_reader.lock().await;
                        if let Some(tx) = map.remove(&resp.request_seq) {
                            let _ = tx.send(resp);
                        } else {
                            tracing::warn!(
                                adapter = %adapter_label,
                                request_seq = resp.request_seq,
                                "response for unknown request_seq"
                            );
                        }
                    }
                    Ok(ProtocolMessage::Event(evt)) => {
                        let _ = event_tx.send(AdapterEvent {
                            event: evt.event,
                            body: evt.body.unwrap_or(serde_json::Value::Null),
                        });
                    }
                    Ok(ProtocolMessage::Request(req)) => {
                        // Adapter-initiated request. DAP defines a
                        // few well-known ones (`runInTerminal`,
                        // `startDebugging`); answer with
                        // `success: false` so the adapter falls back
                        // rather than hanging.
                        let our_seq = reader_reply_seq_for_reader.fetch_add(1, Ordering::Relaxed);
                        let reply = ProtocolMessage::Response(ProtocolResponse {
                            seq: our_seq,
                            request_seq: req.seq,
                            success: false,
                            command: req.command.clone(),
                            message: Some(format!(
                                "host does not implement adapter request '{}'",
                                req.command
                            )),
                            body: None,
                        });
                        let mut stdin = stdin_for_reader.lock().await;
                        if let Err(err) = write_message(&mut *stdin, &reply).await {
                            tracing::warn!(
                                adapter = %adapter_label,
                                command = %req.command,
                                error = %err,
                                "failed to reply to adapter-initiated request"
                            );
                        }
                        drop(stdin);
                    }
                    Err(TransportError::Eof) => {
                        tracing::info!(
                            adapter = %adapter_label,
                            "adapter closed stdout — reader task exiting"
                        );
                        let mut map = pending_for_reader.lock().await;
                        for (_, tx) in map.drain() {
                            // Synthesise a failure response so callers
                            // unblock with a sensible error.
                            let _ = tx.send(ProtocolResponse {
                                seq: 0,
                                request_seq: 0,
                                success: false,
                                command: String::new(),
                                message: Some(format!("adapter '{adapter_label}' closed")),
                                body: None,
                            });
                        }
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            adapter = %adapter_label,
                            error = %e,
                            "transport error — reader task exiting"
                        );
                        let mut map = pending_for_reader.lock().await;
                        for (_, tx) in map.drain() {
                            let _ = tx.send(ProtocolResponse {
                                seq: 0,
                                request_seq: 0,
                                success: false,
                                command: String::new(),
                                message: Some(format!("transport: {e}")),
                                body: None,
                            });
                        }
                        break;
                    }
                }
            }
        });

        let client = Self {
            adapter_name: adapter_name.to_string(),
            spec: spec.clone(),
            child: Some(child),
            stdin: stdin_arc,
            _reader: reader,
            events: Arc::new(Mutex::new(event_rx)),
            pending,
            next_seq: AtomicI64::new(1),
            breakpoints: Mutex::new(HashMap::new()),
            capabilities: Mutex::new(AdapterCapabilities::default()),
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), DapClientError> {
        let init_args = json!({
            "clientID": "nexus",
            "clientName": "nexus-dap",
            "adapterID": self.spec.adapter_type.clone().unwrap_or_else(|| self.adapter_name.clone()),
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsVariableType": true,
            "supportsVariablePaging": false,
            "supportsRunInTerminalRequest": false,
            "supportsProgressReporting": false,
            "locale": "en",
        });
        let resp = match timeout(
            INITIALIZE_TIMEOUT,
            self.send_request("initialize", Some(init_args)),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return Err(DapClientError::Handshake {
                    adapter: self.adapter_name.clone(),
                    reason: e.to_string(),
                });
            }
            Err(_) => {
                return Err(DapClientError::Handshake {
                    adapter: self.adapter_name.clone(),
                    reason: format!(
                        "initialize timed out after {}s",
                        INITIALIZE_TIMEOUT.as_secs()
                    ),
                });
            }
        };
        let caps = AdapterCapabilities::from_response(resp.as_ref());
        tracing::info!(
            adapter = %self.adapter_name,
            supports_configuration_done = caps.supports_configuration_done,
            supports_function_breakpoints = caps.supports_function_breakpoints,
            "adapter initialized"
        );
        *self.capabilities.lock().await = caps;
        Ok(())
    }

    /// Send a request and await the response.
    ///
    /// # Errors
    /// - [`DapClientError::Transport`] for wire failures.
    /// - [`DapClientError::AdapterError`] when the adapter sets
    ///   `success: false`.
    /// - [`DapClientError::RequestTimeout`] if no response arrives
    ///   within [`DEFAULT_REQUEST_TIMEOUT`].
    pub async fn send_request(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, DapClientError> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(seq, tx);
        let req = ProtocolMessage::Request(ProtocolRequest {
            seq,
            command: command.to_string(),
            arguments,
        });
        {
            let mut stdin = self.stdin.lock().await;
            if let Err(e) = write_message(&mut *stdin, &req).await {
                self.pending.lock().await.remove(&seq);
                return Err(DapClientError::Transport(e));
            }
        }
        let outcome = timeout(DEFAULT_REQUEST_TIMEOUT, rx).await;
        match outcome {
            Ok(Ok(resp)) => {
                if resp.success {
                    Ok(resp.body)
                } else if resp.command.is_empty() {
                    // Synthesised by the reader task on EOF / transport
                    // error — surface as NotRunning so the pool's
                    // reconnect loop fires.
                    Err(DapClientError::NotRunning {
                        adapter: self.adapter_name.clone(),
                    })
                } else {
                    Err(DapClientError::AdapterError {
                        command: command.to_string(),
                        message: resp
                            .message
                            .unwrap_or_else(|| "(no error message)".to_string()),
                    })
                }
            }
            Ok(Err(_)) => Err(DapClientError::NotRunning {
                adapter: self.adapter_name.clone(),
            }),
            Err(_) => {
                self.pending.lock().await.remove(&seq);
                Err(DapClientError::RequestTimeout {
                    command: command.to_string(),
                    ms: u64::try_from(DEFAULT_REQUEST_TIMEOUT.as_millis())
                        .unwrap_or(u64::MAX),
                })
            }
        }
    }

    /// Drain pending adapter events. Returns everything queued
    /// without blocking.
    pub async fn drain_events(&self) -> Vec<AdapterEvent> {
        let mut rx = self.events.lock().await;
        let mut out = Vec::new();
        while let Ok(e) = rx.try_recv() {
            out.push(e);
        }
        out
    }

    /// Wait for the next event (or `None` if the channel is closed).
    /// Useful for tests; the host plugin uses [`drain_events`] in its
    /// poll loop.
    pub async fn next_event(&self) -> Option<AdapterEvent> {
        let mut rx = self.events.lock().await;
        rx.recv().await
    }

    /// Cache the latest breakpoint set for a source path. Called by
    /// the core plugin alongside `setBreakpoints` so the pool's
    /// reconnect loop can replay.
    pub async fn remember_breakpoints(&self, source: &str, bps: Vec<SourceBreakpointSpec>) {
        self.breakpoints
            .lock()
            .await
            .insert(source.to_string(), bps);
    }

    /// Snapshot the cached breakpoint set for resync.
    pub async fn breakpoints_snapshot(&self) -> HashMap<String, Vec<SourceBreakpointSpec>> {
        self.breakpoints.lock().await.clone()
    }

    /// Latest capabilities captured during init.
    pub async fn capabilities(&self) -> AdapterCapabilities {
        self.capabilities.lock().await.clone()
    }

    /// Adapter name from the config.
    #[must_use]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Spec the client was constructed from.
    #[must_use]
    pub fn spec(&self) -> &DapAdapterSpec {
        &self.spec
    }

    /// `true` while the child is still attached. The reader task
    /// drains pending requests on EOF, so callers usually rely on
    /// `send_request → NotRunning` rather than polling this.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
    }

    /// Graceful shutdown. Sends `disconnect`, then waits up to 5 s
    /// for the child to exit; kills it otherwise.
    pub async fn shutdown(&mut self) {
        let _ = timeout(
            Duration::from_secs(2),
            self.send_request("disconnect", Some(json!({ "restart": false, "terminateDebuggee": true }))),
        )
        .await;
        if let Ok(mut stdin) = self.stdin.try_lock() {
            let _ = stdin.shutdown().await;
        }
        if let Some(mut child) = self.child.take() {
            match timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => tracing::info!(
                    adapter = %self.adapter_name,
                    code = ?status.code(),
                    "adapter exited"
                ),
                Ok(Err(e)) => tracing::warn!(
                    adapter = %self.adapter_name,
                    error = %e,
                    "wait failed"
                ),
                Err(_) => {
                    tracing::warn!(
                        adapter = %self.adapter_name,
                        "adapter did not exit in 5s — killing"
                    );
                    let _ = child.kill().await;
                }
            }
        }
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        if self.child.is_some() {
            tracing::debug!(
                adapter = %self.adapter_name,
                "DapClient dropped without explicit shutdown"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_parse_known_flags() {
        let body = serde_json::json!({
            "supportsConfigurationDoneRequest": true,
            "supportsFunctionBreakpoints": true,
            "supportsConditionalBreakpoints": false,
            "supportsTerminateRequest": true,
            "supportsExceptionOptions": false,
        });
        let caps = AdapterCapabilities::from_response(Some(&body));
        assert!(caps.supports_configuration_done);
        assert!(caps.supports_function_breakpoints);
        assert!(!caps.supports_conditional_breakpoints);
        assert!(caps.supports_terminate_request);
        assert!(!caps.supports_exception_options);
        assert_eq!(caps.raw, body);
    }

    #[test]
    fn capabilities_empty_body_yields_defaults() {
        let caps = AdapterCapabilities::from_response(None);
        assert!(!caps.supports_configuration_done);
        assert!(!caps.supports_function_breakpoints);
        assert_eq!(caps.raw, serde_json::Value::Null);
    }

    #[test]
    fn is_transient_classifies_correctly() {
        let timeout_err = DapClientError::RequestTimeout {
            command: "continue".to_string(),
            ms: 10,
        };
        assert!(timeout_err.is_transient());

        let spawn_err = DapClientError::Spawn {
            command: "codelldb".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no"),
        };
        assert!(!spawn_err.is_transient());

        let adapter_err = DapClientError::AdapterError {
            command: "launch".to_string(),
            message: "no such file".to_string(),
        };
        assert!(!adapter_err.is_transient());

        let not_running = DapClientError::NotRunning {
            adapter: "rust".to_string(),
        };
        assert!(not_running.is_transient());
    }
}
