//! Wire-mirror IPC types for `com.nexus.dap`.
//!
//! Handlers in [`crate::core_plugin`] construct responses with ad-hoc
//! `serde_json::json!` macros (same shape as `nexus-lsp::ipc` and
//! `nexus-mcp::ipc`). This module exists so the schema generator + the
//! shell have something concrete to consume.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Reply entry from `list_adapters` (handler `1`).
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
pub struct DapAdapterEntry {
    /// Configured adapter name.
    pub name: String,
    /// Executable path or name.
    pub command: String,
    /// CLI args.
    pub args: Vec<String>,
    /// Optional cosmetic type hint.
    pub adapter_type: Option<String>,
    /// File extensions this adapter handles.
    pub file_types: Vec<String>,
    /// `true` if disabled in `dap.toml`.
    pub disabled: bool,
    /// `true` if currently connected via the pool.
    pub connected: bool,
    /// BL-113 — opaque shell-facing payload populated by the
    /// contribution-wiring layer when the adapter came through
    /// `[[registrations.protocol_hosts.dap]]` rather than `dap.toml`.
    /// Carries the contributing plugin's `launch_config_schema` (inline
    /// JSON Schema) and cosmetic fields (`display_name`, etc.) so the
    /// shell can render a typed launch-config form without needing a
    /// separate IPC round-trip. `null` for TOML-loaded entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    pub metadata: Option<serde_json::Value>,
}

/// Args for `launch` (handler `2`).
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
pub struct DapLaunchArgs {
    /// Adapter name from `dap.toml`.
    pub adapter: String,
    /// Path to the program / entry-point to launch.
    pub program: String,
    /// `"launch"` (default) or `"debug"` / `"run"` — adapter-specific
    /// hint passed through verbatim in the `mode` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// CLI args forwarded to the debuggee.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory for the debuggee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Extra environment for the debuggee.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, string>"))]
    pub env: std::collections::HashMap<String, String>,
    /// `true` → adapter should stop at program entry.
    #[serde(default)]
    pub stop_on_entry: bool,
    /// Adapter-specific extras (merged into the `launch` request body).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub extra: Option<serde_json::Value>,
}

/// Args for `attach` (handler `3`).
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
pub struct DapAttachArgs {
    /// Adapter name from `dap.toml`.
    pub adapter: String,
    /// Target process id (when attaching by PID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    /// Target TCP port (when attaching to a remote debug server).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i64>,
    /// Adapter-specific extras (merged into the `attach` request body).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub extra: Option<serde_json::Value>,
}

/// Args for `configuration_done` (`4`) / `disconnect` (`5`) /
/// `terminate` (`6`) / `threads` (`15`).
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
pub struct DapAdapterArgs {
    /// Adapter name from `dap.toml`.
    pub adapter: String,
    /// `disconnect`-only: ask the adapter to terminate the debuggee
    /// alongside the disconnect. Ignored for other handlers.
    #[serde(default)]
    pub terminate_debuggee: bool,
}

