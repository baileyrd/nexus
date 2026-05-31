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
///
/// TOML-loaded entries and plugin-contributed entries share the
/// [`servers`] map; the [`contributed_by`] map distinguishes them
/// for unregister authorisation. See BL-113 / ADR 0027.
///
/// [`servers`]: Self::servers
/// [`contributed_by`]: Self::contributed_by
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpHostConfig {
    /// Ordered map of server name → spec. The name is the stable identifier
    /// the Host uses in logs, UI, and tool prefixing; the file order is
    /// preserved by using [`BTreeMap`] rather than a hash map.
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerSpec>,
    /// P2-06 — per-forge timeout overrides for the MCP client +
    /// server. Each field falls back to the corresponding
    /// `nexus_mcp::{client,server,auth}::DEFAULT_*` constant when
    /// unset. The schema is parsed today; runtime thread-through is
    /// a follow-up — the consts remain the operational defaults
    /// until the call sites are refactored to consult this struct.
    #[serde(default, skip_serializing_if = "McpTimeouts::is_empty")]
    pub timeouts: McpTimeouts,
    /// BL-113 Phase 3b — maps server `name` to the contributing plugin's
    /// reverse-DNS id for servers that came through
    /// [`merge_contributed`] / [`register_contributed`]. TOML-loaded
    /// entries do not appear here, so the host can refuse a plugin's
    /// `unregister_server` against a TOML-pinned name. Not persisted
    /// through serde — runtime state only.
    ///
    /// [`merge_contributed`]: Self::merge_contributed
    /// [`register_contributed`]: Self::register_contributed
    #[serde(default, skip)]
    pub contributed_by: std::collections::HashMap<String, String>,
}

/// P2-06 — `[timeouts]` block of `mcp.toml`. Every field is an
/// optional seconds override; a `None` falls through to the matching
/// `nexus_mcp::{client,server,auth}::DEFAULT_*` constant.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpTimeouts {
    /// Override for `nexus_mcp::client::DEFAULT_CONNECT_TIMEOUT` (15 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_secs: Option<u64>,
    /// Override for `nexus_mcp::client::DEFAULT_SHUTDOWN_TIMEOUT` (5 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown_secs: Option<u64>,
    /// Override for `nexus_mcp::server::DEFAULT_IPC_TIMEOUT` (30 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipc_secs: Option<u64>,
    /// Override for `nexus_mcp::server::DEFAULT_AI_IPC_TIMEOUT` (120 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_ipc_secs: Option<u64>,
    /// Override for `nexus_mcp::auth::DEFAULT_OAUTH_TIMEOUT` (30 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_secs: Option<u64>,
}

