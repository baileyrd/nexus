//! Typed wrappers around `ipc_call` for the storage subsystem.
//!
//! The TUI no longer depends on `nexus-storage` directly. Instead it calls
//! into the `com.nexus.storage` core plugin over the kernel IPC boundary.
//! This module holds:
//! - Local data-transfer objects mirroring the on-the-wire JSON shape of the
//!   storage plugin's responses. Named the same as the storage types so
//!   call-site code is unchanged.
//! - One helper per storage command, each doing
//!   `block_on → ipc_call → deserialize`.
//!
//! Field sets are what the TUI actually reads — not a 1:1 mirror of the
//! storage types. If the TUI starts using a new field, add it here too.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_bootstrap::Runtime;
use nexus_kernel::PluginContext;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime as TokioRuntime;

const STORAGE_PLUGIN: &str = "com.nexus.storage";
const IPC_TIMEOUT: Duration = Duration::from_secs(10);

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Mirror of `nexus_storage::FileRecord` for the fields the TUI reads.
#[derive(Debug, Clone, Deserialize)]
pub struct FileRecord {
    /// Forge-relative path of the file.
    pub path: String,
    /// File size in bytes.
    #[allow(dead_code)]
    pub size_bytes: u64,
}

/// Mirror of `nexus_storage::BacklinkResult` with only the fields the TUI
/// actually reads. Extra JSON fields are ignored by serde.
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
    #[allow(dead_code)]
    pub excerpt: String,
}

/// Mirror of `nexus_storage::GraphStats`.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphStats {
    /// Total number of directed edges.
    pub edge_count: usize,
}

/// Mirror of `nexus_storage::TaskFilter` for outgoing args.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskFilter {
    /// Only return tasks with this completion state.
    pub completed: Option<bool>,
    /// Only return tasks from the file at this path.
    pub file_path: Option<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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

/// List every file in the forge index.
pub fn query_files(runtime: &Runtime, rt: &TokioRuntime) -> Result<Vec<FileRecord>> {
    call(runtime, rt, "query_files", serde_json::json!({}))
}

/// Read a file's bytes by forge-relative path.
pub fn read_file(runtime: &Runtime, rt: &TokioRuntime, path: &str) -> Result<Vec<u8>> {
    #[derive(Deserialize)]
    struct ReadFileResp {
        bytes: Vec<u8>,
    }
    let resp: ReadFileResp = call(runtime, rt, "read_file", serde_json::json!({ "path": path }))?;
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
