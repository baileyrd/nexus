//! Parser for `mcp.toml` — the forge-local registry of external MCP servers
//! Nexus should connect to as a **Host** (client).
//!
//! # File location
//!
//! The Host looks for `<forge_root>/.forge/mcp.toml`. A missing file is
//! equivalent to "no external servers configured" — not an error.
//!
//! # Format
//!
//! ```toml
//! # Optional: default is `false` for every server. Use `disabled = true`
//! # to keep the entry in the file but skip it at connect time.
//!
//! [servers.filesystem]
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
//!
//! [servers.filesystem.env]
//! NODE_ENV = "production"
//!
//! [servers.github]
//! command = "uvx"
//! args = ["mcp-server-github"]
//! disabled = false
//! ```
//!
//! This mirrors Claude Desktop / Cursor's `mcp.json` naming — deliberately
//! so the same invocations work across hosts with a TOML → JSON rewrite.
//!
//! # Scope
//!
//! This module parses the file and surfaces validated specs. Connection
//! orchestration lives in [`crate::client`]; tracking which servers are
//! currently live is the Host's job (see `McpHost` when that lands).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::auth::McpAuth;

/// Wire-level transport for one MCP server entry.
///
/// **BL-023.** Default is [`McpTransport::Stdio`] for back-compat with the
/// pre-BL-023 file format (where every entry had a `command` field and no
/// `transport` discriminator). Remote transports must declare `transport =
/// "..."` explicitly.
///
/// Naming follows the MCP spec terminology: `http` is the modern
/// "Streamable HTTP" transport from the 2025-03-26 spec (single endpoint,
/// HTTP+SSE under the hood); `websocket` is the older WS transport which
/// the MCP working group has since deprecated in favour of Streamable HTTP
/// and is not implemented in `rmcp` 1.5 (see
/// `rmcp/src/transport/ws.rs`'s upstream comment "Maybe we don't really
/// need a ws implementation?"). It is accepted in config so existing
/// `mcp.toml` files declaring it parse cleanly; connect-time it returns a
/// clear "unsupported" error pointing the operator at `http`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// Spawn a local executable; talk MCP over its stdio (the only
    /// transport pre-BL-023, still the dominant transport in the
    /// ecosystem).
    #[default]
    Stdio,
    /// Connect to a remote MCP server over Streamable HTTP — POST for
    /// requests, SSE for the server-pushed event stream.
    Http,
    /// **Reserved.** WebSocket is deprecated upstream and not currently
    /// dispatchable; left in the schema so a future rmcp WS impl can wire
    /// in without a breaking config change.
    Websocket,
}

/// A single external MCP server entry declared in `mcp.toml`.
///
/// Per BL-023 entries can run over either a child-process stdio
/// transport (the default) or the Streamable HTTP transport via
/// `transport = "http"` + `url = "..."`. Stdio-only fields (`command`,
/// `args`, `env`) are ignored on remote transports; remote-only fields
/// (`url`, `headers`, `auth_header`) are ignored on stdio.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct McpServerSpec {
    /// Wire-level transport. Defaults to [`McpTransport::Stdio`] for
    /// back-compat — entries that omit the field continue to be spawned
    /// as child processes.
    #[serde(default)]
    pub transport: McpTransport,
    /// Executable to spawn (e.g. `"npx"`, `"uvx"`, an absolute path).
    /// Required when `transport = "stdio"`; ignored otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Arguments passed to `command`. Empty vector if omitted.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables merged into the spawned child's environment.
    /// Ordered so config serialization stays stable across writes.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Endpoint URL for remote transports (`http` / `websocket`).
    /// Required when transport ≠ `stdio`; ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Bearer / API-key header value (the raw header value, e.g.
    /// `"Bearer ey..."` or just `"ey..."` for Streamable HTTP's bare-token
    /// fast path). Set by the BL-025 auth flow at connect time; the file
    /// can also pin it for static API-key servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
    /// Custom headers attached to every HTTP request (Streamable HTTP only).
    /// `BTreeMap` preserves order across writes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// **BL-025.** Optional auth declaration. Set on remote (`http`)
    /// transports; ignored for stdio (which inherits credentials via
    /// the `env` map). When set, the resolver runs at connect time
    /// and the returned headers are merged into `auth_header` /
    /// `headers` BEFORE the rmcp transport is constructed. Static
    /// `auth_header` from the file still works for back-compat
    /// (resolver output overrides it on conflict — declarative config
    /// wins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpAuth>,
    /// When `true`, the Host skips this server at connect time but leaves
    /// the entry in the file so toggling is a single-field edit.
    #[serde(default)]
    pub disabled: bool,
}

