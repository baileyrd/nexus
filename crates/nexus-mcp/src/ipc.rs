//! Wire-mirror IPC types for `com.nexus.mcp.host`.
//!
//! Audit-2026-05-01 P1-3 (#113). The handlers in
//! [`crate::core_plugin`] construct responses with ad-hoc
//! `serde_json::json!` macros — there are no named arg/reply types
//! to gate. Same pattern as `nexus-storage::ipc` and
//! `nexus-git::ipc`.

use serde::{Deserialize, Serialize};

use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Args for `com.nexus.mcp.host::connect` / `disconnect` / `list_tools` /
/// `list_resources` / `list_prompts` (handler ids 6, 7, 2, 4, 5).
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
pub struct McpServerArgs {
    /// Configured server name (e.g. `linear`, `everything`). The
    /// engine looks this up in `<forge>/.forge/config.toml`'s
    /// `[mcp.servers.<name>]` table.
    pub server: String,
}

/// Args for `com.nexus.mcp.host::call_tool` (handler id `3`).
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
pub struct McpCallToolArgs {
    /// Configured server name.
    pub server: String,
    /// Tool name advertised by the server's `tools/list`.
    pub tool: String,
    /// Tool-call arguments object. Forwarded verbatim to the server.
    /// Schema is per-tool and not validated at the IPC boundary;
    /// validation lives on the MCP server side.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub arguments: serde_json::Map<String, serde_json::Value>,
}

// ── Replies ──────────────────────────────────────────────────────────────────

/// One entry in the `list_servers` response array. Mirrors
/// [`crate::config::ServerSpec`] for the wire.
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
pub struct McpServerEntry {
    /// Configured server name (the `[mcp.servers.<name>]` key).
    pub name: String,
    /// Command path or transport URL.
    pub command: String,
    /// Process arguments (for stdio servers).
    pub args: Vec<String>,
    /// `true` if the server is disabled in the manifest.
    pub disabled: bool,
}

/// One entry in the `list_tools` response array.
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
pub struct McpToolEntry {
    /// Tool name as advertised by the server.
    pub name: String,
    /// Optional human description.
    pub description: Option<String>,
    /// JSON Schema for the tool's `arguments`, as advertised by the
    /// MCP server. Forwarded verbatim from `rmcp::model::Tool::input_schema`.
    /// `None` only when the upstream tool didn't supply one (rmcp
    /// makes this required, so in practice every entry carries it).
    /// Required by AI tool-bridge consumers (G5b) that surface MCP
    /// tools to the model — without the schema the model can't
    /// reliably produce arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, unknown> | null"))]
    pub input_schema: Option<serde_json::Value>,
}

/// One entry in the `list_resources` response array.
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
pub struct McpResourceEntry {
    /// Resource URI.
    pub uri: String,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Optional description.
    pub description: Option<String>,
    /// MIME type, when known.
    pub mime_type: Option<String>,
}

/// One entry in the `list_prompts` response array.
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
pub struct McpPromptEntry {
    /// Prompt name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
}

/// Return type for `com.nexus.mcp.host::connect` (handler id `6`)
/// and the success branch of `disconnect` (handler id `7`).
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
pub struct McpConnectReply {
    /// Always `true` on success.
    pub ok: bool,
    /// Echoed server name.
    pub server: String,
}

/// Return type for `com.nexus.mcp.host::disconnect` when the named
/// server wasn't connected. The wire shape distinguishes "no-op
/// success" from a hard error.
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
pub struct McpDisconnectMissReply {
    /// Always `false` for this variant.
    pub ok: bool,
    /// Echoed server name.
    pub server: String,
    /// Why the disconnect was a no-op (`"not connected"`).
    pub reason: String,
}

/// Return type for `com.nexus.mcp.host::call_tool` (handler id `3`).
/// `content` carries the per-MCP-content-item array verbatim;
/// `truncated` flags when the engine clipped a response that exceeded
/// the per-call size cap (issue #85).
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
pub struct McpCallToolReply {
    /// MCP content items. Each is the raw rmcp `Content` JSON;
    /// schema is per-server.
    #[cfg_attr(feature = "ts-export", ts(type = "Array<unknown>"))]
    pub content: Vec<serde_json::Value>,
    /// `true` when the tool itself reported failure.
    pub is_error: bool,
    /// `true` when the engine clipped the content array to fit the
    /// response cap.
    pub truncated: bool,
}

// ── BL-113 Phase 3b — register_server / unregister_server ───────────────────