impl McpTimeouts {
    /// Used by `serde(skip_serializing_if)` so an empty `[timeouts]`
    /// block doesn't show up in serialised TOML round-trips.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
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
            Self::validate_spec(name, spec).map_err(|reason| McpConfigError::Invalid {
                path: source.to_string(),
                reason,
            })?;
        }
        Ok(())
    }

    /// Per-server semantic check: stdio entries need a non-empty `command`;
    /// remote (`http` / `websocket`) entries need a non-empty `url`. Returns
    /// a human-readable reason on failure so both [`validate`] and
    /// [`merge_contributed`] can build error messages without duplicating
    /// the rule set.
    fn validate_spec(name: &str, spec: &McpServerSpec) -> Result<(), String> {
        match spec.transport {
            McpTransport::Stdio => {
                if spec.command.trim().is_empty() {
                    return Err(format!("server '{name}' has empty command"));
                }
            }
            McpTransport::Http | McpTransport::Websocket => {
                let url = spec.url.as_deref().unwrap_or("").trim();
                if url.is_empty() {
                    return Err(format!(
                        "server '{name}' uses a remote transport but has no `url`"
                    ));
                }
            }
        }
        Ok(())
    }

    /// BL-113 / ADR 0027 — merge plugin-contributed MCP servers into the
    /// in-memory map. Each input triple is `(name, spec, plugin_id)`
    /// where `plugin_id` is the contributing plugin's reverse-DNS id.
    ///
    /// **TOML wins** on name collision — matches the LSP host's
    /// `merge_contributed` precedence and ADR 0027 §Migration's
    /// legacy-fallback stance. The returned [`Vec<McpMergeSkip>`]
    /// preserves input order; empty means every contribution was
    /// accepted.
    ///
    /// Contributed servers must pass the same per-entry validation as
    /// TOML-loaded ones (non-empty `command` for stdio, non-empty `url`
    /// for remote). Invalid contributions are reported as
    /// [`McpMergeSkipReason::Invalid`] with the validation message.
    pub fn merge_contributed(
        &mut self,
        contributions: Vec<(String, McpServerSpec, String)>,
    ) -> Vec<McpMergeSkip> {
        let mut skipped = Vec::new();
        for (name, spec, plugin_id) in contributions {
            if let Err(reason) = self.register_contributed(name.clone(), spec, plugin_id.clone()) {
                skipped.push(McpMergeSkip {
                    name,
                    plugin_id,
                    reason,
                });
            }
        }
        skipped
    }

    /// BL-113 Phase 3b — single-spec variant of [`merge_contributed`],
    /// the inner per-contribution rule the batch merge calls into and
    /// the entry point the `com.nexus.mcp.host::register_server` IPC
    /// verb dispatches to at runtime.
    ///
    /// Validates `name` and the per-transport rules (same as
    /// [`merge_contributed`]), refuses a name that any existing entry
    /// already owns (TOML or plugin-contributed alike — plugins must
    /// `unregister_server` before re-registering), inserts on success,
    /// and records the contributing plugin in
    /// [`contributed_by`](Self::contributed_by).
    ///
    /// # Errors
    /// Returns an [`McpMergeSkipReason`] when the spec fails validation
    /// or collides with an existing entry. On `Err`, the config is
    /// unchanged.
    pub fn register_contributed(
        &mut self,
        name: String,
        spec: McpServerSpec,
        plugin_id: String,
    ) -> Result<(), McpMergeSkipReason> {
        if name.trim().is_empty() {
            return Err(McpMergeSkipReason::InvalidName);
        }
        if let Err(reason) = Self::validate_spec(&name, &spec) {
            return Err(McpMergeSkipReason::Invalid(reason));
        }
        if self.servers.contains_key(&name) {
            return Err(McpMergeSkipReason::TomlOverride);
        }
        self.servers.insert(name.clone(), spec);
        self.contributed_by.insert(name, plugin_id);
        Ok(())
    }

    /// BL-113 Phase 3b — remove a previously contributed server. The
    /// `com.nexus.mcp.host::unregister_server` IPC verb's host entry
    /// point.
    ///
    /// `plugin_id` must match the contributing plugin recorded in
    /// [`contributed_by`](Self::contributed_by); this gates plugins
    /// from unregistering servers they don't own (including any
    /// TOML-pinned entry, which has no `contributed_by` row).
    ///
    /// # Errors
    /// Returns [`McpUnregisterError::NotFound`] when no server exists
    /// for `name`, [`McpUnregisterError::TomlEntry`] when the entry is
    /// TOML-loaded (not in `contributed_by`), and
    /// [`McpUnregisterError::NotOwnedByPlugin`] when the row exists
    /// but was contributed by a different plugin.
    pub fn unregister_contributed(
        &mut self,
        name: &str,
        plugin_id: &str,
    ) -> Result<McpServerSpec, McpUnregisterError> {
        match self.contributed_by.get(name) {
            None if self.servers.contains_key(name) => Err(McpUnregisterError::TomlEntry),
            None => Err(McpUnregisterError::NotFound),
            Some(owner) if owner != plugin_id => Err(McpUnregisterError::NotOwnedByPlugin {
                actual_owner: owner.clone(),
            }),
            Some(_) => {
                self.contributed_by.remove(name);
                self.servers
                    .remove(name)
                    .ok_or(McpUnregisterError::NotFound)
            }
        }
    }
}

/// Why [`McpHostConfig::unregister_contributed`] refused. Distinguishes
/// "this name was never registered" from "this name belongs to TOML /
/// another plugin" so the IPC layer can surface a precise reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpUnregisterError {
    /// No server exists under that name.
    NotFound,
    /// The server exists but came from `mcp.toml`, not a plugin
    /// contribution — plugins can't unregister TOML-pinned entries.
    TomlEntry,
    /// The server exists and was plugin-contributed, but the calling
    /// plugin isn't the one that contributed it.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually owns the entry.
        actual_owner: String,
    },
}

