//! BL-113 Phase 4 — call `com.nexus.acp::register_server` /
//! `unregister_server` once per ACP contribution declared in plugin
//! manifests.
//!
//! Mirrors [`crate::lsp_contribution_wiring`] /
//! [`crate::dap_contribution_wiring`] /
//! [`crate::mcp_contribution_wiring`] layer-for-layer. ACP differs in
//! exactly one detail: there is no `acp.toml` flat-TOML class (ADR 0027
//! §Phase 4 lands ACP greenfield under the contribution model), so the
//! host-reply `"already_registered"` status replaces LSP/DAP/MCP's
//! `"toml_override"`.
//!
//! Failure is per-adapter: a single rejection or transport error is
//! logged as an [`AcpWireOutcome`] and the wire pass continues.

use std::time::Duration;

use nexus_acp::AcpAdapterSpec;
use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::{collect_contributions, PluginManifest};
use serde_json::{json, Value};

use crate::protocol_host_specs::acp_contributions_to_specs;

/// Per-call timeout for `register_server` / `unregister_server`. The
/// host's sync dispatch arm only takes a write lock on the in-memory
/// adapter map; no spawn or I/O. A 5s ceiling is generous.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

const ACP_PLUGIN_ID: &str = "com.nexus.acp";
const REGISTER_COMMAND: &str = "register_server";
const UNREGISTER_COMMAND: &str = "unregister_server";

/// Per-adapter outcome of a wire / unwire round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpWireOutcome {
    /// Reverse-DNS id of the plugin that contributed the adapter.
    pub plugin_id: String,
    /// Adapter `name` (the contribution's stable id).
    pub agent_name: String,
    /// Host reply status — see [`AcpWireStatus`].
    pub status: AcpWireStatus,
}

/// Outcome of a single `register_server` / `unregister_server` IPC
/// call. The `Ok` variants map 1:1 to the host's `status` reply field
/// strings; the trailing variants cover transport / payload errors
/// that can't be expressed in the host's reply contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpWireStatus {
    /// Host accepted the contribution.
    Ok,
    /// Host refused — another contribution already owns the `name`.
    AlreadyRegistered,
    /// Host refused — `name` was empty / whitespace-only.
    InvalidName,
    /// Host refused — `command` was empty / whitespace-only.
    InvalidCommand,
    /// `unregister_server` only: no adapter exists under that name.
    NotFound,
    /// `unregister_server` only: the name was contributed by a
    /// different plugin. The `actual_owner` is the reverse-DNS id of
    /// the real contributor.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually contributed the
        /// adapter.
        actual_owner: String,
    },
    /// The IPC call itself failed (plugin not registered, timeout,
    /// crash, capability denial, …).
    DispatchError(String),
    /// The reply payload didn't match the host's documented shape.
    UnexpectedReply(String),
}

