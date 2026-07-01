//! Search-domain handlers: `search`, `hybrid_search`, `query_symbol`,
//! `query_tags`, `find_in_files`, `replace_in_files`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{StorageHybridSearchArgs, StorageQueryTagsArgs, StorageSearchArgs};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

/// Default match count when the caller omits `limit`. Kept identical
/// to the pre-#190 hand-rolled default for wire-compat.
const DEFAULT_SEARCH_LIMIT: usize = 50;

/// Default fused-match count for `hybrid_search` when the caller omits
/// `limit`.
const DEFAULT_HYBRID_LIMIT: usize = 10;

pub(crate) fn search(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via existing `StorageSearchArgs`.
    let StorageSearchArgs { query, limit } = parse_args(args, "search")?;
    let limit = limit
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_SEARCH_LIMIT);
    let results = engine
        .search(&query, limit)
        .map_err(|e| exec_err(format!("search: {e}")))?;
    to_value(&results, "search")
}

/// `com.nexus.storage::hybrid_search` (handler id `76`) — RRF fusion of
/// the Tantivy FTS arm and the vector arm. The reply wire shape
/// `{"results": [...]}` matches the typed
/// `crate::ipc::StorageHybridSearchResult`; `HybridMatch` serialises
/// field-for-field as `StorageHybridMatch` (pinned by the
/// `ipc_schema_emit` invariant).
pub(crate) fn hybrid_search(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageHybridSearchArgs {
        query,
        embedding,
        namespace,
        limit,
    } = parse_args(args, "hybrid_search")?;
    let limit = limit
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_HYBRID_LIMIT);
    let results = engine
        .hybrid_search(&query, &namespace, &embedding, limit)
        .map_err(|e| exec_err(format!("hybrid_search: {e}")))?;
    Ok(serde_json::json!({ "results": results }))
}

pub(crate) fn query_symbol(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // SymbolFilter already round-trips through `parse_args` (the
    // args side is strict — `deny_unknown_fields`). The reply wire
    // shape `{"symbols": [...]}` matches the typed
    // `crate::ipc::StorageQuerySymbolResult`; `SymbolRecord`
    // serialises field-for-field as `StorageSymbolRow` (pinned by
    // the `ipc_schema_emit` invariant), so the existing `json!`
    // envelope here IS the typed wire shape.
    let filter: crate::code_index::SymbolFilter = parse_args(args, "query_symbol")?;
    let symbols = engine
        .query_symbols(&filter)
        .map_err(|e| exec_err(format!("query_symbol: {e}")))?;
    Ok(serde_json::json!({ "symbols": symbols }))
}

pub(crate) fn query_tags(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageQueryTagsArgs`.
    let StorageQueryTagsArgs { name } = parse_args(args, "query_tags")?;
    let tags = engine
        .query_tags(&name)
        .map_err(|e| exec_err(format!("query_tags: {e}")))?;
    to_value(&tags, "query_tags")
}

/// BL-078 — args go straight through to the [`crate::find_in_files`]
/// free function. No engine dependency; the walk uses the `forge_root`
/// the plugin was built with.
pub(crate) fn find_in_files(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::FindInFilesArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("find_in_files: invalid args: {e}")))?;
    let hits = crate::find_in_files(forge_root, &parsed)
        .map_err(|e| exec_err(format!("find_in_files: {e}")))?;
    to_value(&hits, "find_in_files")
}

/// `com.nexus.storage::ast_query` (handler id `75`) — tree-sitter structural
/// code search. Phase 5.2 / RFC 0005.
pub(crate) fn ast_query(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::StorageAstQueryArgs = parse_args(args, "ast_query")?;
    let result = crate::ast_query::ast_query(forge_root, &parsed)
        .map_err(|e| exec_err(format!("ast_query: {e}")))?;
    to_value(&result, "ast_query")
}

/// BL-078 — pass-through to [`crate::replace_in_files`]. After a
/// successful replacement we trigger an index rebuild so search /
/// graph stay consistent with the rewritten files.
pub(crate) fn replace_in_files(
    engine: &StorageEngine,
    forge_root: &Path,
    args: &Value,
) -> Result<Value, PluginError> {
    let parsed: crate::ReplaceInFilesArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("replace_in_files: invalid args: {e}")))?;
    let report = crate::replace_in_files(forge_root, &parsed)
        .map_err(|e| exec_err(format!("replace_in_files: {e}")))?;
    if report.files_changed > 0 {
        let _ = engine.rebuild_index();
    }
    to_value(&report, "replace_in_files")
}
