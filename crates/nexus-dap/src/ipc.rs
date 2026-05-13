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