/// Top-level structure of `mcp.toml`. Currently one table (`[servers]`)
/// keyed by logical name; new tables (auth providers, transport selectors,
/// per-server capability allow-lists) can be added without breaking
/// existing files because `#[serde(default)]` makes every field optional.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpHostConfig {
    /// Ordered map of server name → spec. The name is the stable identifier
    /// the Host uses in logs, UI, and tool prefixing; the file order is
    /// preserved by using [`BTreeMap`] rather than a hash map.
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerSpec>,
}

/// Error parsing `mcp.toml`.
#[derive(Debug, thiserror::Error)]
pub enum McpConfigError {
    /// I/O error reading the config file.
    #[error("read {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// TOML parse / shape error.
    #[error("parse {path}: {source}")]
    Toml {
        /// Path that failed to parse.
        path: String,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },

    /// Semantic validation error (e.g. empty command).
    #[error("invalid mcp.toml at {path}: {reason}")]
    Invalid {
        /// Path that failed validation.
        path: String,
        /// Human-readable reason.
        reason: String,
    },
}

impl McpHostConfig {
    /// Parse TOML text into a validated [`McpHostConfig`]. `source` is the
    /// display path included in error messages; tests can pass a bogus path.
    ///
    /// # Errors
    /// Returns [`McpConfigError::Toml`] for malformed TOML or
    /// [`McpConfigError::Invalid`] for semantically-invalid entries
    /// (currently: empty `command`).
    pub fn from_str(text: &str, source: &str) -> Result<Self, McpConfigError> {
        let cfg: McpHostConfig = toml::from_str(text).map_err(|e| McpConfigError::Toml {
            path: source.to_string(),
            source: e,
        })?;
        cfg.validate(source)?;
        Ok(cfg)
    }

