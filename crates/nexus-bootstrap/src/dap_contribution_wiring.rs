//! BL-113 Phase 1c — call `com.nexus.dap::register_adapter` /
//! `unregister_adapter` once per DAP contribution declared in plugin
//! manifests.
//!
//! The `nexus-dap` host's runtime register/unregister IPC verbs (Phase
//! 1b) accept one adapter at a time. Bootstrap-side wiring iterates
//! aggregated contributions from
//! [`nexus_plugins::collect_contributions`], converts them through
//! [`crate::protocol_host_specs::dap_contributions_to_specs`], and
//! dispatches one IPC call per `(spec, plugin_id)` pair.
//!
//! Failure is per-adapter: a single rejection or transport error is
//! logged as a [`DapWireOutcome`] and the wire pass continues. The
//! caller chooses what to do with the report (panic, warn, surface in
//! a status panel, etc.).
//!
//! The same module covers the inverse direction:
//! [`unwire_dap_contributions_for_plugin`] is the symmetric pass for
//! plugin disable / shutdown.

use std::time::Duration;

use nexus_dap::DapAdapterSpec;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{collect_contributions, PluginManifest};
use serde_json::{json, Value};

use crate::protocol_host_specs::dap_contributions_to_specs;

/// Per-call timeout for `register_adapter` / `unregister_adapter`. The
/// host's sync dispatch arm only takes a write lock on the in-memory
/// adapter map; no spawn or I/O. A 5s ceiling is generous.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

const DAP_PLUGIN_ID: &str = "com.nexus.dap";
const REGISTER_COMMAND: &str = "register_adapter";
const UNREGISTER_COMMAND: &str = "unregister_adapter";

/// Per-adapter outcome of a wire / unwire round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DapWireOutcome {
    /// Reverse-DNS id of the plugin that contributed the adapter.
    pub plugin_id: String,
    /// Adapter `name` (the contribution's stable id).
    pub adapter_name: String,
    /// Host reply status — see [`DapWireStatus`].
    pub status: DapWireStatus,
}

/// Outcome of a single `register_adapter` / `unregister_adapter` IPC
/// call. The `Ok` variants map 1:1 to the host's `status` reply field
/// strings; the trailing variants cover transport / payload errors
/// that can't be expressed in the host's reply contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DapWireStatus {
    /// Host accepted the contribution.
    Ok,
    /// Host refused — a TOML entry or earlier contribution already
    /// owns this `name`. Per ADR 0027 §Migration the TOML wins.
    TomlOverride,
    /// Host refused — `name` was empty / whitespace-only.
    InvalidName,
    /// Host refused — `command` was empty / whitespace-only.
    InvalidCommand,
    /// `unregister_adapter` only: no adapter exists under that name.
    NotFound,
    /// `unregister_adapter` only: the name is TOML-pinned; plugins
    /// cannot unregister TOML entries.
    TomlEntry,
    /// `unregister_adapter` only: the name was contributed by a
    /// different plugin. The `actual_owner` is the reverse-DNS id of
    /// the real contributor.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually contributed the
        /// adapter — useful for diagnostics when a unregister fails
        /// because of a contributor mismatch.
        actual_owner: String,
    },
    /// The IPC call itself failed (plugin not registered, timeout,
    /// crash, capability denial, …).
    DispatchError(String),
    /// The reply payload didn't match the host's documented shape
    /// (missing `status`, unknown status string, etc.).
    UnexpectedReply(String),
}

/// Wire every DAP contribution found across `manifests` by issuing a
/// `com.nexus.dap::register_adapter` IPC call for each adapter spec.
///
/// Iteration order follows the order of `manifests`, matching how
/// [`collect_contributions`] aggregates them. Returns a per-adapter
/// outcome list in the same order, so the caller can render an
/// "accepted N, skipped M" diagnostic.
///
/// Errors are per-adapter — a single rejection or transport failure
/// doesn't short-circuit the rest of the pass.
pub async fn wire_dap_contributions(
    context: &KernelPluginContext,
    manifests: &[&PluginManifest],
) -> Vec<DapWireOutcome> {
    let set = collect_contributions(manifests.iter().copied());
    let specs = dap_contributions_to_specs(&set);
    let mut outcomes = Vec::with_capacity(specs.len());
    for (spec, plugin_id) in specs {
        let adapter_name = spec.name.clone();
        let args = register_adapter_args(&spec, &plugin_id);
        let status = match context
            .ipc_call(DAP_PLUGIN_ID, REGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_register_reply(&reply),
            Err(e) => DapWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(DapWireOutcome {
            plugin_id,
            adapter_name,
            status,
        });
    }
    outcomes
}

/// Unwire every DAP contribution from a single plugin's manifest by
/// issuing `com.nexus.dap::unregister_adapter` once per declared
/// adapter. Used at plugin disable / shutdown to roll back what
/// [`wire_dap_contributions`] previously registered.
///
/// Iteration order follows the manifest's declaration order.
pub async fn unwire_dap_contributions_for_plugin(
    context: &KernelPluginContext,
    manifest: &PluginManifest,
) -> Vec<DapWireOutcome> {
    let mut outcomes = Vec::with_capacity(manifest.registrations.protocol_hosts.dap.len());
    for entry in &manifest.registrations.protocol_hosts.dap {
        let args = json!({
            "name": entry.id,
            "plugin_id": manifest.id,
        });
        let status = match context
            .ipc_call(DAP_PLUGIN_ID, UNREGISTER_COMMAND, args, REGISTER_TIMEOUT)
            .await
        {
            Ok(reply) => decode_unregister_reply(&reply),
            Err(e) => DapWireStatus::DispatchError(e.to_string()),
        };
        outcomes.push(DapWireOutcome {
            plugin_id: manifest.id.clone(),
            adapter_name: entry.id.clone(),
            status,
        });
    }
    outcomes
}

fn register_adapter_args(spec: &DapAdapterSpec, plugin_id: &str) -> Value {
    json!({
        "name": spec.name,
        "command": spec.command,
        "args": spec.args,
        "adapter_type": spec.adapter_type,
        "file_types": spec.file_types,
        "disabled": spec.disabled,
        "env": spec.env,
        "plugin_id": plugin_id,
        "metadata": spec.metadata,
    })
}

fn decode_register_reply(reply: &Value) -> DapWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => DapWireStatus::Ok,
        Some("toml_override") => DapWireStatus::TomlOverride,
        Some("invalid_name") => DapWireStatus::InvalidName,
        Some("invalid_command") => DapWireStatus::InvalidCommand,
        Some(other) => DapWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => DapWireStatus::UnexpectedReply("missing `status` field".to_string()),
    }
}

