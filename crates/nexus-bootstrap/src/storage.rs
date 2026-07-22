//! Typed IPC-client helpers for the `com.nexus.storage` core plugin.
//!
//! CLI and TUI callers reach storage exclusively through these helpers — no
//! direct `nexus-storage` dependency needed. Each helper:
//!
//! 1. Serializes arguments to JSON.
//! 2. `await`s the async [`IpcInvoker::ipc_call`] on the provided invoker.
//! 3. Deserializes the response into a typed DTO.
//!
//! DTO field sets are intentionally minimal — only what the current callers
//! read. Extra JSON fields in the response are ignored by serde, so adding
//! fields upstream does not break callers here.
//!
//! BL-147 — helpers take `&dyn IpcInvoker` rather than `&Runtime`, so the
//! same surface works against both local and remote (`ssh://`) forges. Each
//! helper is `async`; sync callers wrap with `rt.block_on(...)`.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::invoker::IpcInvoker;

const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Default per-call timeout for the storage IPC helpers in this
/// module. Issue #83 flagged this as un-overridable; for now callers
/// that need a different timeout build their own
/// `invoker.ipc_call(...)` invocation directly. Exposed as `pub` so
/// callers can mirror the default rather than re-deriving it.
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
    /// #375 — the block's file mtime, Unix seconds.
    #[serde(default)]
    pub mtime: i64,
}

/// Sort order for [`search_with_options`]. Mirror of
/// `nexus_storage::search::SearchSort`. #375.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    /// BM25 relevance, descending score (the historical/only behavior).
    #[default]
    Relevance,
    /// Most-recently-modified block first.
    MtimeDesc,
    /// Least-recently-modified block first.
    MtimeAsc,
}

/// Optional paging/sort/date-range knobs for [`search_with_options`].
/// Mirror of `nexus_storage::search::SearchOptions`. #375.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchOptions {
    /// Skip this many ranked hits before taking the page of `limit`.
    pub offset: usize,
    /// How to order hits.
    pub sort: SearchSort,
    /// Only include blocks whose file mtime is on or after this
    /// Unix-seconds timestamp.
    pub mtime_after: Option<i64>,
    /// Only include blocks whose file mtime is on or before this
    /// Unix-seconds timestamp.
    pub mtime_before: Option<i64>,
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

async fn call<T: serde::de::DeserializeOwned>(
    invoker: &(dyn IpcInvoker + Send + Sync),
    command: &str,
    args: serde_json::Value,
) -> Result<T> {
    let value = invoker
        .ipc_call(STORAGE_PLUGIN, command, args, IPC_TIMEOUT)
        .await
        .with_context(|| format!("storage ipc call '{command}' failed"))?;
    serde_json::from_value(value)
        .with_context(|| format!("storage ipc response '{command}' decode failed"))
}

// ── Public helpers ───────────────────────────────────────────────────────────

/// List every file in the forge index.
pub async fn query_files(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<Vec<FileRecord>> {
    call(invoker, "query_files", serde_json::json!({})).await
}

/// List files whose path starts with `prefix`. Empty prefix matches all.
pub async fn query_files_with_prefix(
    invoker: &(dyn IpcInvoker + Send + Sync),
    prefix: &str,
) -> Result<Vec<FileRecord>> {
    let args = if prefix.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "prefix": prefix })
    };
    call(invoker, "query_files", args).await
}

/// Return every block belonging to the file at `path`. Empty when the path is
/// unknown to the index.
pub async fn query_blocks(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
) -> Result<Vec<BlockRecord>> {
    call(invoker, "query_blocks", serde_json::json!({ "path": path })).await
}

/// Query all occurrences of the tag named `name` across the forge.
pub async fn query_tags(
    invoker: &(dyn IpcInvoker + Send + Sync),
    name: &str,
) -> Result<Vec<TagResult>> {
    call(invoker, "query_tags", serde_json::json!({ "name": name })).await
}

/// Read a file's bytes by forge-relative path. Returns an error when the
/// file does not exist — callers that want a typed "missing" signal should
/// use [`read_file_optional`].
pub async fn read_file(invoker: &(dyn IpcInvoker + Send + Sync), path: &str) -> Result<Vec<u8>> {
    read_file_optional(invoker, path)
        .await?
        .with_context(|| format!("read_file: file not found: {path}"))
}