/// One contributed MCP server that did not make it into the merged
/// config. Same shape as `LspHostConfig`'s `MergeSkip` but the reason
/// variant includes a free-form `Invalid(String)` because MCP's
/// validation rules vary by transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpMergeSkip {
    /// The proposed server name (may be empty when
    /// [`McpMergeSkipReason::InvalidName`]).
    pub name: String,
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
    /// Reason the contribution was not accepted.
    pub reason: McpMergeSkipReason,
}

/// Skip reasons for [`McpHostConfig::merge_contributed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpMergeSkipReason {
    /// A TOML-loaded entry already owns this name.
    TomlOverride,
    /// `name` was empty / whitespace-only.
    InvalidName,
    /// Per-spec validation failed (empty stdio command, missing remote URL).
    /// Inner string is the human-readable reason.
    Invalid(String),
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
        assert_eq!(
            cfg.servers.get("fs").unwrap().transport,
            McpTransport::Stdio
        );
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

    // ── BL-113 / ADR 0027 — merge_contributed ──────────────────────────────────

    fn stdio_spec(command: &str) -> McpServerSpec {
        McpServerSpec {
            transport: McpTransport::Stdio,
            command: command.to_string(),
            ..McpServerSpec::default()
        }
    }

    fn http_spec(url: &str) -> McpServerSpec {
        McpServerSpec {
            transport: McpTransport::Http,
            url: Some(url.to_string()),
            ..McpServerSpec::default()
        }
    }

    #[test]
    fn merge_contributed_inserts_new_entries() {
        let mut cfg = McpHostConfig::default();
        let skipped = cfg.merge_contributed(vec![
            (
                "fs".into(),
                stdio_spec("filesystem-mcp"),
                "community.fs".into(),
            ),
            (
                "remote".into(),
                http_spec("https://example.com/mcp"),
                "community.remote".into(),
            ),
        ]);
        assert!(skipped.is_empty());
        assert_eq!(cfg.servers.len(), 2);
        assert!(cfg.servers.contains_key("fs"));
        assert!(cfg.servers.contains_key("remote"));
    }

    #[test]
    fn merge_contributed_toml_wins_on_collision() {
        let mut cfg = McpHostConfig::from_str(
            r#"
            [servers.fs]
            command = "fs-from-toml"
        "#,
            "inline",
        )
        .unwrap();
        let skipped = cfg.merge_contributed(vec![(
            "fs".into(),
            stdio_spec("fs-from-plugin"),
            "community.fs".into(),
        )]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "fs");
        assert_eq!(skipped[0].plugin_id, "community.fs");
        assert_eq!(skipped[0].reason, McpMergeSkipReason::TomlOverride);
        assert_eq!(cfg.servers["fs"].command, "fs-from-toml");
    }

    #[test]
    fn merge_contributed_rejects_invalid_specs() {
        let mut cfg = McpHostConfig::default();
        let skipped = cfg.merge_contributed(vec![
            ("".into(), stdio_spec("x"), "p1".into()),
            ("empty-cmd".into(), stdio_spec(""), "p2".into()),
            (
                "remote-no-url".into(),
                McpServerSpec {
                    transport: McpTransport::Http,
                    url: None,
                    ..McpServerSpec::default()
                },
                "p3".into(),
            ),
        ]);
        assert_eq!(skipped.len(), 3);
        assert_eq!(skipped[0].reason, McpMergeSkipReason::InvalidName);
        assert!(
            matches!(skipped[1].reason, McpMergeSkipReason::Invalid(ref r) if r.contains("empty command"))
        );
        assert!(
            matches!(skipped[2].reason, McpMergeSkipReason::Invalid(ref r) if r.contains("no `url`"))
        );
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn merge_contributed_preserves_input_order() {
        let mut cfg = McpHostConfig::from_str(
            r#"
            [servers.taken]
            command = "from-toml"
        "#,
            "inline",
        )
        .unwrap();
        let skipped = cfg.merge_contributed(vec![
            ("taken".into(), stdio_spec("p1"), "plug1".into()),
            ("new1".into(), stdio_spec("ok"), "plug2".into()),
            ("".into(), stdio_spec("oops"), "plug3".into()),
        ]);
        assert_eq!(skipped.len(), 2);
        assert_eq!(skipped[0].plugin_id, "plug1");
        assert_eq!(skipped[1].plugin_id, "plug3");
        assert_eq!(cfg.servers.len(), 2);
        assert!(cfg.servers.contains_key("new1"));
    }

    #[test]
    fn merge_contributed_populates_contributed_by_for_accepted_entries() {
        let mut cfg = McpHostConfig::default();
        cfg.servers.insert("toml-pinned".into(), stdio_spec("x"));
        let skipped = cfg.merge_contributed(vec![
            ("contrib-a".into(), stdio_spec("x"), "plugin.a".into()),
            ("contrib-b".into(), stdio_spec("y"), "plugin.b".into()),
            ("toml-pinned".into(), stdio_spec("y"), "plugin.c".into()),
        ]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(cfg.contributed_by.len(), 2);
        assert_eq!(cfg.contributed_by["contrib-a"], "plugin.a");
        assert_eq!(cfg.contributed_by["contrib-b"], "plugin.b");
        assert!(!cfg.contributed_by.contains_key("toml-pinned"));
    }

    // ── BL-113 Phase 3b — register_contributed / unregister_contributed ────────

    #[test]
    fn register_contributed_happy_path_inserts_and_records_provenance() {
        let mut cfg = McpHostConfig::default();
        assert!(cfg
            .register_contributed(
                "fs".into(),
                stdio_spec("filesystem-mcp"),
                "community.fs".into(),
            )
            .is_ok());
        assert_eq!(cfg.servers["fs"].command, "filesystem-mcp");
        assert_eq!(cfg.contributed_by["fs"], "community.fs");
    }

    #[test]
    fn register_contributed_rejects_invalid_and_collisions() {
        let mut cfg = McpHostConfig::default();
        cfg.servers.insert("taken".into(), stdio_spec("x"));
        assert_eq!(
            cfg.register_contributed("".into(), stdio_spec("ok"), "p".into())
                .unwrap_err(),
            McpMergeSkipReason::InvalidName,
        );
        assert!(matches!(
            cfg.register_contributed("empty".into(), stdio_spec(""), "p".into())
                .unwrap_err(),
            McpMergeSkipReason::Invalid(_),
        ));
        assert_eq!(
            cfg.register_contributed("taken".into(), stdio_spec("y"), "p".into())
                .unwrap_err(),
            McpMergeSkipReason::TomlOverride,
        );
        cfg.register_contributed("contrib".into(), stdio_spec("x"), "p1".into())
            .unwrap();
        assert_eq!(
            cfg.register_contributed("contrib".into(), stdio_spec("y"), "p2".into())
                .unwrap_err(),
            McpMergeSkipReason::TomlOverride,
        );
        assert_eq!(cfg.servers.len(), 2);
        assert_eq!(cfg.contributed_by.len(), 1);
        assert_eq!(cfg.contributed_by["contrib"], "p1");
    }

    #[test]
    fn unregister_contributed_removes_when_owner_matches() {
        let mut cfg = McpHostConfig::default();
        cfg.register_contributed(
            "fs".into(),
            stdio_spec("filesystem-mcp"),
            "community.fs".into(),
        )
        .unwrap();
        let removed = cfg.unregister_contributed("fs", "community.fs").unwrap();
        assert_eq!(removed.command, "filesystem-mcp");
        assert!(!cfg.servers.contains_key("fs"));
        assert!(!cfg.contributed_by.contains_key("fs"));
    }

    #[test]
    fn unregister_contributed_distinguishes_not_found_toml_and_wrong_owner() {
        let mut cfg = McpHostConfig::default();
        cfg.servers.insert("toml".into(), stdio_spec("x"));
        cfg.register_contributed("contrib".into(), stdio_spec("x"), "plugin.owner".into())
            .unwrap();
        assert_eq!(
            cfg.unregister_contributed("ghost", "anyone").unwrap_err(),
            McpUnregisterError::NotFound,
        );
        assert_eq!(
            cfg.unregister_contributed("toml", "anyone").unwrap_err(),
            McpUnregisterError::TomlEntry,
        );
        match cfg.unregister_contributed("contrib", "plugin.intruder") {
            Err(McpUnregisterError::NotOwnedByPlugin { actual_owner }) => {
                assert_eq!(actual_owner, "plugin.owner");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
        assert!(cfg.servers.contains_key("toml"));
        assert!(cfg.servers.contains_key("contrib"));
        assert_eq!(cfg.contributed_by["contrib"], "plugin.owner");
    }
}
