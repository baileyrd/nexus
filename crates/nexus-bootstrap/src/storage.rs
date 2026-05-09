//! Typed IPC-client helpers for the `com.nexus.storage` core plugin.
//!
//! CLI and TUI callers reach storage exclusively through these helpers — no
//! direct `nexus-storage` dependency needed. Each helper:
//!
//! 1. Serializes arguments to JSON.
//! 2. `block_on`s the async `ipc_call` on the provided Tokio runtime.
//! 3. Deserializes the response into a typed DTO.
//!
//! DTO field sets are intentionally minimal — only what the current callers
//! read. Extra JSON fields in the response are ignored by serde, so adding
//! fields upstream does not break callers here.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime as TokioRuntime;

use crate::Runtime;

const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Default per-call timeout for the synchronous storage IPC helpers
/// in this module. Issue #83 flagged this as un-overridable; for
/// now callers that need a different timeout build their own
/// `runtime.context.ipc_call(...)` invocation directly. Exposed as
/// `pub` so callers can mirror the default rather than re-deriving
/// it. Adding a `_with_timeout` overload to every helper is
/// tracked under #83.
pub const IPC_TIMEOUT: Duration = Duration::from_secs(30);

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Mirror of `nexus_storage::FileRecord` with the fields callers read.
/// Extra JSON fields in the response are ignored by serde.
#[derive(Debug, Clone, Deserialize)]
pub struct FileRecord {
    /// Forge-relative path of the file.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of last modification.
    #[serde(default)]
    pub modified_at: i64,
}

/// Mirror of `nexus_storage::BlockRecord` with the fields callers read.
/// Extra JSON fields in the response are ignored by serde.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockRecord {
    /// Primary key in the `blocks` table.
    pub id: u64,
    /// Kind of block: `"heading"`, `"paragraph"`, etc.
    pub block_type: String,
    /// Plain-text content of the block.
    pub content: String,
    /// Heading level 1-6; `None` for non-headings.
    #[serde(default)]
    pub level: Option<i32>,
}

/// Mirror of `nexus_storage::TagResult`.
#[derive(Debug, Clone, Deserialize)]
pub struct TagResult {
    /// Tag name (without the `#` prefix).
    pub name: String,
    /// Forge-relative path of the file containing the tag.
    pub file_path: String,
    /// Where the tag came from: `"frontmatter"` or `"inline"`.
    pub source: String,
}

/// Mirror of `nexus_storage::BacklinkResult`.
#[derive(Debug, Clone, Deserialize)]
pub struct BacklinkResult {
    /// Path of the file containing the link.
    pub source_path: String,
    /// Display text of the link.
    pub link_text: String,
}

/// Mirror of `nexus_storage::TaskRecord`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskRecord {
    /// Primary key in the tasks table.
    pub id: u64,
    /// Forge-relative path of the file containing the task.
    pub file_path: String,
    /// Task text without the checkbox prefix.
    pub content: String,
    /// Whether the task is completed.
    pub completed: bool,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
}

/// Mirror of `nexus_storage::SearchResult`.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    /// Path to the file containing the matching block.
    pub file_path: String,
    /// Excerpt of the matching content.
    pub excerpt: String,
    /// Kind of the matching block (`"paragraph"`, `"heading"`, …).
    pub block_type: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// Mirror of `nexus_storage::FileMetadata`.
#[derive(Debug, Clone, Deserialize)]
pub struct FileMetadata {
    /// Forge-relative path of the file.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// SHA-256 hex digest of the file content.
    pub content_hash: String,
}

/// Mirror of `nexus_storage::OutgoingLink`.
#[derive(Debug, Clone, Deserialize)]
pub struct OutgoingLink {
    /// Path of the link target.
    pub target_path: String,
    /// Display text of the link.
    pub link_text: String,
    /// Kind of link.
    pub link_type: String,
    /// Whether the target file exists in the forge.
    pub is_resolved: bool,
    /// Fragment identifier, if any.
    pub fragment: Option<String>,
}

