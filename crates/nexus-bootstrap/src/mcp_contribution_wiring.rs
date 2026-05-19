//! BL-113 Phase 3b — call `com.nexus.mcp.host::register_server` /
//! `unregister_server` once per MCP contribution declared in plugin
//! manifests.
//!
//! Mirrors [`crate::dap_contribution_wiring`] and
//! [`crate::lsp_contribution_wiring`] layer-for-layer; see those
//! modules for the design rationale. MCP uses a `(name, spec,
//! plugin_id)` triple at the conversion boundary because the host
//! keeps a `BTreeMap<String, McpServerSpec>` keyed on server name.
//!
//! Failure is per-server: a single rejection or transport error is
//! logged as an [`McpWireOutcome`] and the wire pass continues.

use std::time::Duration;

use nexus_kernel::{Identity as _, Ipc as _, KernelPluginContext};
use nexus_mcp::{McpServerSpec, McpTransport};
use nexus_plugins::{collect_contributions, PluginManifest};
use serde_json::{json, Value};

use crate::protocol_host_specs::mcp_contributions_to_specs;

/// Per-call timeout for `register_server` / `unregister_server`. The
/// host's sync dispatch arm only takes a write lock on the in-memory
/// server map; no spawn or I/O. A 5s ceiling is generous.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

const MCP_PLUGIN_ID: &str = "com.nexus.mcp.host";
const REGISTER_COMMAND: &str = "register_server";
const UNREGISTER_COMMAND: &str = "unregister_server";

/// Per-server outcome of a wire / unwire round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpWireOutcome {
    /// Reverse-DNS id of the plugin that contributed the server.
    pub plugin_id: String,
    /// Server `name` (the contribution's stable id).
    pub server_name: String,
    /// Host reply status — see [`McpWireStatus`].
    pub status: McpWireStatus,
}

/// Outcome of a single `register_server` / `unregister_server` IPC
/// call. The `Ok` variants map 1:1 to the host's `status` reply field
/// strings; the trailing variants cover transport / payload errors
/// that can't be expressed in the host's reply contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpWireStatus {
    /// Host accepted the contribution.
    Ok,
    /// Host refused — a TOML entry or earlier contribution already
    /// owns this `name`. Per ADR 0027 §Migration the TOML wins.
    TomlOverride,
    /// Host refused — `name` was empty / whitespace-only.
    InvalidName,
    /// Host refused — per-spec validation failed (empty stdio
    /// command, missing remote URL, …). Inner string is the
    /// human-readable reason from the host's validator.
    Invalid(String),
    /// `unregister_server` only: no server exists under that name.
    NotFound,
    /// `unregister_server` only: the name is TOML-pinned; plugins
    /// cannot unregister TOML entries.
    TomlEntry,
    /// `unregister_server` only: the name was contributed by a
    /// different plugin. The `actual_owner` is the reverse-DNS id of
    /// the real contributor.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually contributed the
        /// server.
        actual_owner: String,
    },
    /// The IPC call itself failed (plugin not registered, timeout,
    /// crash, capability denial, …).
    DispatchError(String),
    /// The reply payload didn't match the host's documented shape
    /// (missing `status`, unknown status string, etc.).
    UnexpectedReply(String),
}

