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
}

/// Mirror of `nexus_storage::GraphStats`.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphStats {
    /// Total number of directed edges.
    pub edge_count: usize,
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
