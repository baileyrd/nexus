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
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Mirror of `nexus_storage::FileRecord` with the fields CLI/TUI read.
#[derive(Debug, Clone, Deserialize)]
pub struct FileRecord {
    /// Forge-relative path of the file.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
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

/// Read a file's bytes by forge-relative path.
pub fn read_file(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<u8>> {
    #[derive(Deserialize)]
    struct Resp {
        bytes: Vec<u8>,
    }
    let resp: Resp = call(runtime, rt, "read_file", serde_json::json!({ "path": path }))?;
    Ok(resp.bytes)
}

/// Return every file that links TO `path`.
pub fn backlinks(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<BacklinkResult>> {
    call(runtime, rt, "backlinks", serde_json::json!({ "path": path }))
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
