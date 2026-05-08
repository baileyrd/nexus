//! Wire-mirror IPC types for `com.nexus.lsp`.
//!
//! Audit-2026-05-01 P1-3 (#113). The handlers in
//! [`crate::core_plugin`] construct responses with ad-hoc
//! `serde_json::json!` macros — same shape as `nexus-storage::ipc` and
//! `nexus-mcp::ipc`. This module gives the schema generator + the
//! shell something concrete to consume.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Args for `open_file` (handler `2`). The `language_id` defaults to
/// the inferred id from the path extension if absent; `version`
/// defaults to `1`.
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
pub struct LspOpenFileArgs {
    /// Filesystem path of the file being opened.
    pub path: String,
    /// File contents at version 1 (full text — LSP requires the
    /// initial sync to ship the document body).
    pub content: String,
    /// LSP `languageId` (`"rust"`, `"typescript"`, …). When `None`,
    /// the handler infers it from the extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_id: Option<String>,
    /// Optional starting version. Servers expect a monotonic counter
    /// across `didChange` notifications; default `1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

/// Args for `close_file` (handler `3`).
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
pub struct LspPathArgs {
    /// Filesystem path of the affected document.
    pub path: String,
}

/// Args for `change_file` (handler `4`).
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
pub struct LspChangeFileArgs {
    /// Filesystem path of the document being updated.
    pub path: String,
    /// New full file contents (full-sync mode).
    pub content: String,
    /// Monotonic version counter — must strictly increase per file.
    pub version: i64,
}

/// Args for `completions` / `hover` / `definition` (handlers `5`/`6`/`7`).
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
pub struct LspPositionArgs {
    /// Filesystem path of the open document.
    pub path: String,
    /// Zero-indexed line number.
    pub line: i64,
    /// Zero-indexed character offset within the line (UTF-16 code
    /// units per the LSP spec).
    pub character: i64,
}

/// Args for `references` (handler `8`).
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
pub struct LspReferencesArgs {
    /// Filesystem path of the open document.
    pub path: String,
    /// Zero-indexed line.
    pub line: i64,
    /// Zero-indexed character.
    pub character: i64,
    /// Include the symbol's own declaration in the result list.
    /// Defaults to `true`.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub include_declaration: bool,
}

fn default_true() -> bool {
    true
}
// `serde(skip_serializing_if = "…")` requires `&T → bool`, so the
// reference is load-bearing — not a clippy bug, but we suppress the
// pedantic warning rather than restructure.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(b: &bool) -> bool {
    *b
}

/// Args for `rename` (handler `9`).
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
pub struct LspRenameArgs {
    /// Filesystem path of the open document.
    pub path: String,
    /// Zero-indexed line of the symbol being renamed.
    pub line: i64,
    /// Zero-indexed character of the symbol being renamed.
    pub character: i64,
    /// Replacement identifier.
    pub new_name: String,
}

/// Args for `code_actions` (handler `10`).
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
pub struct LspCodeActionsArgs {
    /// Filesystem path of the open document.
    pub path: String,
    /// LSP `Range` (`{ start: {line, character}, end: {line, character} }`).
    /// Forwarded verbatim; schema is intentionally untyped here so we
    /// don't have to mirror every LSP nested struct.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub range: serde_json::Value,
}

// ── Replies ──────────────────────────────────────────────────────────────────

/// One entry in the `list_servers` response array.
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
pub struct LspServerEntry {
    /// Configured server name.
    pub name: String,
    /// Executable path or name.
    pub command: String,
    /// Process arguments.
    pub args: Vec<String>,
    /// File extensions this server handles.
    pub file_types: Vec<String>,
    /// `true` if disabled in `lsp.toml`.
    pub disabled: bool,
}

/// Reply from `open_file` when a server is routed (the call returns
/// JSON `null` for paths that don't match any server).
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
pub struct LspOpenFileReply {
    /// Resolved `file://` URI passed to the server.
    pub uri: String,
    /// Server name that owns this file.
    pub server: String,
}

/// Tiny ack used by `close_file` / `change_file` (`{ "ok": true }`).
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
pub struct LspOk {
    /// Always `true`.
    pub ok: bool,
}