/// Mirror of `nexus_storage::GraphStats`.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphStats {
    /// Total number of nodes (files + phantoms).
    pub node_count: usize,
    /// Total number of directed edges.
    pub edge_count: usize,
    /// Number of phantom (unresolved) nodes.
    pub unresolved_count: usize,
}

/// Mirror of `nexus_storage::UnresolvedLink`.
#[derive(Debug, Clone, Deserialize)]
pub struct UnresolvedLink {
    /// The missing target path.
    pub target_path: String,
    /// Paths of files that reference this target.
    pub referenced_by: Vec<String>,
}

/// Mirror of `nexus_storage::RebuildStats`.
#[derive(Debug, Clone, Deserialize)]
pub struct RebuildStats {
    /// Number of files processed.
    pub files_processed: usize,
    /// Total blocks indexed.
    pub blocks_indexed: usize,
    /// Total links found.
    pub links_found: usize,
    /// Total tags found.
    pub tags_found: usize,
    /// Wall-clock time in milliseconds.
    pub duration_ms: u64,
}

/// Outgoing `TaskFilter`. Defaults are `None` for both fields.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskFilter {
    /// Only return tasks with this completion state.
    pub completed: Option<bool>,
    /// Only return tasks from the file at this path.
    pub file_path: Option<String>,
}

// ── Internal helper ──────────────────────────────────────────────────────────

fn call<T: serde::de::DeserializeOwned>(
    runtime: &Runtime,
    rt: &TokioRuntime,
    command: &str,
    args: serde_json::Value,
) -> Result<T> {
    let value = rt
        .block_on(
            runtime
                .context
                .ipc_call(STORAGE_PLUGIN, command, args, IPC_TIMEOUT),
        )
        .with_context(|| format!("storage ipc call '{command}' failed"))?;
    serde_json::from_value(value)
        .with_context(|| format!("storage ipc response '{command}' decode failed"))
}

// ── Public helpers ───────────────────────────────────────────────────────────

/// List every file in the forge index.
pub fn query_files(runtime: &Runtime, rt: &TokioRuntime) -> Result<Vec<FileRecord>> {
    call(runtime, rt, "query_files", serde_json::json!({}))
}

/// List files whose path starts with `prefix`. Empty prefix matches all.
pub fn query_files_with_prefix(
    runtime: &Runtime,
    rt: &TokioRuntime,
    prefix: &str,
) -> Result<Vec<FileRecord>> {
    let args = if prefix.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "prefix": prefix })
    };
    call(runtime, rt, "query_files", args)
}

/// Return every block belonging to the file at `path`. Empty when the path is
/// unknown to the index.
pub fn query_blocks(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<BlockRecord>> {
    call(
        runtime,
        rt,
        "query_blocks",
        serde_json::json!({ "path": path }),
    )
}

/// Query all occurrences of the tag named `name` across the forge.
pub fn query_tags(runtime: &Runtime, rt: &TokioRuntime, name: &str) -> Result<Vec<TagResult>> {
    call(
        runtime,
        rt,
        "query_tags",
        serde_json::json!({ "name": name }),
    )
}

/// Read a file's bytes by forge-relative path. Returns an error when the
/// file does not exist — callers that want a typed "missing" signal should
/// use [`read_file_optional`].
pub fn read_file(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<u8>> {
    read_file_optional(runtime, rt, path)?
        .with_context(|| format!("read_file: file not found: {path}"))
}

/// Read a file's bytes by forge-relative path. Returns `Ok(None)` when the
/// file does not exist (storage returns `{ "bytes": null }` for missing),
/// `Err` for any other failure.
pub fn read_file_optional(
    runtime: &Runtime,
    rt: &TokioRuntime,
    path: &str,
) -> Result<Option<Vec<u8>>> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        bytes: Option<Vec<u8>>,
    }
    let resp: Resp = call(
        runtime,
        rt,
        "read_file",
        serde_json::json!({ "path": path }),
    )?;
    Ok(resp.bytes)
}