/// Read a file's bytes by forge-relative path. Returns `Ok(None)` when the
/// file does not exist (storage returns `{ "bytes": null }` for missing),
/// `Err` for any other failure.
pub async fn read_file_optional(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
) -> Result<Option<Vec<u8>>> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        bytes: Option<Vec<u8>>,
    }
    let resp: Resp = call(invoker, "read_file", serde_json::json!({ "path": path })).await?;
    Ok(resp.bytes)
}

/// Return every file that links TO `path`.
pub async fn backlinks(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
) -> Result<Vec<BacklinkResult>> {
    call(invoker, "backlinks", serde_json::json!({ "path": path })).await
}

/// Query tasks matching `filter`.
pub async fn query_tasks(
    invoker: &(dyn IpcInvoker + Send + Sync),
    filter: &TaskFilter,
) -> Result<Vec<TaskRecord>> {
    let args = serde_json::to_value(filter).context("serialize TaskFilter")?;
    call(invoker, "query_tasks", args).await
}

/// Return knowledge-graph summary statistics.
pub async fn graph_stats(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<GraphStats> {
    call(invoker, "graph_stats", serde_json::json!({})).await
}

/// Full-text search across block content.
pub async fn search(
    invoker: &(dyn IpcInvoker + Send + Sync),
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    search_with_options(invoker, query, limit, SearchOptions::default()).await
}

/// Search with paging/sort/date-range knobs (#375). See
/// [`SearchOptions`] for their semantics.
pub async fn search_with_options(
    invoker: &(dyn IpcInvoker + Send + Sync),
    query: &str,
    limit: usize,
    options: SearchOptions,
) -> Result<Vec<SearchResult>> {
    call(
        invoker,
        "search",
        serde_json::json!({
            "query": query,
            "limit": limit,
            "offset": options.offset,
            "sort": options.sort,
            "mtime_after": options.mtime_after,
            "mtime_before": options.mtime_before,
        }),
    )
    .await
}

/// Rebuild the forge index from files on disk.
pub async fn rebuild_index(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<RebuildStats> {
    call(invoker, "rebuild_index", serde_json::json!({})).await
}

/// Write `bytes` to `path` (forge-relative) atomically and update the index.
pub async fn write_file(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
    bytes: &[u8],
) -> Result<FileMetadata> {
    call(
        invoker,
        "write_file",
        serde_json::json!({ "path": path, "bytes": bytes }),
    )
    .await
}

/// Delete the file at `path`.
pub async fn delete_file(invoker: &(dyn IpcInvoker + Send + Sync), path: &str) -> Result<()> {
    let _: serde_json::Value =
        call(invoker, "delete_file", serde_json::json!({ "path": path })).await?;
    Ok(())
}

// ── C3 (#356) — trash verbs ──────────────────────────────────────────────────

/// One trashed entry, as returned by [`trash_list`].
#[derive(Debug, Clone, Deserialize)]
pub struct TrashRow {
    /// Bucket id — pass to [`trash_restore`].
    pub trash_id: String,
    /// Forge-relative path the entry lived at before deletion.
    pub original_path: String,
    /// Unix epoch milliseconds at deletion time.
    pub deleted_at_ms: i64,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// Recursive size of the trashed content in bytes.
    pub size_bytes: u64,
}

/// Move an entry to the forge (`"forge"`) or OS (`"system"`) trash.
/// Returns the forge-trash bucket id (`None` for the OS trash).
pub async fn trash_entry(
    invoker: &(dyn IpcInvoker + Send + Sync),
    relpath: &str,
    destination: &str,
) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Resp {
        trash_id: Option<String>,
    }
    let resp: Resp = call(
        invoker,
        "trash_entry",
        serde_json::json!({ "relpath": relpath, "destination": destination }),
    )
    .await?;
    Ok(resp.trash_id)
}

/// List trash buckets, newest first.
pub async fn trash_list(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<Vec<TrashRow>> {
    #[derive(Deserialize)]
    struct Resp {
        entries: Vec<TrashRow>,
    }
    let resp: Resp = call(invoker, "trash_list", serde_json::json!({})).await?;
    Ok(resp.entries)
}

/// Restore a trashed entry; returns the restored forge-relative path.
pub async fn trash_restore(
    invoker: &(dyn IpcInvoker + Send + Sync),
    trash_id: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct Resp {
        restored_path: String,
    }
    let resp: Resp = call(
        invoker,
        "trash_restore",
        serde_json::json!({ "trash_id": trash_id }),
    )
    .await?;
    Ok(resp.restored_path)
}

/// Permanently delete trash buckets (optionally only those older than
/// `older_than_days`). Returns the number removed.
pub async fn trash_empty(
    invoker: &(dyn IpcInvoker + Send + Sync),
    older_than_days: Option<u64>,
) -> Result<usize> {
    #[derive(Deserialize)]
    struct Resp {
        removed: usize,
    }
    let resp: Resp = call(
        invoker,
        "trash_empty",
        serde_json::json!({ "older_than_days": older_than_days }),
    )
    .await?;
    Ok(resp.removed)
}

/// Check whether a file at `path` exists in the forge.
pub async fn file_exists(invoker: &(dyn IpcInvoker + Send + Sync), path: &str) -> Result<bool> {
    #[derive(Deserialize)]
    struct Resp {
        exists: bool,
    }
    let resp: Resp = call(invoker, "file_exists", serde_json::json!({ "path": path })).await?;
    Ok(resp.exists)
}

/// Rebuild the full-text search index from the current file set.
pub async fn rebuild_search_index(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<()> {
    let _: serde_json::Value = call(invoker, "rebuild_search_index", serde_json::json!({})).await?;
    Ok(())
}

/// Toggle a task's completed state, returning the updated record.
pub async fn toggle_task(
    invoker: &(dyn IpcInvoker + Send + Sync),
    task_id: u64,
) -> Result<TaskRecord> {
    call(
        invoker,
        "toggle_task",
        serde_json::json!({ "task_id": task_id }),
    )
    .await
}

/// Return every link FROM `path` to another file.
pub async fn outgoing_links(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
) -> Result<Vec<OutgoingLink>> {
    call(
        invoker,
        "outgoing_links",
        serde_json::json!({ "path": path }),
    )
    .await
}

/// Return every link target that has no corresponding file.
pub async fn unresolved_links(
    invoker: &(dyn IpcInvoker + Send + Sync),
) -> Result<Vec<UnresolvedLink>> {
    call(invoker, "unresolved_links", serde_json::json!({})).await
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
pub async fn base_index(invoker: &(dyn IpcInvoker + Send + Sync), path: &str) -> Result<i64> {
    #[derive(Deserialize)]
    struct Resp {
        base_id: i64,
    }
    let resp: Resp = call(invoker, "base_index", serde_json::json!({ "path": path })).await?;
    Ok(resp.base_id)
}

/// List every indexed base.
pub async fn base_list(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<Vec<BaseSummary>> {
    call(invoker, "base_list", serde_json::json!({})).await
}

/// Run a structured query against the base at `path`. Filters/sorts are
/// parsed server-side using `nexus_storage::bases::query::parse_filter` /
/// `parse_sort`.
pub async fn base_query(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
    filters: &[String],
    sorts: &[String],
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<BaseQueryResult> {
    call(
        invoker,
        "base_query",
        serde_json::json!({
            "path": path,
            "filters": filters,
            "sorts": sorts,
            "limit": limit,
            "offset": offset,
        }),
    )
    .await
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
pub async fn config_read(
    invoker: &(dyn IpcInvoker + Send + Sync),
    kind: &str,
) -> Result<ConfigPayload> {
    call(invoker, "config_read", serde_json::json!({ "kind": kind })).await
}

/// Reset one of the four forge config files to its defaults.
///
/// `kind` must be `"app"`, `"workspace"`, `"mcp"`, or `"ai"`.
pub async fn config_reset(invoker: &(dyn IpcInvoker + Send + Sync), kind: &str) -> Result<()> {
    let _: serde_json::Value =
        call(invoker, "config_reset", serde_json::json!({ "kind": kind })).await?;
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
pub async fn write_default_gitignore(invoker: &(dyn IpcInvoker + Send + Sync)) -> Result<bool> {
    #[derive(Deserialize)]
    struct Resp {
        wrote: bool,
    }
    let resp: Resp = call(invoker, "write_default_gitignore", serde_json::json!({})).await?;
    Ok(resp.wrote)
}

/// Return paths of files within `depth` link hops of `path`.
pub async fn graph_neighbors(
    invoker: &(dyn IpcInvoker + Send + Sync),
    path: &str,
    depth: usize,
) -> Result<Vec<String>> {
    call(
        invoker,
        "graph_neighbors",
        serde_json::json!({ "path": path, "depth": depth }),
    )
    .await
}

// ── BL-128 entity-graph helpers ───────────────────────────────────────────────

/// One hit returned by [`entity_search`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntitySearchHit {
    /// Canonical entity id (file stem).
    pub id: String,
    /// `entity_type` from frontmatter.
    pub entity_type: String,
    /// One-line description (frontmatter or fallback body paragraph).
    pub description: String,
    /// Forge-relative path of the markdown file.
    pub relpath: String,
    /// Match score per
    /// `nexus_storage::entity_index::EntityIndex::search`'s doc-comment.
    pub score: i32,
}

/// Full entity payload returned by [`entity_get`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntityRecord {
    /// Canonical id (file stem).
    pub id: String,
    /// `entity_type` from frontmatter.
    pub entity_type: String,
    /// Aliases from frontmatter (after empty-string filtering).
    pub aliases: Vec<String>,
    /// One-line description (frontmatter or fallback body paragraph).
    pub description: String,
    /// Outgoing relations declared on this entity.
    pub relations: Vec<EntityRelation>,
    /// Forge-relative path of the markdown file.
    pub relpath: String,
}

/// One outgoing relation in [`EntityRecord::relations`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntityRelation {
    /// Target entity id (or alias).
    pub target: String,
    /// Free-form relation kind.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// One row in [`entity_relations`]'s response.
#[derive(Debug, Clone, Deserialize)]
pub struct EntityRelationEdge {
    /// Source entity id.
    pub from: String,
    /// Target entity id (alias-resolved).
    pub to: String,
    /// Free-form relation kind.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// Mirror of `nexus_storage::ipc::EntityUpsertArgs` for the IPC client.
#[derive(Debug, Clone, Serialize)]
pub struct EntityUpsert {
    /// Canonical id — becomes the markdown file stem under `entities/`.
    pub id: String,
    /// `entity_type:` frontmatter key.
    pub entity_type: String,
    /// `aliases:` frontmatter key. Omitted on disk when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `description:` frontmatter key. Omitted on disk when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `relations:` frontmatter list. Omitted on disk when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<EntityUpsertRelation>,
}

/// One relation entry inside [`EntityUpsert::relations`].
#[derive(Debug, Clone, Serialize)]
pub struct EntityUpsertRelation {
    /// Target entity id or alias.
    pub target: String,
    /// Free-form relation kind. Normalised server-side before persistence.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`. Absent ⇒ `1.0` on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Result of [`entity_upsert`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntityUpsertOutcome {
    /// Forge-relative path of the entity markdown file that was written.
    pub relpath: String,
    /// `true` when an existing file was replaced.
    pub replaced: bool,
}

/// One pair returned by [`entity_find_duplicates`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntityDuplicatePair {
    /// Lexicographically-smaller entity id.
    pub a: String,
    /// Lexicographically-greater entity id.
    pub b: String,
    /// Jaccard token similarity in `[0.0, 1.0]`.
    pub similarity: f32,
}

/// Substring-rank search over the file-backed `entities/` index.
pub async fn entity_search(
    invoker: &(dyn IpcInvoker + Send + Sync),
    query: &str,
    entity_type: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<EntitySearchHit>> {
    #[derive(Deserialize)]
    struct Resp {
        results: Vec<EntitySearchHit>,
    }
    let mut args = serde_json::json!({ "query": query });
    if let Some(t) = entity_type {
        args["entity_type"] = serde_json::Value::String(t.to_string());
    }
    if let Some(l) = limit {
        args["limit"] = serde_json::Value::from(l);
    }
    let resp: Resp = call(invoker, "entity_search", args).await?;
    Ok(resp.results)
}

/// Look up one entity by canonical id or alias.
pub async fn entity_get(
    invoker: &(dyn IpcInvoker + Send + Sync),
    id: &str,
) -> Result<Option<EntityRecord>> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        entity: Option<EntityRecord>,
    }
    let resp: Resp = call(invoker, "entity_get", serde_json::json!({ "id": id })).await?;
    Ok(resp.entity)
}

/// Outgoing / incoming / both relations for an entity. `direction`
/// is one of `"outgoing"`, `"incoming"`, `"both"`; unknown values
/// map to `"both"` server-side.
pub async fn entity_relations(
    invoker: &(dyn IpcInvoker + Send + Sync),
    id: &str,
    direction: Option<&str>,
) -> Result<Vec<EntityRelationEdge>> {
    #[derive(Deserialize)]
    struct Resp {
        relations: Vec<EntityRelationEdge>,
    }
    let mut args = serde_json::json!({ "id": id });
    if let Some(d) = direction {
        args["direction"] = serde_json::Value::String(d.to_string());
    }
    let resp: Resp = call(invoker, "entity_relations", args).await?;
    Ok(resp.relations)
}

/// File-as-truth write-through. Creates or replaces
/// `<forge>/entities/<id>.md` via the atomic temp-fsync-rename path.
pub async fn entity_upsert(
    invoker: &(dyn IpcInvoker + Send + Sync),
    payload: &EntityUpsert,
) -> Result<EntityUpsertOutcome> {
    let args = serde_json::to_value(payload).context("serialize EntityUpsert")?;
    call(invoker, "entity_upsert", args).await
}

/// Find pairs of same-type entities whose token sets overlap by at
/// least `threshold` (defaults to `0.92` server-side when `None`).
pub async fn entity_find_duplicates(
    invoker: &(dyn IpcInvoker + Send + Sync),
    threshold: Option<f32>,
) -> Result<Vec<EntityDuplicatePair>> {
    #[derive(Deserialize)]
    struct Resp {
        pairs: Vec<EntityDuplicatePair>,
    }
    let mut args = serde_json::json!({});
    if let Some(t) = threshold {
        args["threshold"] = serde_json::Value::from(t);
    }
    let resp: Resp = call(invoker, "entity_find_duplicates", args).await?;
    Ok(resp.pairs)
}

/// BL-129 — outcome of [`entity_merge`].
#[derive(Debug, Clone, Deserialize)]
pub struct EntityMergeOutcome {
    /// Canonical id of the surviving entity (echoes the `keep` arg).
    pub kept: String,
    /// Canonical id of the deleted entity (echoes the `drop` arg).
    pub dropped: String,
    /// Aliases newly added to the survivor (including `drop`'s id).
    pub aliases_added: u32,
    /// Relations newly added to the survivor.
    pub relations_added: u32,
}

/// BL-129 — merge `drop` into `keep`. Caller picks the surviving id;
/// the convention is the lexicographically-smaller of the pair.
pub async fn entity_merge(
    invoker: &(dyn IpcInvoker + Send + Sync),
    keep: &str,
    drop: &str,
) -> Result<EntityMergeOutcome> {
    call(
        invoker,
        "entity_merge",
        serde_json::json!({ "keep": keep, "drop": drop }),
    )
    .await
}

/// BL-129 — multiplicative confidence decay across every entity
/// relation. `factor` and `floor` fall back server-side to `0.95`
/// and `0.10` when `None`. When `dry_run` is true, counts are
/// computed but no file is written.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EntityDecayRelationsOutcome {
    /// Entity files parsed during the sweep.
    pub entities_scanned: u32,
    /// Entity files that had at least one relation modified.
    pub entities_updated: u32,
    /// Relations whose confidence was strictly reduced this pass.
    pub relations_decayed: u32,
    /// Relations that landed exactly on `floor` this pass. Pre-existing
    /// at-floor relations are excluded.
    pub relations_at_floor: u32,
    /// Reflects the request: when `true`, no files were written.
    pub dry_run: bool,
}

/// BL-129 follow-up — one draft relation row surfaced by the inbox.
#[derive(Debug, Clone, Deserialize)]
pub struct DraftRelation {
    /// Canonical id of the source entity that declares the relation.
    pub from: String,
    /// Target as it appears in the source file (may be an alias).
    pub target: String,
    /// Relation kind (canonical).
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Forge-relative path of the source entity's markdown file.
    pub relpath: String,
}

/// BL-129 follow-up — return of [`list_draft_relations`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DraftRelationsPage {
    /// Draft relations sorted by ascending confidence.
    pub relations: Vec<DraftRelation>,
    /// Total qualifying relations across the forge.
    pub total: u32,
    /// `true` when `relations.len() < total`.
    pub truncated: bool,
}

/// BL-129 follow-up — list every outgoing relation at-or-below the
/// confidence `threshold` (defaults server-side to `0.5`, matching
/// the value the Dream-Cycle `infer_entity_relations` handler writes
/// for new proposals). Read-only; never mutates entity files.
pub async fn list_draft_relations(
    invoker: &(dyn IpcInvoker + Send + Sync),
    threshold: Option<f32>,
    limit: Option<u32>,
) -> Result<DraftRelationsPage> {
    let mut args = serde_json::json!({});
    if let Some(t) = threshold {
        args["threshold"] = serde_json::Value::from(t);
    }
    if let Some(l) = limit {
        args["limit"] = serde_json::Value::from(l);
    }
    call(invoker, "list_draft_relations", args).await
}

/// BL-129 — multiplicative confidence decay across every entity
/// relation in the forge's `entities/` directory.
pub async fn entity_decay_relations(
    invoker: &(dyn IpcInvoker + Send + Sync),
    factor: Option<f32>,
    floor: Option<f32>,
    dry_run: bool,
) -> Result<EntityDecayRelationsOutcome> {
    let mut args = serde_json::json!({});
    if let Some(f) = factor {
        args["factor"] = serde_json::Value::from(f);
    }
    if let Some(f) = floor {
        args["floor"] = serde_json::Value::from(f);
    }
    if dry_run {
        args["dry_run"] = serde_json::Value::Bool(true);
    }
    call(invoker, "entity_decay_relations", args).await
}

// ── C23 (#376) — note duplicate detection ─────────────────────────────────────

/// One exact-duplicate group returned by [`note_find_duplicates`] — two or
/// more markdown files sharing the same `content_hash`.
#[derive(Debug, Clone, Deserialize)]
pub struct NoteExactDuplicateGroup {
    /// SHA-256 hex digest shared by every path in the group.
    pub content_hash: String,
    /// Forge-relative paths sharing that hash, ascending.
    pub paths: Vec<String>,
}

/// One near-duplicate pair returned by [`note_find_duplicates`].
#[derive(Debug, Clone, Deserialize)]
pub struct NoteNearDuplicatePair {
    /// Lexicographically-smaller forge-relative path.
    pub a: String,
    /// Lexicographically-greater forge-relative path.
    pub b: String,
    /// Cosine similarity in `[0.0, 1.0]` between the two files' mean-
    /// pooled embedding vectors.
    pub similarity: f32,
}

/// Result of [`note_find_duplicates`].
#[derive(Debug, Clone, Deserialize)]
pub struct NoteFindDuplicatesResult {
    /// Exact-duplicate groups (`content_hash` collisions).
    pub exact: Vec<NoteExactDuplicateGroup>,
    /// Near-duplicate pairs at or above the near-duplicate threshold.
    pub near: Vec<NoteNearDuplicatePair>,
}

/// Find exact and near-duplicate notes. Exact duplicates come from a
/// `content_hash` collision over indexed markdown files; near-duplicates
/// score cosine similarity over mean-pooled per-file vectors from the
/// `notes` embedding namespace at or above `near_threshold` (defaults to
/// `0.97` server-side when `None`). Read-only; never mutates files.
pub async fn note_find_duplicates(
    invoker: &(dyn IpcInvoker + Send + Sync),
    near_threshold: Option<f32>,
) -> Result<NoteFindDuplicatesResult> {
    let mut args = serde_json::json!({});
    if let Some(t) = near_threshold {
        args["near_threshold"] = serde_json::Value::from(t);
    }
    call(invoker, "note_find_duplicates", args).await
}