/// Wire every ACP contribution found across `manifests` by issuing a
/// `com.nexus.acp::register_server` IPC call for each adapter spec.
///
/// Iteration order follows the order of `manifests`. Errors are
/// per-adapter — a single rejection or transport failure doesn't
/// short-circuit the rest of the pass.
pub async fn wire_acp_contributions(
    context: &KernelPluginContext,
    manifests: &[&PluginManifest],
) -> Vec<AcpWireOutcome> {
    let set = collect_contributions(manifests.iter().copied());
    let specs = acp_contributions_to_specs(&set);
    let mut outcomes = Vec::with_capacity(specs.len());
    for (spec, plugin_id) in specs {
        let agent_name = spec.name.clone();
        let args = register_server_args(&spec, &plugin_id);
        let status = match context
            .ipc_call(ACP_PLUGIN_ID, REGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_register_reply(&reply),
            Err(e) => AcpWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(AcpWireOutcome {
            plugin_id,
            agent_name,
            status,
        });
    }
    outcomes
}

/// Unwire every ACP contribution from a single plugin's manifest by
/// issuing `com.nexus.acp::unregister_server` once per declared
/// adapter. Used at plugin disable / shutdown.
pub async fn unwire_acp_contributions_for_plugin(
    context: &KernelPluginContext,
    manifest: &PluginManifest,
) -> Vec<AcpWireOutcome> {
    let mut outcomes = Vec::with_capacity(manifest.registrations.protocol_hosts.acp.len());
    for entry in &manifest.registrations.protocol_hosts.acp {
        let args = json!({
            "name": entry.id,
            "plugin_id": manifest.id,
        });
        let status = match context
            .ipc_call(ACP_PLUGIN_ID, UNREGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_unregister_reply(&reply),
            Err(e) => AcpWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(AcpWireOutcome {
            plugin_id: manifest.id.clone(),
            agent_name: entry.id.clone(),
            status,
        });
    }
    outcomes
}

fn register_server_args(spec: &AcpAdapterSpec, plugin_id: &str) -> Value {
    json!({
        "name": spec.name,
        "command": spec.command,
        "args": spec.args,
        "capabilities": spec.capabilities,
        "disabled": spec.disabled,
        "env": spec.env,
        "metadata": spec.metadata,
        "plugin_id": plugin_id,
    })
}

fn decode_register_reply(reply: &Value) -> AcpWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => AcpWireStatus::Ok,
        Some("already_registered") => AcpWireStatus::AlreadyRegistered,
        Some("invalid_name") => AcpWireStatus::InvalidName,
        Some("invalid_command") => AcpWireStatus::InvalidCommand,
        Some(other) => AcpWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => AcpWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

fn decode_unregister_reply(reply: &Value) -> AcpWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => AcpWireStatus::Ok,
        Some("not_found") => AcpWireStatus::NotFound,
        Some("not_owned_by_plugin") => {
            let actual_owner = reply
                .get("actual_owner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            AcpWireStatus::NotOwnedByPlugin { actual_owner }
        }
        Some(other) => AcpWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => AcpWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn decode_register_reply_maps_every_status_string() {
        assert_eq!(
            decode_register_reply(&json!({ "ok": true, "status": "ok" })),
            AcpWireStatus::Ok,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "already_registered" })),
            AcpWireStatus::AlreadyRegistered,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_name" })),
            AcpWireStatus::InvalidName,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_command" })),
            AcpWireStatus::InvalidCommand,
        );
        assert!(matches!(
            decode_register_reply(&json!({ "ok": false, "status": "wat" })),
            AcpWireStatus::UnexpectedReply(_),
        ));
        assert!(matches!(
            decode_register_reply(&json!({})),
            AcpWireStatus::UnexpectedReply(_),
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
            AcpWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.someone-else");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn decode_unregister_reply_handles_missing_actual_owner_gracefully() {
        let reply = json!({ "ok": false, "status": "not_owned_by_plugin" });
        match decode_unregister_reply(&reply) {
            AcpWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert!(actual_owner.is_empty());
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn register_server_args_carries_every_protocol_field() {
        let spec = AcpAdapterSpec {
            name: "hermes".into(),
            command: "hermes-agent".into(),
            args: vec!["--stdio".into()],
            capabilities: vec!["delegate".into(), "tools".into()],
            disabled: false,
            env: {
                let mut m = HashMap::new();
                m.insert("HERMES_LOG".to_string(), "info".to_string());
                m
            },
            metadata: Some(json!({"plugin_id": "community.hermes", "display_name": "Hermes"})),
        };
        let args = register_server_args(&spec, "community.hermes");
        assert_eq!(args["name"], "hermes");
        assert_eq!(args["command"], "hermes-agent");
        assert_eq!(args["args"], json!(["--stdio"]));
        assert_eq!(args["capabilities"], json!(["delegate", "tools"]));
        assert_eq!(args["disabled"], false);
        assert_eq!(args["env"]["HERMES_LOG"], "info");
        assert_eq!(args["metadata"]["plugin_id"], "community.hermes");
        assert_eq!(args["plugin_id"], "community.hermes");
    }
}