/// Wire every MCP contribution found across `manifests` by issuing a
/// `com.nexus.mcp.host::register_server` IPC call for each server spec.
///
/// Iteration order follows the order of `manifests`, matching how
/// [`collect_contributions`] aggregates them. Returns a per-server
/// outcome list in the same order, so the caller can render an
/// "accepted N, skipped M" diagnostic.
///
/// Errors are per-server — a single rejection or transport failure
/// doesn't short-circuit the rest of the pass.
pub async fn wire_mcp_contributions(
    context: &KernelPluginContext,
    manifests: &[&PluginManifest],
) -> Vec<McpWireOutcome> {
    let set = collect_contributions(manifests.iter().copied());
    let specs = mcp_contributions_to_specs(&set);
    let mut outcomes = Vec::with_capacity(specs.len());
    for (name, spec, plugin_id) in specs {
        let args = register_server_args(&name, &spec, &plugin_id);
        let status = match context
            .ipc_call(MCP_PLUGIN_ID, REGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_register_reply(&reply),
            Err(e) => McpWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(McpWireOutcome {
            plugin_id,
            server_name: name,
            status,
        });
    }
    outcomes
}

/// Unwire every MCP contribution from a single plugin's manifest by
/// issuing `com.nexus.mcp.host::unregister_server` once per declared
/// server.
///
/// Iteration order follows the manifest's declaration order.
pub async fn unwire_mcp_contributions_for_plugin(
    context: &KernelPluginContext,
    manifest: &PluginManifest,
) -> Vec<McpWireOutcome> {
    let mut outcomes = Vec::with_capacity(manifest.registrations.protocol_hosts.mcp.len());
    for entry in &manifest.registrations.protocol_hosts.mcp {
        let args = json!({
            "name": entry.id,
            "plugin_id": manifest.id,
        });
        let status = match context
            .ipc_call(MCP_PLUGIN_ID, UNREGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_unregister_reply(&reply),
            Err(e) => McpWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(McpWireOutcome {
            plugin_id: manifest.id.clone(),
            server_name: entry.id.clone(),
            status,
        });
    }
    outcomes
}

fn transport_to_str(t: McpTransport) -> &'static str {
    match t {
        McpTransport::Stdio => "stdio",
        McpTransport::Http => "http",
        McpTransport::Websocket => "websocket",
    }
}

fn register_server_args(name: &str, spec: &McpServerSpec, plugin_id: &str) -> Value {
    json!({
        "name": name,
        "transport": transport_to_str(spec.transport),
        "command": spec.command,
        "args": spec.args,
        "env": spec.env,
        "url": spec.url,
        "disabled": spec.disabled,
        "plugin_id": plugin_id,
    })
}

fn decode_register_reply(reply: &Value) -> McpWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => McpWireStatus::Ok,
        Some("toml_override") => McpWireStatus::TomlOverride,
        Some("invalid_name") => McpWireStatus::InvalidName,
        Some("invalid") => {
            let reason = reply
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            McpWireStatus::Invalid(reason)
        }
        Some(other) => McpWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => McpWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

fn decode_unregister_reply(reply: &Value) -> McpWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => McpWireStatus::Ok,
        Some("not_found") => McpWireStatus::NotFound,
        Some("toml_entry") => McpWireStatus::TomlEntry,
        Some("not_owned_by_plugin") => {
            let actual_owner = reply
                .get("actual_owner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            McpWireStatus::NotOwnedByPlugin { actual_owner }
        }
        Some(other) => McpWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => McpWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_register_reply_maps_every_status_string() {
        assert_eq!(
            decode_register_reply(&json!({ "ok": true, "status": "ok" })),
            McpWireStatus::Ok,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "toml_override" })),
            McpWireStatus::TomlOverride,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_name" })),
            McpWireStatus::InvalidName,
        );
        match decode_register_reply(
            &json!({ "ok": false, "status": "invalid", "reason": "empty command" }),
        ) {
            McpWireStatus::Invalid(r) => assert_eq!(r, "empty command"),
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert!(matches!(
            decode_register_reply(&json!({ "ok": false, "status": "wat" })),
            McpWireStatus::UnexpectedReply(_),
        ));
    }

    #[test]
    fn decode_unregister_reply_carries_actual_owner() {
        let reply = json!({
            "ok": false,
            "status": "not_owned_by_plugin",
            "actual_owner": "community.someone-else",
        });
        match decode_unregister_reply(&reply) {
            McpWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.someone-else");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn register_server_args_carries_every_protocol_field() {
        let spec = McpServerSpec {
            transport: McpTransport::Stdio,
            command: "filesystem-mcp".to_string(),
            args: vec!["--root".to_string(), ".".to_string()],
            ..McpServerSpec::default()
        };
        let args = register_server_args("fs", &spec, "community.fs");
        assert_eq!(args["name"], "fs");
        assert_eq!(args["transport"], "stdio");
        assert_eq!(args["command"], "filesystem-mcp");
        assert_eq!(args["args"], json!(["--root", "."]));
        assert_eq!(args["plugin_id"], "community.fs");
    }

    #[test]
    fn register_server_args_serialises_http_transport_with_url() {
        let spec = McpServerSpec {
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".into()),
            ..McpServerSpec::default()
        };
        let args = register_server_args("remote", &spec, "community.remote");
        assert_eq!(args["transport"], "http");
        assert_eq!(args["url"], "https://example.com/mcp");
    }
}
