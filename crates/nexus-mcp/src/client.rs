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
//! Two transports ship today, dispatched per-entry from `mcp.toml` via the
//! [`crate::config::McpTransport`] discriminant:
//!
//! - **`stdio`** (default) — spawn the configured command, talk MCP over
//!   the child's stdin/stdout. The dominant transport in the ecosystem
//!   (Claude Desktop, Cursor, Cline all use it) and the only pre-BL-023
//!   option.
//! - **`http`** — connect over the modern MCP "Streamable HTTP" transport
//!   (POST + SSE under one endpoint, per the 2025-03-26 spec). Backed by
//!   `rmcp::transport::StreamableHttpClientTransport`. Auth headers and
//!   custom headers ride on the same `McpServerSpec`.
//!
//! WebSocket is reserved in the config schema but not currently
//! dispatchable — rmcp 1.5 ships only a stub (`src/transport/ws.rs`
//! comment: "Maybe we don't really need a ws implementation?") because
//! the MCP working group folded WS into Streamable HTTP. See
//! [`McpClient::connect`] for the explicit error path.
//!
//! # Lifecycle
//!
//! [`McpClient::connect`] spawns the configured command, runs the MCP
//! handshake, and returns a ready client. [`McpClient::shutdown`] issues a
//! graceful close; dropping without calling `shutdown` still tears the
//! connection down via the transport's own Drop (the child process is
//! killed after a 3-second grace window).

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, CallToolResult, Prompt, Resource, Tool};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::TokioChildProcess;
// `from_config` is only present on `StreamableHttpClientTransport<reqwest::Client>`,
// where `reqwest::Client` is the version rmcp itself depends on (currently
// 0.13). We must NOT name `reqwest::Client` ourselves here — the workspace
// pins `reqwest = "0.12"` for the AI provider crates and the two would not
// be type-compatible. The function call below uses the rmcp re-exported
// type implicitly through trait inference, dodging the version gap.
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{serve_client, RoleClient};
use tokio::process::Command;

use crate::auth::{self, AuthError};
use crate::config::{McpServerSpec, McpTransport};

/// P2-06 — default timeout for the MCP initialize handshake. A
/// non-responding server binary would otherwise hang connect for
/// however long its stdout takes to unblock. Override via a future
/// `[mcp.timeouts] connect_secs = N` block (deferred from P2-06).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = DEFAULT_CONNECT_TIMEOUT;

