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
