//! BL-113 Phase 2b — call `com.nexus.lsp::register_server` /
//! `unregister_server` once per LSP contribution declared in plugin
//! manifests.
//!
//! Mirrors [`crate::dap_contribution_wiring`] layer-for-layer; see
//! that module for the design rationale. The `nexus-lsp` host's
//! runtime register/unregister IPC verbs (Phase 2b) accept one server
//! at a time; bootstrap-side wiring iterates aggregated contributions
//! from [`nexus_plugins::collect_contributions`], converts them
//! through [`crate::protocol_host_specs::lsp_contributions_to_specs`],
//! and dispatches one IPC call per `(spec, plugin_id)` pair.
//!
//! Failure is per-server: a single rejection or transport error is
//! logged as an [`LspWireOutcome`] and the wire pass continues.

use std::time::Duration;

use nexus_kernel::{Identity as _, Ipc as _, KernelPluginContext};
use nexus_lsp::LspServerSpec;
use nexus_plugins::{collect_contributions, PluginManifest};
use serde_json::{json, Value};

use crate::protocol_host_specs::lsp_contributions_to_specs;

/// Per-call timeout for `register_server` / `unregister_server`. The
/// host's sync dispatch arm only takes a write lock on the in-memory
/// server map; no spawn or I/O. A 5s ceiling is generous.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

const LSP_PLUGIN_ID: &str = "com.nexus.lsp";
const REGISTER_COMMAND: &str = "register_server";
const UNREGISTER_COMMAND: &str = "unregister_server";

/// Per-server outcome of a wire / unwire round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspWireOutcome {
    /// Reverse-DNS id of the plugin that contributed the server.
    pub plugin_id: String,
    /// Server `name` (the contribution's stable id).
    pub server_name: String,
    /// Host reply status — see [`LspWireStatus`].
    pub status: LspWireStatus,
}

/// Outcome of a single `register_server` / `unregister_server` IPC
/// call. The `Ok` variants map 1:1 to the host's `status` reply field
/// strings; the trailing variants cover transport / payload errors
/// that can't be expressed in the host's reply contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspWireStatus {
    /// Host accepted the contribution.
    Ok,
    /// Host refused — a TOML entry or earlier contribution already
    /// owns this `name`. Per ADR 0027 §Migration the TOML wins.
    TomlOverride,
    /// Host refused — `name` was empty / whitespace-only.
    InvalidName,
    /// Host refused — `command` was empty / whitespace-only.
    InvalidCommand,
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

/// Wire every LSP contribution found across `manifests` by issuing a
/// `com.nexus.lsp::register_server` IPC call for each server spec.
///
/// Iteration order follows the order of `manifests`, matching how
/// [`collect_contributions`] aggregates them. Returns a per-server
/// outcome list in the same order, so the caller can render an
/// "accepted N, skipped M" diagnostic.
///
/// Errors are per-server — a single rejection or transport failure
/// doesn't short-circuit the rest of the pass.
pub async fn wire_lsp_contributions(
    context: &KernelPluginContext,
    manifests: &[&PluginManifest],
) -> Vec<LspWireOutcome> {
    let set = collect_contributions(manifests.iter().copied());
    let specs = lsp_contributions_to_specs(&set);
    let mut outcomes = Vec::with_capacity(specs.len());
    for (spec, plugin_id) in specs {
        let server_name = spec.name.clone();
        let args = register_server_args(&spec, &plugin_id);
        let status = match context
            .ipc_call(LSP_PLUGIN_ID, REGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_register_reply(&reply),
            Err(e) => LspWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(LspWireOutcome {
            plugin_id,
            server_name,
            status,
        });
    }
    outcomes
}

/// Unwire every LSP contribution from a single plugin's manifest by
/// issuing `com.nexus.lsp::unregister_server` once per declared
/// server. Used at plugin disable / shutdown to roll back what
/// [`wire_lsp_contributions`] previously registered.
///
/// Iteration order follows the manifest's declaration order.
pub async fn unwire_lsp_contributions_for_plugin(
    context: &KernelPluginContext,
    manifest: &PluginManifest,
) -> Vec<LspWireOutcome> {
    let mut outcomes = Vec::with_capacity(manifest.registrations.protocol_hosts.lsp.len());
    for entry in &manifest.registrations.protocol_hosts.lsp {
        let args = json!({
            "name": entry.id,
            "plugin_id": manifest.id,
        });
        let status = match context
            .ipc_call(LSP_PLUGIN_ID, UNREGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_unregister_reply(&reply),
            Err(e) => LspWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(LspWireOutcome {
            plugin_id: manifest.id.clone(),
            server_name: entry.id.clone(),
            status,
        });
    }
    outcomes
}

fn register_server_args(spec: &LspServerSpec, plugin_id: &str) -> Value {
    json!({
        "name": spec.name,
        "command": spec.command,
        "args": spec.args,
        "file_types": spec.file_types,
        "root_markers": spec.root_markers,
        "disabled": spec.disabled,
        "env": spec.env,
        "plugin_id": plugin_id,
    })
}

fn decode_register_reply(reply: &Value) -> LspWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => LspWireStatus::Ok,
        Some("toml_override") => LspWireStatus::TomlOverride,
        Some("invalid_name") => LspWireStatus::InvalidName,
        Some("invalid_command") => LspWireStatus::InvalidCommand,
        Some(other) => LspWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => LspWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

fn decode_unregister_reply(reply: &Value) -> LspWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => LspWireStatus::Ok,
        Some("not_found") => LspWireStatus::NotFound,
        Some("toml_entry") => LspWireStatus::TomlEntry,
        Some("not_owned_by_plugin") => {
            let actual_owner = reply
                .get("actual_owner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            LspWireStatus::NotOwnedByPlugin { actual_owner }
        }
        Some(other) => LspWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => LspWireStatus::UnexpectedReply("missing `status` field".to_string()),
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
            LspWireStatus::Ok,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "toml_override" })),
            LspWireStatus::TomlOverride,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_name" })),
            LspWireStatus::InvalidName,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_command" })),
            LspWireStatus::InvalidCommand,
        );
        assert!(matches!(
            decode_register_reply(&json!({ "ok": false, "status": "wat" })),
            LspWireStatus::UnexpectedReply(_),
        ));
        assert!(matches!(
            decode_register_reply(&json!({})),
            LspWireStatus::UnexpectedReply(_),
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
            LspWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.someone-else");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn decode_unregister_reply_handles_missing_actual_owner_gracefully() {
        let reply = json!({ "ok": false, "status": "not_owned_by_plugin" });
        match decode_unregister_reply(&reply) {
            LspWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert!(actual_owner.is_empty());
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn register_server_args_carries_every_protocol_field() {
        let spec = LspServerSpec {
            name: "rust-analyzer".into(),
            command: "rust-analyzer".into(),
            args: vec!["--log".into(), "info".into()],
            file_types: vec!["rs".into()],
            root_markers: vec!["Cargo.toml".into()],
            disabled: false,
            env: std::iter::once(("RUST_LOG".to_string(), "trace".to_string())).collect(),
        };
        let args = register_server_args(&spec, "community.rust");
        assert_eq!(args["name"], "rust-analyzer");
        assert_eq!(args["command"], "rust-analyzer");
        assert_eq!(args["args"], json!(["--log", "info"]));
        assert_eq!(args["file_types"], json!(["rs"]));
        assert_eq!(args["root_markers"], json!(["Cargo.toml"]));
        assert_eq!(args["disabled"], false);
        assert_eq!(args["env"]["RUST_LOG"], "trace");
        assert_eq!(args["plugin_id"], "community.rust");
    }
}
