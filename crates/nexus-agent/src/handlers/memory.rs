//! DG-33 memory handlers — record / query / prune / export.

use std::sync::Arc;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::shared::{exec_err, now_unix_ms, parse_args, parse_memory_lines};

/// Args for `memory_record` — `{ agent_id, entry }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecordArgs {
    /// Reverse-DNS or short id naming the agent that owns the memory.
    pub agent_id: String,
    /// Entry to append.
    pub entry: crate::memory::MemoryEntry,
}

/// Args for `memory_query` — `{ agent_id, pattern?, limit? }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryArgs {
    /// Agent id to query.
    pub agent_id: String,
    /// Substring filter; empty / absent returns the most recent entries.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Max entries to return (default 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Args for `memory_prune` — `{ agent_id, retention_days }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryPruneArgs {
    /// Agent id whose memory should be pruned.
    pub agent_id: String,
    /// Drop entries older than this many days.
    pub retention_days: u32,
}

/// Args for `memory_export` — `{ agent_id }`.
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportArgs {
    /// Agent id whose memory should be exported as markdown.
    pub agent_id: String,
}

const MEMORY_DEFAULT_QUERY_LIMIT: u32 = 50;

pub(crate) async fn handle_memory_record(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryRecordArgs = parse_args(args, "memory_record")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_record: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let mut bytes = serde_json::to_vec(&a.entry)
        .map_err(|e| exec_err(format!("memory_record: serialize entry: {e}")))?;
    bytes.push(b'\n');

    let existing = ctx.read_file(&path).await.unwrap_or_default();
    let mut combined = existing;
    combined.extend_from_slice(&bytes);
    ctx.write_file(&path, &combined)
        .await
        .map_err(|e| exec_err(format!("memory_record: write {}: {e}", path.display())))?;
    Ok(serde_json::json!({ "recorded": true }))
}

pub(crate) async fn handle_memory_query(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryQueryArgs = parse_args(args, "memory_query")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_query: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    let entries = parse_memory_lines(&bytes);
    let pattern = a.pattern.unwrap_or_default();
    let limit =
        usize::try_from(a.limit.unwrap_or(MEMORY_DEFAULT_QUERY_LIMIT)).unwrap_or(usize::MAX);
    let hits = crate::memory::query_entries(&entries, &pattern, limit);
    serde_json::to_value(hits).map_err(|e| exec_err(format!("memory_query: serialize: {e}")))
}

pub(crate) async fn handle_memory_prune(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryPruneArgs = parse_args(args, "memory_prune")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_prune: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    if bytes.is_empty() {
        return Ok(serde_json::json!({ "pruned": 0, "kept": 0 }));
    }
    let entries = parse_memory_lines(&bytes);
    let retention_ms = u64::from(a.retention_days).saturating_mul(86_400_000);
    let (kept, pruned) = crate::memory::prune_entries(entries, now_unix_ms(), retention_ms);
    let mut out = Vec::with_capacity(bytes.len());
    for entry in &kept {
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| exec_err(format!("memory_prune: serialize: {e}")))?;
        line.push(b'\n');
        out.extend_from_slice(&line);
    }
    ctx.write_file(&path, &out)
        .await
        .map_err(|e| exec_err(format!("memory_prune: write {}: {e}", path.display())))?;
    Ok(serde_json::json!({
        "pruned": pruned,
        "kept": kept.len(),
    }))
}

pub(crate) async fn handle_memory_export(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: MemoryExportArgs = parse_args(args, "memory_export")?;
    crate::memory::normalize_agent_id(&a.agent_id)
        .map_err(|e| exec_err(format!("memory_export: {e}")))?;
    let path = crate::memory::history_path(&a.agent_id);
    let bytes = ctx.read_file(&path).await.unwrap_or_default();
    let entries = parse_memory_lines(&bytes);
    let markdown = crate::memory::export_markdown(&a.agent_id, &entries);
    Ok(serde_json::json!({ "markdown": markdown }))
}