/// One row in [`DapSetBreakpointsArgs::breakpoints`].
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
pub struct DapSourceBreakpoint {
    /// 1-based line number.
    pub line: i64,
    /// Adapter-evaluated boolean expression; breakpoint only fires
    /// when truthy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Hit-count expression (e.g. `"> 10"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Turns the breakpoint into a logpoint (prints `log_message`
    /// instead of pausing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

/// Args for `set_breakpoints` (handler `7`).
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
pub struct DapSetBreakpointsArgs {
    /// Adapter name from `dap.toml`.
    pub adapter: String,
    /// Source path (absolute, host filesystem).
    pub source_path: String,
    /// Replacement breakpoint set for this source. An empty array
    /// clears all breakpoints in the file.
    pub breakpoints: Vec<DapSourceBreakpoint>,
}

/// One function-breakpoint row.
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
pub struct DapFunctionBreakpoint {
    /// Function name to break on.
    pub name: String,
    /// Optional condition expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Args for `set_function_breakpoints` (handler `8`).
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
pub struct DapSetFunctionBreakpointsArgs {
    /// Adapter name.
    pub adapter: String,
    /// Replacement set of function breakpoints.
    pub breakpoints: Vec<DapFunctionBreakpoint>,
}

/// Args for `set_exception_breakpoints` (handler `9`).
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
pub struct DapSetExceptionBreakpointsArgs {
    /// Adapter name.
    pub adapter: String,
    /// Filter ids from the adapter's `exceptionBreakpointFilters`
    /// capability (typically `raised` / `uncaught`).
    pub filters: Vec<String>,
}

/// Args for `continue` / `next` / `step_in` / `step_out` / `pause`
/// (handlers `10`..=`14`).
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
pub struct DapThreadArgs {
    /// Adapter name.
    pub adapter: String,
    /// DAP thread id.
    pub thread_id: i64,
}

/// Args for `stack_trace` (handler `16`).
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
pub struct DapStackTraceArgs {
    /// Adapter name.
    pub adapter: String,
    /// DAP thread id.
    pub thread_id: i64,
    /// First frame to return (0-based). Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<i64>,
    /// Max frames to return. Defaults to adapter's choice (typically
    /// unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<i64>,
}

/// Args for `scopes` (handler `17`).
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
pub struct DapScopesArgs {
    /// Adapter name.
    pub adapter: String,
    /// DAP frame id (from `stackTrace`).
    pub frame_id: i64,
}

/// Args for `variables` (handler `18`).
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
pub struct DapVariablesArgs {
    /// Adapter name.
    pub adapter: String,
    /// Reference returned by an earlier `scopes` / `variables` reply.
    pub variables_reference: i64,
    /// Optional `"named"` / `"indexed"` filter for paginated types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Start offset for paginated reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// Max children to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// Args for `evaluate` (handler `19`).
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
pub struct DapEvaluateArgs {
    /// Adapter name.
    pub adapter: String,
    /// Expression to evaluate.
    pub expression: String,
    /// Frame id to evaluate against. When absent, evaluates in
    /// global scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
    /// `"watch"` / `"repl"` / `"hover"` / `"clipboard"` — adapter
    /// uses the context to gate side effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Tiny ack reply for fire-and-forget verbs
/// (`configuration_done`, `disconnect`, `terminate`, `continue`,
/// `next`, `step_in`, `step_out`, `pause`,
/// `set_exception_breakpoints`).
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
pub struct DapOk {
    /// Always `true`.
    pub ok: bool,
}

// ── BL-113 Phase 1b — register_adapter / unregister_adapter ─────────────────

/// Args for `register_adapter` (handler `20`). Mirrors a
/// [`crate::config::DapAdapterSpec`] plus the contributing plugin's
/// reverse-DNS id. The host crate stays protocol-only per ADR 0027 —
/// shell-side cosmetic fields (`display_name`, `variable_renderers`,
/// `root_markers`) remain absent — they don't affect host behaviour.
/// `metadata` is an opaque pass-through: the host stores it verbatim
/// and round-trips it on `list_adapters` so the shell can render a
/// typed launch-config form from the contributing plugin's
/// `launch_config_schema` (packed under `metadata.launch_config_schema`
/// by `nexus-bootstrap::dap_contribution_to_spec`).
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
pub struct DapRegisterAdapterArgs {
    /// Stable adapter identifier (`DapAdapterSpec::name`).
    pub name: String,
    /// Executable to spawn.
    pub command: String,
    /// CLI args appended at spawn time.
    #[serde(default)]
    pub args: Vec<String>,
    /// Cosmetic adapter-type hint (`"lldb"`, `"node"`, `"python"`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_type: Option<String>,
    /// File extensions the adapter handles.
    #[serde(default)]
    pub file_types: Vec<String>,
    /// `true` to keep the entry registered but skip spawning.
    #[serde(default)]
    pub disabled: bool,
    /// Environment merged on top of the host process's environment
    /// at spawn time.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, string>"))]
    pub env: std::collections::HashMap<String, String>,
    /// Reverse-DNS id of the contributing plugin; used for diagnostics
    /// and as the authorisation key for `unregister_adapter`.
    pub plugin_id: String,
    /// Opaque shell-facing payload. The host never interprets it; it
    /// flows through `list_adapters` as-is. Populated by
    /// `nexus-bootstrap::dap_contribution_to_spec` with the
    /// contributing plugin's `launch_config_schema` (inline JSON
    /// Schema) + cosmetic shell-only fields. `null` is fine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    pub metadata: Option<serde_json::Value>,
}

/// Reply for `register_adapter` (handler `20`).
///
/// `status` is one of:
/// - `"ok"` — adapter registered successfully.
/// - `"toml_override"` — the `name` is already taken by a TOML-loaded
///   entry or another plugin's contribution; nothing was inserted.
/// - `"invalid_name"` / `"invalid_command"` — `name` or `command` was
///   empty / whitespace-only; nothing was inserted.
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
pub struct DapRegisterAdapterReply {
    /// `true` iff the adapter was inserted (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"toml_override"`, `"invalid_name"`,
    /// `"invalid_command"`.
    pub status: String,
}

/// Args for `unregister_adapter` (handler `21`).
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
pub struct DapUnregisterAdapterArgs {
    /// Adapter `name` to remove.
    pub name: String,
    /// Reverse-DNS id of the plugin claiming to own the entry. Must
    /// match the plugin recorded at register time, otherwise the host
    /// refuses with `status = "not_owned_by_plugin"`.
    pub plugin_id: String,
}

/// Reply for `unregister_adapter` (handler `21`).
///
/// `status` is one of:
/// - `"ok"` — adapter removed.
/// - `"not_found"` — no adapter exists under that `name`.
/// - `"toml_entry"` — the name belongs to a TOML-loaded entry; plugins
///   cannot unregister TOML-pinned adapters.
/// - `"not_owned_by_plugin"` — name exists and was plugin-contributed,
///   but by a different plugin. `actual_owner` carries the real owner's
///   reverse-DNS id for diagnostics.
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
pub struct DapUnregisterAdapterReply {
    /// `true` iff the adapter was removed (status == `"ok"`).
    pub ok: bool,
    /// One of `"ok"`, `"not_found"`, `"toml_entry"`,
    /// `"not_owned_by_plugin"`.
    pub status: String,
    /// Populated when `status = "not_owned_by_plugin"` so the caller
    /// can log who actually contributed the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_owner: Option<String>,
}
