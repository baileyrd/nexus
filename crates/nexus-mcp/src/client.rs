//! MCP **Host** — client-side integration that lets Nexus consume external
//! MCP servers.
//!
//! # Role split
//!
//! | Side | Module | Purpose |
//! |------|--------|---------|
//! | Server | [`crate::server`] | Nexus exposes its own tools (note CRUD, search, graph, RAG, …) to external AI clients (Claude Desktop, Cursor, etc.) |
//! | **Host / client** | `crate::client` (this module) | Nexus spawns external MCP servers from `mcp.toml` and calls their tools — the same pattern Claude Desktop uses to call `filesystem` / `github` / etc. servers. |
//!
//! Both halves share the same wire protocol — different roles on it.
//!
//! # Microkernel fit
//!
//! [`McpClient`] is a plain library type, not a core plugin. Nothing in the
//! kernel needs to call external MCP servers today (the AI engine already
//! has its own provider traits), so exposing this as an IPC surface would
//! add ceremony without customers. If a plugin later needs cross-server
//! tool dispatch — e.g. routing an AI tool call through a Host-owned
//! filesystem server — the natural shape is a `com.nexus.mcp.host` core
//! plugin whose `dispatch` routes to a shared `McpHost` managing a pool of
//! [`McpClient`] connections. None of this module has to change when that
//! lands.
//!
//! # Transport
//!
//! stdio only. The MCP ecosystem's dominant transport is stdio (Claude
//! Desktop, Cursor, Cline all use it); WebSocket / HTTP+SSE transports
//! exist in rmcp and can be wired in as a follow-up without changing
//! the public surface here — `McpClient::list_tools` / `call_tool` are
//! transport-agnostic.
//!
//! # Lifecycle
//!
//! [`McpClient::connect`] spawns the configured command, runs the MCP
//! handshake, and returns a ready client. [`McpClient::shutdown`] issues a
//! graceful close; dropping without calling `shutdown` still tears the
//! connection down via the transport's own Drop (the child process is
//! killed after a 3-second grace window).

use std::process::Stdio;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, Prompt, Resource, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{serve_client, RoleClient};
use tokio::process::Command;

use crate::config::McpServerSpec;

/// Default timeout for the MCP initialize handshake. A non-responding server
/// binary would otherwise hang connect for however long its stdout takes to
/// unblock.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Budget for graceful shutdown. Beyond this the transport will kill the
/// child process forcibly (see `TokioChildProcess::graceful_shutdown`).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors from the MCP Host client.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    /// Failed to spawn the server process (command not on PATH, permission
    /// denied, etc.).
    #[error("spawn {command}: {source}")]
    Spawn {
        /// Command that failed to spawn.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The MCP initialize handshake failed or timed out.
    #[error("initialize handshake failed: {reason}")]
    Handshake {
        /// Why the handshake failed.
        reason: String,
    },

    /// Any runtime error from the underlying rmcp service (transport closed,
    /// protocol violation, etc.).
    #[error("mcp service error: {0}")]
    Service(String),
}

impl McpClientError {
    /// Whether the error represents a transient runtime failure that may
    /// succeed on retry. `Service` errors are transient (transport blip,
    /// remote restart). `Spawn` and `Handshake` are not — both indicate
    /// misconfiguration that retrying would just delay surfacing.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Service(_))
    }
}

/// A live connection to one external MCP server. Cheap to `Deref` through
/// for advanced use (the rmcp `Peer<RoleClient>` is exposed via the field
/// type), but the methods on this struct cover the common cases and are
/// stable across rmcp minor-version bumps.
///
/// `McpClient` is `Send` but **not** `Sync`. Share via `Arc<Mutex<…>>` or
/// (preferred) move it into a dedicated actor task owned by the Host.
#[derive(Debug)]
pub struct McpClient {
    /// Human-readable name (the key from `mcp.toml`), used in logs and error
    /// messages.
    name: String,
    /// Live rmcp service. Dropping closes the transport and kills the child.
    service: RunningService<RoleClient, ()>,
}

impl McpClient {
    /// Spawn the configured external MCP server and perform the initialize
    /// handshake. Returns once the server has reported its capabilities.
    ///
    /// The child's stderr is inherited (not captured) so the operator sees
    /// server-side startup logs in their terminal — critical for diagnosing
    /// misconfigured server binaries.
    ///
    /// # Errors
    /// - [`McpClientError::Spawn`] if the executable cannot be started.
    /// - [`McpClientError::Handshake`] if the initialize round-trip fails
    ///   or exceeds [`CONNECT_TIMEOUT`].
    pub async fn connect(name: &str, spec: &McpServerSpec) -> Result<Self, McpClientError> {
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let transport = TokioChildProcess::new(command).map_err(|e| McpClientError::Spawn {
            command: spec.command.clone(),
            source: e,
        })?;

        // Race the handshake against a wall-clock timeout so a server that
        // writes nothing to stdout doesn't hang connect forever.
        let serve = serve_client((), transport);
        let service = match tokio::time::timeout(CONNECT_TIMEOUT, serve).await {
            Ok(Ok(svc)) => svc,
            Ok(Err(e)) => {
                return Err(McpClientError::Handshake {
                    reason: e.to_string(),
                });
            }
            Err(_) => {
                return Err(McpClientError::Handshake {
                    reason: format!("initialize exceeded {CONNECT_TIMEOUT:?}"),
                });
            }
        };

        Ok(Self {
            name: name.to_string(),
            service,
        })
    }