fn decode_unregister_reply(reply: &Value) -> DapWireStatus {
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => DapWireStatus::Ok,
        Some("not_found") => DapWireStatus::NotFound,
        Some("toml_entry") => DapWireStatus::TomlEntry,
        Some("not_owned_by_plugin") => {
            let actual_owner = reply
                .get("actual_owner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            DapWireStatus::NotOwnedByPlugin { actual_owner }
        }
        Some(other) => DapWireStatus::UnexpectedReply(format!("unknown status: {other}")),
        None => DapWireStatus::UnexpectedReply("missing `status` field".to_string()),
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
            DapWireStatus::Ok,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "toml_override" })),
            DapWireStatus::TomlOverride,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_name" })),
            DapWireStatus::InvalidName,
        );
        assert_eq!(
            decode_register_reply(&json!({ "ok": false, "status": "invalid_command" })),
            DapWireStatus::InvalidCommand,
        );
        assert!(matches!(
            decode_register_reply(&json!({ "ok": false, "status": "wat" })),
            DapWireStatus::UnexpectedReply(_),
        ));
        assert!(matches!(
            decode_register_reply(&json!({})),
            DapWireStatus::UnexpectedReply(_),
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
            DapWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.someone-else");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn decode_unregister_reply_handles_missing_actual_owner_gracefully() {
        // Defensive — the host *should* include actual_owner, but if
        // a future refactor drops the field we surface an empty
        // string rather than crashing.
        let reply = json!({ "ok": false, "status": "not_owned_by_plugin" });
        match decode_unregister_reply(&reply) {
            DapWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert!(actual_owner.is_empty());
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    #[test]
    fn register_adapter_args_carries_every_protocol_field() {
        let spec = DapAdapterSpec {
            name: "rust".into(),
            command: "codelldb".into(),
            args: vec!["--port".into(), "0".into()],
            adapter_type: Some("lldb".into()),
            file_types: vec!["rs".into()],
            disabled: false,
            env: std::iter::once(("RUST_BACKTRACE".to_string(), "1".to_string())).collect(),
            metadata: None,
        };
        let args = register_adapter_args(&spec, "community.rust");
        assert_eq!(args["name"], "rust");
        assert_eq!(args["command"], "codelldb");
        assert_eq!(args["args"], json!(["--port", "0"]));
        assert_eq!(args["adapter_type"], "lldb");
        assert_eq!(args["file_types"], json!(["rs"]));
        assert_eq!(args["disabled"], false);
        assert_eq!(args["env"]["RUST_BACKTRACE"], "1");
        assert_eq!(args["plugin_id"], "community.rust");
        // `metadata` is absent on the spec → forwarded as JSON null.
        assert_eq!(args["metadata"], json!(null));
    }

    #[test]
    fn register_adapter_args_forwards_metadata_verbatim() {
        // BL-113 — opaque `metadata` (shell-only fields packed by
        // `dap_contribution_to_spec`) flows through the wire-args
        // helper untouched so the host can round-trip it on
        // `list_adapters` for the shell launch form.
        let spec = DapAdapterSpec {
            name: "rust".into(),
            command: "codelldb".into(),
            args: vec![],
            adapter_type: None,
            file_types: vec![],
            disabled: false,
            env: Default::default(),
            metadata: Some(json!({
                "plugin_id": "community.rust",
                "display_name": "Rust (codelldb)",
                "launch_config_schema": "./launch.schema.json",
                "root_markers": ["Cargo.toml"],
            })),
        };
        let args = register_adapter_args(&spec, "community.rust");
        assert_eq!(args["metadata"]["display_name"], "Rust (codelldb)");
        assert_eq!(
            args["metadata"]["launch_config_schema"],
            "./launch.schema.json",
        );
        assert_eq!(args["metadata"]["root_markers"], json!(["Cargo.toml"]));
    }
}
