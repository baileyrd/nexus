//! Wire-mirror IPC types for `com.nexus.editor`.
//!
//! Audit-2026-05-01 P1-3 (#113) — the handlers in
//! [`crate::core_plugin`] currently parse args directly from
//! `serde_json::Value` (via `relpath_arg(...)` and ad-hoc `args.get`
//! probes) and emit responses with `serde_json::json!`. This module
//! mirrors those wire shapes so the schema generator + the shell have
//! something concrete to consume, matching the pattern already used by
//! `nexus_git::ipc`, `nexus_lsp::ipc`, `nexus_dap::ipc`,
//! `nexus_acp::ipc`, and `nexus_mcp::ipc`.
//!
//! Reply types are intentionally minimal — the editor's structural
//! reply ([`crate::core_plugin::EditorSnapshot`]) transitively pulls in
//! the entire block-tree domain model (`Block`, `BlockTree`,
//! `BlockProperties`, `PropertyValue`, …) which uses `#[serde(flatten)]`
//! and other forward-compat shapes that don't compose with the P0-2
//! `deny_unknown_fields` gate. Same trade-off `nexus-skills` and
//! `nexus-workflow` already make: args wired in, structural returns
//! treated as opaque.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Args for the relpath-only handlers: `open`, `close`, `get_tree`,
/// `get_markdown`, `save`, `undo`, `redo`, `refresh_excerpts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorPathArgs {
    /// Forge-relative path of the session.
    pub relpath: String,
}

/// Args for `sync_content`. Re-seeds the session's content from a
/// caller-supplied string (used by the shell when the user pastes a
/// full document or after external file changes).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorSyncContentArgs {
    /// Forge-relative path of the session.
    pub relpath: String,
    /// New full document content. Reparsed end-to-end.
    pub content: String,
}

/// Args for `stamp_block`. Returns a stable `<!-- ^<uuid> -->` marker
/// for the addressed block, allocating one if absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorStampBlockArgs {
    /// Forge-relative path of the session.
    pub relpath: String,
    /// UUID of the block to stamp. Sent as a string so the wire form
    /// is JSON-friendly; the handler parses it back to a `uuid::Uuid`.
    pub block_id: String,
}

/// Args for `apply_transaction`. The `transaction` blob is the typed
/// [`crate::Transaction`] payload — kept opaque on the wire so this
/// module doesn't transitively pull in the entire transaction domain
/// model (which uses `#[serde(flatten)]` for forward-compat fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorApplyTransactionArgs {
    /// Forge-relative path of the session.
    pub relpath: String,
    /// Serialized [`crate::Transaction`]. The handler decodes via
    /// `serde_json::from_value` and rejects payloads larger than 16
    /// MiB.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub transaction: serde_json::Value,
}

/// Args for `resolve_block_link`. Note: the field is `file_relpath`,
/// not `relpath` — the handler uses this name to disambiguate from
/// the *current* session path when resolving cross-file links.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorResolveBlockLinkArgs {
    /// Forge-relative path of the target file.
    pub file_relpath: String,
    /// UUID of the block to resolve, as a string.
    pub block_id: String,
}

/// Args for `open_excerpts`. Each item is a
/// [`crate::core_plugin::ExcerptRequest`] line-range over a source
/// file; the handler merges overlapping ranges and assembles a
/// synthetic read-only session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorOpenExcerptsArgs {
    /// One or more excerpt requests. Empty arrays are rejected by
    /// the handler.
    pub items: Vec<EditorExcerptRequest>,
}

/// Wire-mirror of [`crate::core_plugin::ExcerptRequest`]. Kept here
/// so the schema generator sees a `deny_unknown_fields` form even
/// though the impl type accepts `#[serde(default)]` for `label`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorExcerptRequest {
    /// Forge-relative path of the source file to read from.
    pub relpath: String,
    /// First line to include (1-based, inclusive).
    pub line_start: u32,
    /// Last line to include (1-based, inclusive).
    pub line_end: u32,
    /// Optional caller-supplied label rendered alongside the
    /// `{relpath}#L{line_start}-L{line_end}` excerpt header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── Replies ──────────────────────────────────────────────────────────────────

/// Standard `{}` ack reply for handlers that return no payload:
/// `close`, `save`, `sync_content`, `refresh_excerpts`, `undo`/`redo`
/// in some paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorOk {}

/// Reply for `stamp_block`. The handler returns both the input and
/// post-stamp UUIDs (as JSON strings) because allocation rekeys the
/// block: callers that were tracking the input id need the new
/// `stable_id` to address the block in subsequent transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditorStampBlockReply {
    /// Echo of the caller-supplied block UUID, JSON-stringified.
    pub block_id: String,
    /// Post-stamp UUID — equal to `block_id` when the block was
    /// already stamped, freshly allocated otherwise.
    pub stable_id: String,
    /// `true` when the handler allocated a new stamp on this call;
    /// `false` if the block was already stamped.
    pub newly_stamped: bool,
}