/// Return every file that links TO `path`.
pub fn backlinks(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<BacklinkResult>> {
    call(
        runtime,
        rt,
        "backlinks",
        serde_json::json!({ "path": path }),
    )
}

/// Query tasks matching `filter`.
pub fn query_tasks(
    runtime: &Runtime,
    rt: &TokioRuntime,
    filter: &TaskFilter,
) -> Result<Vec<TaskRecord>> {
    let args = serde_json::to_value(filter).context("serialize TaskFilter")?;
    call(runtime, rt, "query_tasks", args)
}

/// Return knowledge-graph summary statistics.
pub fn graph_stats(runtime: &Runtime, rt: &TokioRuntime) -> Result<GraphStats> {
    call(runtime, rt, "graph_stats", serde_json::json!({}))
}

/// Full-text search across block content.
pub fn search(
    runtime: &Runtime,
    rt: &TokioRuntime,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    call(
        runtime,
        rt,
        "search",
        serde_json::json!({ "query": query, "limit": limit }),
    )
}

/// Rebuild the forge index from files on disk.
pub fn rebuild_index(runtime: &Runtime, rt: &TokioRuntime) -> Result<RebuildStats> {
    call(runtime, rt, "rebuild_index", serde_json::json!({}))
}

/// Write `bytes` to `path` (forge-relative) atomically and update the index.
pub fn write_file(
    runtime: &Runtime,
    rt: &TokioRuntime,
    path: &str,
    bytes: &[u8],
) -> Result<FileMetadata> {
    call(
        runtime,
        rt,
        "write_file",
        serde_json::json!({ "path": path, "bytes": bytes }),
    )
}

/// Delete the file at `path`.
pub fn delete_file(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<()> {
    let _: serde_json::Value = call(
        runtime,
        rt,
        "delete_file",
        serde_json::json!({ "path": path }),
    )?;
    Ok(())
}

/// Check whether a file at `path` exists in the forge.
pub fn file_exists(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<bool> {
    #[derive(Deserialize)]
    struct Resp {
        exists: bool,
    }
    let resp: Resp = call(
        runtime,
        rt,
        "file_exists",
        serde_json::json!({ "path": path }),
    )?;
    Ok(resp.exists)
}

/// Rebuild the full-text search index from the current file set.
pub fn rebuild_search_index(runtime: &Runtime, rt: &TokioRuntime) -> Result<()> {
    let _: serde_json::Value = call(runtime, rt, "rebuild_search_index", serde_json::json!({}))?;
    Ok(())
}

/// Toggle a task's completed state, returning the updated record.
pub fn toggle_task(runtime: &Runtime, rt: &TokioRuntime, task_id: u64) -> Result<TaskRecord> {
    call(
        runtime,
        rt,
        "toggle_task",
        serde_json::json!({ "task_id": task_id }),
    )
}

/// Return every link FROM `path` to another file.
pub fn outgoing_links(
    runtime: &Runtime,
    rt: &TokioRuntime,
    path: &str,
) -> Result<Vec<OutgoingLink>> {
    call(
        runtime,
        rt,
        "outgoing_links",
        serde_json::json!({ "path": path }),
    )
}

/// Return every link target that has no corresponding file.
pub fn unresolved_links(runtime: &Runtime, rt: &TokioRuntime) -> Result<Vec<UnresolvedLink>> {
    call(runtime, rt, "unresolved_links", serde_json::json!({}))
}

/// Re-export of [`nexus_types::bases::BaseSummary`] so CLI/TUI callers don't
/// need a direct `nexus-types` dep just to consume `base_list` results.
///
/// **API stability** (issue #83). The shape and field names are
/// pinned to whatever `nexus_types::bases::BaseSummary` defines;
/// any rename or field change in `nexus-types` is a breaking
/// change for downstream callers reaching this re-export. The
/// long-term fix is a bootstrap-owned DTO that mirrors the fields
/// CLI/TUI actually consume — that's tracked under #83. Until
/// then, audit any `nexus-types::bases::BaseSummary` change for
/// downstream impact.
pub use nexus_types::bases::BaseSummary;

/// Re-export of [`nexus_storage::bases::query::QueryResult`] for
/// `base_query` callers — the SQL query engine lives in the storage
/// crate (the sole rusqlite owner), so CLI/TUI consumers can reach it
/// here without depending on `nexus-storage` directly.
///
/// **API stability** (issue #83). Same caveat as
/// [`BaseSummary`] — pinned to the storage crate's shape; a future
/// bootstrap-owned DTO would isolate the surface from internal
/// rename / field churn.
pub use nexus_storage::bases::query::QueryResult as BaseQueryResult;

/// Reindex a `.bases` directory from disk into the `SQLite` index.
///
/// `path` is forge-relative.
pub fn base_index(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<i64> {
    #[derive(Deserialize)]
    struct Resp {
        base_id: i64,
    }
    let resp: Resp = call(
        runtime,
        rt,
        "base_index",
        serde_json::json!({ "path": path }),
    )?;
    Ok(resp.base_id)
}

/// List every indexed base.
pub fn base_list(runtime: &Runtime, rt: &TokioRuntime) -> Result<Vec<BaseSummary>> {
    call(runtime, rt, "base_list", serde_json::json!({}))
}

/// Run a structured query against the base at `path`. Filters/sorts are
/// parsed server-side using `nexus_storage::bases::query::parse_filter` /
/// `parse_sort`.
pub fn base_query(
    runtime: &Runtime,
    rt: &TokioRuntime,
    path: &str,
    filters: &[String],
    sorts: &[String],
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<BaseQueryResult> {
    call(
        runtime,
        rt,
        "base_query",
        serde_json::json!({
            "path": path,
            "filters": filters,
            "sorts": sorts,
            "limit": limit,
            "offset": offset,
        }),
    )
}

/// Serialized `.forge/<file>` config payload returned by [`config_read`].
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigPayload {
    /// Encoding of [`content`](Self::content): `"toml"` or `"json"`.
    pub format: String,
    /// Pretty-printed serialized config text.
    pub content: String,
}

/// Read one of the four forge config files as pretty-printed text.
///
/// `kind` must be `"app"`, `"workspace"`, `"mcp"`, or `"ai"`.
pub fn config_read(runtime: &Runtime, rt: &TokioRuntime, kind: &str) -> Result<ConfigPayload> {
    call(
        runtime,
        rt,
        "config_read",
        serde_json::json!({ "kind": kind }),
    )
}

/// Reset one of the four forge config files to its defaults.
///
/// `kind` must be `"app"`, `"workspace"`, `"mcp"`, or `"ai"`.
pub fn config_reset(runtime: &Runtime, rt: &TokioRuntime, kind: &str) -> Result<()> {
    let _: serde_json::Value = call(
        runtime,
        rt,
        "config_reset",
        serde_json::json!({ "kind": kind }),
    )?;
    Ok(())
}

/// BL-007 — write `.forge/.gitignore` with the default exclusion list
/// if the file does not already exist. Returns `true` when a fresh
/// file was written, `false` when the file was already there
/// (idempotent re-run).
///
/// `nexus crdt enable-transport` calls this so forges created before
/// BL-007 shipped get the gitignore policy that lets the CRDT state
/// files at `.forge/.editor/crdt/*.json` ride through to peers via
/// git while rebuildable / per-machine state stays excluded.
pub fn write_default_gitignore(runtime: &Runtime, rt: &TokioRuntime) -> Result<bool> {
    #[derive(Deserialize)]
    struct Resp {
        wrote: bool,
    }
    let resp: Resp = call(
        runtime,
        rt,
        "write_default_gitignore",
        serde_json::json!({}),
    )?;
    Ok(resp.wrote)
}

/// Return paths of files within `depth` link hops of `path`.
pub fn graph_neighbors(
    runtime: &Runtime,
    rt: &TokioRuntime,
    path: &str,
    depth: usize,
) -> Result<Vec<String>> {
    call(
        runtime,
        rt,
        "graph_neighbors",
        serde_json::json!({ "path": path, "depth": depth }),
    )
}