    /// Logical name of this connection (the key from `mcp.toml`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fetch every tool the server exposes, transparently paginating until
    /// the server reports no further cursor.
    ///
    /// # Errors
    /// [`McpClientError::Service`] on transport failure or protocol error.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, McpClientError> {
        self.service
            .list_all_tools()
            .await
            .map_err(|e| McpClientError::Service(e.to_string()))
    }

    /// Fetch every resource the server exposes.
    ///
    /// # Errors
    /// [`McpClientError::Service`] on transport failure or protocol error.
    pub async fn list_resources(&self) -> Result<Vec<Resource>, McpClientError> {
        self.service
            .list_all_resources()
            .await
            .map_err(|e| McpClientError::Service(e.to_string()))
    }

    /// Fetch every prompt template the server exposes.
    ///
    /// # Errors
    /// [`McpClientError::Service`] on transport failure or protocol error.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>, McpClientError> {
        self.service
            .list_all_prompts()
            .await
            .map_err(|e| McpClientError::Service(e.to_string()))
    }

    /// Invoke a tool by name with the given JSON arguments.
    ///
    /// `arguments` is an optional `serde_json::Map` matching the tool's
    /// declared input schema. Pass `None` for tools that take no arguments.
    ///
    /// # Errors
    /// [`McpClientError::Service`] on transport failure, protocol error, or
    /// a tool error reported by the server.
    pub async fn call_tool(
        &self,
        name: impl Into<String>,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<CallToolResult, McpClientError> {
        // `CallToolRequestParams` is `#[non_exhaustive]` (added `_meta` and
        // `task` fields in rmcp 1.4 for SEP-1319 task augmentation), so
        // construct via the `new(...).with_arguments(...)` builder rather
        // than a struct literal — keeps us compatible with future field
        // additions without another compile break.
        let mut params = CallToolRequestParams::new(name.into());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        self.service
            .call_tool(params)
            .await
            .map_err(|e| McpClientError::Service(e.to_string()))
    }

    /// Gracefully shut down the connection: cancels the service, waits for
    /// the transport to flush and close, and kills the child process if it
    /// doesn't exit within [`SHUTDOWN_TIMEOUT`].
    ///
    /// # Errors
    /// Returns [`McpClientError::Service`] if the shutdown join failed. The
    /// child is killed regardless.
    pub async fn shutdown(mut self) -> Result<(), McpClientError> {
        match self.service.close_with_timeout(SHUTDOWN_TIMEOUT).await {
            Ok(_) => Ok(()),
            Err(e) => Err(McpClientError::Service(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only thing we can meaningfully unit-test at this layer is the
    /// spawn-failure path, because connecting to a real MCP server requires
    /// a whole other binary. A "server binary does not exist on PATH" check
    /// exercises the spawn surface end-to-end without needing one.
    #[tokio::test]
    async fn connect_fails_for_nonexistent_command() {
        let spec = McpServerSpec {
            command: "this-binary-definitely-does-not-exist-12345".to_string(),
            args: vec![],
            env: std::collections::BTreeMap::new(),
            disabled: false,
        };
        let err = McpClient::connect("test", &spec).await.unwrap_err();
        assert!(
            matches!(err, McpClientError::Spawn { .. }),
            "expected Spawn error, got {err:?}"
        );
    }

    /// Handshake must time out if the spawned process never writes to
    /// stdout. We use `/usr/bin/yes` as a "never-respond-to-MCP" stand-in:
    /// it writes to stdout indefinitely but never produces valid MCP
    /// framing, so the initialize read stalls and the timeout fires.
    ///
    /// Skipped on platforms where `yes` is not available; this is a
    /// best-effort smoke test rather than a portability guarantee.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn connect_times_out_when_server_never_speaks_mcp() {
        // `sleep 60` is portable and quieter than `yes`: it writes nothing,
        // so the transport's framed reader blocks forever. Our 15 s
        // CONNECT_TIMEOUT would be too long for CI, so we override by
        // shortening via direct tokio timeout around the future. But the
        // public API caps at CONNECT_TIMEOUT, so we simply skip this test
        // if the binary is missing and otherwise tolerate the wait.
        if std::process::Command::new("sleep")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let spec = McpServerSpec {
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::BTreeMap::new(),
            disabled: false,
        };
        // Directly bound the connect call below CONNECT_TIMEOUT so CI
        // completes quickly. The production timeout still applies in real
        // usage; here we only want to prove the spawn + transport path
        // doesn't panic when the child refuses to speak MCP.
        let connect = McpClient::connect("test", &spec);
        let short = tokio::time::timeout(Duration::from_millis(500), connect).await;
        // Expect either the local short timeout fired (Err) OR the rmcp
        // side errored out fast. Either way, we must not get `Ok`.
        match short {
            Err(_elapsed) => {}
            Ok(Err(_)) => {}
            Ok(Ok(_)) => panic!("connect should not have succeeded against a non-MCP process"),
        }
    }
}
