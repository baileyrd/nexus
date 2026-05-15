//! Wire-mirror IPC types for `com.nexus.acp`.
//!
//! The handlers in [`crate::core_plugin`] build replies with ad-hoc
//! `serde_json::json!` macros — same shape as `nexus-lsp::ipc` and
//! `nexus-dap::ipc`. This module gives the schema generator + the
//! shell something concrete to consume, and is the canonical IPC
//! drift target for `scripts/check_ipc_drift.sh`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Args for `initialize` (handler `2`). Triggers a lazy connect for
/// `agent` and returns the agent-reported capabilities object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpAgentArgs {
    /// Stable agent identifier (the contributing manifest's `id`).
    pub agent: String,
}

/// Args for `propose` (handler `3`). The `params` blob is agent-
/// specific; the host forwards it verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpProposeArgs {
    /// Stable agent identifier.
    pub agent: String,
    /// Symbolic action name the agent advertised in its capability
    /// list (`"delegate"`, `"tool_call"`, `"search"`, …). Carried as
    /// `method` on the JSON-RPC request to the agent.
    pub action: String,
    /// Action-specific JSON payload. Forwarded verbatim as the
    /// JSON-RPC `params` field.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Args for `accept` / `reject` (handlers `4`/`5`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpDecisionArgs {
    /// Stable agent identifier.
    pub agent: String,
    /// Identifier of the proposal being acknowledged. Echoed back by
    /// the agent on the original `propose` response.
    pub proposal_id: String,
    /// Optional free-form reason. Plumbed through to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ── Replies ──────────────────────────────────────────────────────────────────

/// One entry in the `list_agents` reply array.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpAgentEntry {
    /// Stable agent identifier.
    pub name: String,
    /// Executable path or name.
    pub command: String,
    /// Process arguments.
    pub args: Vec<String>,
    /// Declarative capability tags advertised by the contribution.
    pub capabilities: Vec<String>,
    /// `true` if the adapter is registered but disabled.
    pub disabled: bool,
    /// `true` while a child process for this agent is alive in the
    /// pool — the shell can render a green/grey status dot.
    pub connected: bool,
    /// Opaque shell-only metadata round-tripped from contribution
    /// time. Carries `plugin_id`, `display_name`, and any future
    /// shell-only fields.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── BL-113 Phase 4 — register_server / unregister_server ────────────────────

/// Args for `register_server` (handler `6`). Mirrors
/// [`crate::config::AcpAdapterSpec`] plus the contributing plugin's
/// reverse-DNS id. Shell-only fields ride in `metadata`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpRegisterServerArgs {
    /// Stable agent identifier ([`crate::config::AcpAdapterSpec::name`]).
    pub name: String,
    /// Executable to spawn.
    pub command: String,
    /// CLI args appended at spawn time.
    #[serde(default)]
    pub args: Vec<String>,
    /// Declarative capability tags.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// `true` to keep the entry registered but skip spawning.
    #[serde(default)]
    pub disabled: bool,
    /// Environment merged on top of the host process's environment at
    /// spawn time.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, string>"))]
    pub env: std::collections::HashMap<String, String>,
    /// Opaque shell-only metadata. Pre-packed by the bootstrap-side
    /// converter so the host can store it verbatim.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Reverse-DNS id of the contributing plugin. Authorisation key
    /// for `unregister_server`.
    pub plugin_id: String,
}

/// Reply for `register_server` (handler `6`). `status` is one of:
/// - `"ok"` — adapter registered.
/// - `"already_registered"` — another contribution owns the `name`.
/// - `"invalid_name"` / `"invalid_command"` — required field was
///   empty / whitespace-only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpRegisterServerReply {
    /// `true` iff the adapter was inserted (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"already_registered"`, `"invalid_name"`,
    /// `"invalid_command"`.
    pub status: String,
}

/// Args for `unregister_server` (handler `7`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpUnregisterServerArgs {
    /// Adapter `name` to remove.
    pub name: String,
    /// Reverse-DNS id of the plugin claiming to own the entry. Must
    /// match the plugin recorded at register time, otherwise the host
    /// refuses with `status = "not_owned_by_plugin"`.
    pub plugin_id: String,
}

/// Reply for `unregister_server` (handler `7`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AcpUnregisterServerReply {
    /// `true` iff the adapter was removed (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"not_found"`, `"not_owned_by_plugin"`.
    pub status: String,
    /// Populated when `status = "not_owned_by_plugin"` so the caller
    /// can log who actually contributed the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_owner: Option<String>,
}