    /// Read and parse a file on disk. A missing file is treated as an empty
    /// configuration — equivalent to "no external servers" — because the
    /// Host MUST remain functional without any external MCP dependency.
    ///
    /// # Errors
    /// Returns [`McpConfigError::Io`] on read failures other than
    /// `NotFound`, [`McpConfigError::Toml`] on parse failures, or
    /// [`McpConfigError::Invalid`] on semantic validation failures.
    pub fn read_from(path: &Path) -> Result<Self, McpConfigError> {
        let source = path.display().to_string();
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(McpConfigError::Io {
                    path: source,
                    source: e,
                });
            }
        };
        Self::from_str(&text, &source)
    }

    /// Return only the servers that are currently enabled — the set the Host
    /// should actually try to connect to on startup.
    pub fn enabled_servers(&self) -> impl Iterator<Item = (&str, &McpServerSpec)> {
        self.servers
            .iter()
            .filter(|(_, spec)| !spec.disabled)
            .map(|(name, spec)| (name.as_str(), spec))
    }

    fn validate(&self, source: &str) -> Result<(), McpConfigError> {
        for (name, spec) in &self.servers {
            match spec.transport {
                McpTransport::Stdio => {
                    if spec.command.trim().is_empty() {
                        return Err(McpConfigError::Invalid {
                            path: source.to_string(),
                            reason: format!("server '{name}' has empty command"),
                        });
                    }
                }
                McpTransport::Http | McpTransport::Websocket => {
                    let url = spec.url.as_deref().unwrap_or("").trim();
                    if url.is_empty() {
                        return Err(McpConfigError::Invalid {
                            path: source.to_string(),
                            reason: format!(
                                "server '{name}' uses a remote transport but has no `url`"
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_server() {
        let toml = r#"
            [servers.echo]
            command = "echo"
            args = ["hello"]
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        assert_eq!(cfg.servers.len(), 1);
        let spec = cfg.servers.get("echo").unwrap();
        assert_eq!(spec.command, "echo");
        assert_eq!(spec.args, vec!["hello"]);
        assert!(spec.env.is_empty());
        assert!(!spec.disabled);
    }

    #[test]
    fn parses_env_and_disabled_flag() {
        let toml = r#"
            [servers.github]
            command = "uvx"
            args = ["mcp-server-github"]
            disabled = true

            [servers.github.env]
            GITHUB_TOKEN = "ghp_xxx"
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        let spec = cfg.servers.get("github").unwrap();
        assert!(spec.disabled);
        assert_eq!(spec.env.get("GITHUB_TOKEN").unwrap(), "ghp_xxx");
    }

    #[test]
    fn empty_file_yields_no_servers() {
        let cfg = McpHostConfig::from_str("", "inline").unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn empty_command_rejected() {
        let toml = r#"
            [servers.bad]
            command = ""
        "#;
        let err = McpHostConfig::from_str(toml, "inline").unwrap_err();
        assert!(matches!(err, McpConfigError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn missing_file_returns_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = McpHostConfig::read_from(&dir.path().join("does-not-exist.toml")).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn enabled_servers_filters_disabled() {
        let toml = r#"
            [servers.on]
            command = "a"

            [servers.off]
            command = "b"
            disabled = true
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        let enabled: Vec<&str> = cfg.enabled_servers().map(|(n, _)| n).collect();
        assert_eq!(enabled, vec!["on"]);
    }

    #[test]
    fn roundtrip_serialize_then_parse() {
        let mut cfg = McpHostConfig::default();
        cfg.servers.insert(
            "fs".to_string(),
            McpServerSpec {
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "@modelcontextprotocol/server-filesystem".into(),
                ],
                ..McpServerSpec::default()
            },
        );
        let text = toml::to_string_pretty(&cfg).unwrap();
        let round = McpHostConfig::from_str(&text, "roundtrip").unwrap();
        assert_eq!(cfg.servers, round.servers);
    }

    // ── BL-023 — remote transport variants ────────────────────────────────

    #[test]
    fn parses_http_transport() {
        let toml = r#"
            [servers.remote]
            transport = "http"
            url = "https://example.com/mcp"
            auth_header = "Bearer token"

            [servers.remote.headers]
            X-Workspace = "alpha"
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        let spec = cfg.servers.get("remote").unwrap();
        assert_eq!(spec.transport, McpTransport::Http);
        assert_eq!(spec.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(spec.auth_header.as_deref(), Some("Bearer token"));
        assert_eq!(spec.headers.get("X-Workspace").unwrap(), "alpha");
        assert!(spec.command.is_empty());
    }

    #[test]
    fn http_transport_requires_url() {
        let toml = r#"
            [servers.remote]
            transport = "http"
        "#;
        let err = McpHostConfig::from_str(toml, "inline").unwrap_err();
        assert!(matches!(err, McpConfigError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn stdio_transport_default_keeps_back_compat() {
        // Pre-BL-023 file (no `transport` field) must still parse and
        // dispatch as stdio.
        let toml = r#"
            [servers.fs]
            command = "echo"
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        assert_eq!(cfg.servers.get("fs").unwrap().transport, McpTransport::Stdio);
    }

    #[test]
    fn websocket_transport_parses_but_is_reserved() {
        // Config-level acceptance only; connect-time will fail with a
        // clear "unsupported" error per the McpTransport doc.
        let toml = r#"
            [servers.legacy]
            transport = "websocket"
            url = "wss://example.com/mcp"
        "#;
        let cfg = McpHostConfig::from_str(toml, "inline").unwrap();
        assert_eq!(
            cfg.servers.get("legacy").unwrap().transport,
            McpTransport::Websocket
        );
    }
}