/// Args for `register_server` (handler `11`). Mirrors an
/// [`crate::config::McpServerSpec`] plus a name + the contributing
/// plugin's reverse-DNS id. The host crate stays protocol-only per
/// ADR 0027; shell-side fields are intentionally absent.
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
pub struct McpRegisterServerArgs {
    /// Stable server identifier (the BTreeMap key used by the host).
    pub name: String,
    /// Wire-level transport — one of `"stdio"`, `"http"`, `"websocket"`.
    /// Unknown values fall back to stdio at conversion time.
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Executable to spawn — required for stdio.
    #[serde(default)]
    pub command: String,
    /// CLI args appended at spawn time.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment merged on top of the host process's environment
    /// at spawn time (stdio only).
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, string>"))]
    pub env: std::collections::BTreeMap<String, String>,
    /// Endpoint URL — required for remote transports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `true` to keep the entry registered but skip spawning.
    #[serde(default)]
    pub disabled: bool,
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
}

fn default_transport() -> String {
    "stdio".to_string()
}

/// Reply for `register_server` (handler `11`).
///
/// `status` is one of `"ok"`, `"toml_override"`, `"invalid_name"`,
/// `"invalid"` (with a `reason` field).
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
pub struct McpRegisterServerReply {
    /// `true` iff the server was inserted (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"toml_override"`, `"invalid_name"`, `"invalid"`.
    pub status: String,
    /// Populated when `status = "invalid"` with the host's validator
    /// message (e.g. `"server 'fs' has empty command"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Args for `unregister_server` (handler `12`).
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
pub struct McpUnregisterServerArgs {
    /// Server `name` to remove.
    pub name: String,
    /// Reverse-DNS id of the plugin claiming to own the entry.
    pub plugin_id: String,
}

/// Reply for `unregister_server` (handler `12`).
///
/// `status` is one of `"ok"`, `"not_found"`, `"toml_entry"`,
/// `"not_owned_by_plugin"`.
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
pub struct McpUnregisterServerReply {
    /// `true` iff the server was removed (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"not_found"`, `"toml_entry"`,
    /// `"not_owned_by_plugin"`.
    pub status: String,
    /// Populated when `status = "not_owned_by_plugin"` so the caller
    /// can log who actually contributed the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_owner: Option<String>,
}

/// Args for `unregister_tool` (DG-39). #190 — typed counterpart of
/// the prior `str_arg(args, "name")` lookup.
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
pub struct McpUnregisterToolArgs {
    /// Tool name to unregister from the dynamic-tool registry.
    pub name: String,
}

/// Return type for `register_tool` (DG-39). #190 — typed counterpart
/// of the prior `json!({"ok": true})` reply.
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
pub struct McpRegisterToolReply {
    /// Always `true` when the tool was registered.
    pub ok: bool,
}

/// Return type for `unregister_tool` (DG-39). #190 — typed counterpart
/// of the prior `json!({"removed": bool, "name": ...})` reply.
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
pub struct McpUnregisterToolReply {
    /// `true` iff a registration was removed (i.e. the tool was
    /// present). `false` is the no-op success case.
    pub removed: bool,
    /// Echoed tool name.
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G5a: HANDLER_LIST_TOOLS must forward the input_schema verbatim
    /// so AI tool-bridge consumers can drive function-calling. The
    /// struct also has to round-trip an entry that omits the field
    /// (for backwards compat with any external producer that hasn't
    /// upgraded yet).
    #[test]
    fn mcp_tool_entry_round_trips_input_schema() {
        let wire = serde_json::json!({
            "name": "fetch",
            "description": "fetch a URL",
            "input_schema": {
                "type": "object",
                "properties": { "url": { "type": "string" } },
                "required": ["url"]
            }
        });
        let entry: McpToolEntry =
            serde_json::from_value(wire.clone()).expect("decode populated entry");
        assert_eq!(entry.name, "fetch");
        let schema = entry.input_schema.as_ref().expect("input_schema present");
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"][0], "url");
        // Re-serialising drops nothing.
        let back = serde_json::to_value(&entry).unwrap();
        assert_eq!(back, wire);
    }

    #[test]
    fn mcp_tool_entry_decodes_when_input_schema_absent() {
        let wire = serde_json::json!({ "name": "x", "description": null });
        let entry: McpToolEntry = serde_json::from_value(wire).expect("decode bare entry");
        assert_eq!(entry.name, "x");
        assert!(entry.input_schema.is_none());
        // skip_serializing_if keeps the wire shape minimal for absent schemas.
        let back = serde_json::to_value(&entry).unwrap();
        assert!(back.get("input_schema").is_none());
    }
}