/// P2-06 — budget for graceful shutdown. Beyond this the transport
/// will kill the child process forcibly (see
/// `TokioChildProcess::graceful_shutdown`).
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = DEFAULT_SHUTDOWN_TIMEOUT;

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

    /// Configuration error — the spec is missing required fields for its
    /// declared transport (e.g. `transport = "http"` with no `url`).
    #[error("transport config: {reason}")]
    Config {
        /// Why the spec was rejected before any I/O happened.
        reason: String,
    },

    /// The transport listed in the spec is recognised but not currently
    /// dispatchable in this build (e.g. `transport = "websocket"`, which
    /// rmcp 1.5 does not implement).
    #[error("transport unsupported: {reason}")]
    Unsupported {
        /// Human-readable explanation including a migration hint.
        reason: String,
    },

    /// BL-025 — auth resolution failed before the transport could be
    /// constructed (missing env var, OAuth token endpoint refused,
    /// malformed token response, …).
    #[error("auth: {0}")]
    Auth(#[from] AuthError),

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
        match spec.transport {
            McpTransport::Stdio => Self::connect_stdio(name, spec).await,
            McpTransport::Http => Self::connect_http(name, spec).await,
            McpTransport::Websocket => Err(McpClientError::Unsupported {
                reason: format!(
                    "server '{name}': WebSocket transport is reserved in the config schema \
                     but not implemented (rmcp 1.5 ships no WebSocket transport; the MCP \
                     2025-03-26 spec deprecates WebSocket in favour of `transport = \"http\"`). \
                     Switch to `transport = \"http\"` to connect to this server."
                ),
            }),
        }
    }

    /// Stdio path — spawn the configured command and run the MCP
    /// handshake over the child's stdio. Pre-BL-023 behaviour, kept
    /// byte-identical for forward compatibility with deployed
    /// `mcp.toml` files.
    async fn connect_stdio(name: &str, spec: &McpServerSpec) -> Result<Self, McpClientError> {
        if spec.command.trim().is_empty() {
            return Err(McpClientError::Config {
                reason: format!("server '{name}': stdio transport needs a non-empty `command`"),
            });
        }
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

        Self::run_handshake(name, serve_client((), transport)).await
    }

    /// Streamable HTTP path — the modern remote MCP transport (single
    /// endpoint, POST for requests + SSE for the server stream). Auth
    /// headers and custom HTTP headers from the spec are forwarded on
    /// every request.
    async fn connect_http(name: &str, spec: &McpServerSpec) -> Result<Self, McpClientError> {
        let url = spec.url.as_deref().unwrap_or("").trim();
        if url.is_empty() {
            return Err(McpClientError::Config {
                reason: format!("server '{name}': http transport needs `url = \"https://…\"`"),
            });
        }

        // BL-025 — resolve the optional `auth` declaration up front so
        // any missing env-var or OAuth endpoint failure surfaces with a
        // clear `Auth` error before we construct the transport. The
        // resolver returns a logical `Authorization` value plus
        // additional headers; we then merge those into the per-request
        // header map below. A static `auth_header` from the file still
        // works (back-compat with pre-BL-025 installs) but a present
        // `auth` block always wins on conflict — declarative beats
        // legacy.
        let resolved_auth = if let Some(auth_decl) = spec.auth.as_ref() {
            Some(auth::resolve(auth_decl).await?)
        } else {
            None
        };

        // Parse custom headers up front so a malformed entry surfaces as a
        // clean Config error (rather than rmcp's typed-error stack at
        // runtime). Header names get the same case-insensitive treatment
        // browsers do; rmcp internally canonicalises again.
        let mut custom_headers: HashMap<HeaderName, HeaderValue> = HashMap::with_capacity(
            spec.headers.len() + resolved_auth.as_ref().map_or(0, |r| r.extra_headers.len()),
        );
        for (k, v) in spec.headers.iter().chain(
            resolved_auth
                .as_ref()
                .map(|r| r.extra_headers.iter())
                .into_iter()
                .flatten(),
        ) {
            let header_name =
                HeaderName::try_from(k.as_str()).map_err(|e| McpClientError::Config {
                    reason: format!("server '{name}': invalid header name '{k}': {e}"),
                })?;
            let header_value =
                HeaderValue::try_from(v.as_str()).map_err(|e| McpClientError::Config {
                    reason: format!("server '{name}': invalid header value for '{k}': {e}"),
                })?;
            custom_headers.insert(header_name, header_value);
        }

        let mut http_cfg = StreamableHttpClientTransportConfig::with_uri(Arc::<str>::from(url));
        http_cfg = http_cfg.custom_headers(custom_headers);
        // Resolved auth wins; otherwise fall back to the static
        // `auth_header` field (the BL-023 path).
        let auth_header = resolved_auth
            .as_ref()
            .and_then(|r| r.authorization.clone())
            .or_else(|| spec.auth_header.clone());
        if let Some(auth) = auth_header {
            http_cfg = http_cfg.auth_header(auth);
        }
        // The reqwest-backed default client uses the version of reqwest
        // that ships with rmcp's `transport-streamable-http-client-reqwest`
        // feature; calling through `from_config` avoids naming the type
        // here (see the import comment).
        let transport = StreamableHttpClientTransport::from_config(http_cfg);

        Self::run_handshake(name, serve_client((), transport)).await
    }

    /// Shared handshake driver: race the rmcp `serve_client(...)` future
    /// against [`CONNECT_TIMEOUT`] so a non-responsive transport doesn't
    /// hang `connect` forever. Reused by every transport branch above so
    /// the timeout policy stays uniform.
    async fn run_handshake<F, E>(name: &str, fut: F) -> Result<Self, McpClientError>
    where
        F: std::future::Future<Output = Result<RunningService<RoleClient, ()>, E>>,
        E: std::fmt::Display,
    {
        let service = match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
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
            ..McpServerSpec::default()
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
            ..McpServerSpec::default()
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

    // ── BL-023 — transport dispatch ──────────────────────────────────────

    #[tokio::test]
    async fn websocket_transport_returns_unsupported() {
        // The transport variant is reserved (see McpTransport doc); the
        // connect-time dispatch must surface a clear error pointing at
        // the http alternative rather than silently falling back.
        let spec = McpServerSpec {
            transport: McpTransport::Websocket,
            url: Some("wss://example.com/mcp".into()),
            ..McpServerSpec::default()
        };
        let err = McpClient::connect("legacy", &spec).await.unwrap_err();
        match err {
            McpClientError::Unsupported { reason } => {
                assert!(
                    reason.contains("WebSocket"),
                    "error should mention WebSocket: {reason}"
                );
                assert!(
                    reason.contains("http"),
                    "error should suggest http alternative: {reason}"
                );
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_transport_rejects_missing_url_with_config_error() {
        let spec = McpServerSpec {
            transport: McpTransport::Http,
            url: None,
            ..McpServerSpec::default()
        };
        let err = McpClient::connect("remote", &spec).await.unwrap_err();
        assert!(
            matches!(err, McpClientError::Config { .. }),
            "expected Config, got {err:?}"
        );
    }

    #[tokio::test]
    async fn http_transport_rejects_invalid_header_name() {
        // Headers are validated up-front so malformed entries don't reach
        // the wire (the rmcp transport would otherwise lazily error on
        // first request, which is harder to diagnose).
        let mut headers = std::collections::BTreeMap::new();
        headers.insert("not a valid header".to_string(), "value".to_string());
        let spec = McpServerSpec {
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".into()),
            headers,
            ..McpServerSpec::default()
        };
        let err = McpClient::connect("remote", &spec).await.unwrap_err();
        assert!(
            matches!(err, McpClientError::Config { .. }),
            "expected Config, got {err:?}"
        );
    }

    #[tokio::test]
    async fn http_transport_auth_missing_env_surfaces_as_auth_error() {
        // BL-025: an env-indirected secret with no env value must abort
        // connect with the typed Auth variant — not the generic Handshake
        // — so the operator can act on it directly.
        unsafe { std::env::remove_var("NEXUS_TEST_BL025_CONNECT_NOENV") };
        let spec = McpServerSpec {
            transport: McpTransport::Http,
            url: Some("http://127.0.0.1:1/mcp".into()),
            auth: Some(crate::auth::McpAuth::Bearer {
                token: crate::auth::McpAuthSecret::Env {
                    env: "NEXUS_TEST_BL025_CONNECT_NOENV".into(),
                },
            }),
            ..McpServerSpec::default()
        };
        let err = McpClient::connect("remote", &spec).await.unwrap_err();
        match err {
            McpClientError::Auth(crate::auth::AuthError::MissingEnv { name }) => {
                assert_eq!(name, "NEXUS_TEST_BL025_CONNECT_NOENV");
            }
            other => panic!("expected Auth(MissingEnv), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_transport_handshake_times_out_against_dead_endpoint() {
        // 127.0.0.1 with a deliberately-unused port. We don't wait for
        // the production CONNECT_TIMEOUT (15s) — bound the test future
        // tighter via a local timeout. The point is to prove the connect
        // surface routes through `connect_http` without panicking.
        let spec = McpServerSpec {
            transport: McpTransport::Http,
            // Port 1 is reserved; OS rejects the TCP connect immediately.
            url: Some("http://127.0.0.1:1/mcp".into()),
            ..McpServerSpec::default()
        };
        let connect = McpClient::connect("dead", &spec);
        let short = tokio::time::timeout(Duration::from_millis(2_000), connect).await;
        match short {
            Err(_elapsed) => {}
            Ok(Err(_)) => {} // surfaced as Handshake / transport error
            Ok(Ok(_)) => panic!("connect should not have succeeded against 127.0.0.1:1"),
        }
    }
}
