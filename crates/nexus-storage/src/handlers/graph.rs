//! Knowledge-graph handlers: `backlinks`, `backlinks_to_block`,
//! `outgoing_links`, `unresolved_links`, `list_all_links`,
//! `graph_stats`, `graph_neighbors`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, path_arg, to_value};

pub(crate) fn backlinks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "backlinks")?;
    let results = engine
        .backlinks(&path)
        .map_err(|e| exec_err(format!("backlinks: {e}")))?;
    to_value(&results, "backlinks")
}

pub(crate) fn backlinks_to_block(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "backlinks_to_block")?;
    let block_id = args
        .get("block_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("backlinks_to_block: missing 'block_id' string".to_string()))?;
    let results = engine
        .backlinks_to_block(&path, block_id)
        .map_err(|e| exec_err(format!("backlinks_to_block: {e}")))?;
    to_value(&results, "backlinks_to_block")
}

pub(crate) fn outgoing_links(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "outgoing_links")?;
    let links = engine
        .outgoing_links(&path)
        .map_err(|e| exec_err(format!("outgoing_links: {e}")))?;
    to_value(&links, "outgoing_links")
}

pub(crate) fn unresolved_links(engine: &StorageEngine) -> Result<Value, PluginError> {
    let links = engine
        .unresolved_links()
        .map_err(|e| exec_err(format!("unresolved_links: {e}")))?;
    to_value(&links, "unresolved_links")
}

pub(crate) fn list_all_links(engine: &StorageEngine) -> Result<Value, PluginError> {
    let snapshot = engine
        .list_all_links()
        .map_err(|e| exec_err(format!("list_all_links: {e}")))?;
    to_value(&snapshot, "list_all_links")
}

pub(crate) fn graph_stats(engine: &StorageEngine) -> Result<Value, PluginError> {
    let stats = engine
        .graph_stats()
        .map_err(|e| exec_err(format!("graph_stats: {e}")))?;
    to_value(&stats, "graph_stats")
}

pub(crate) fn graph_neighbors(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "graph_neighbors")?;
    let depth = args
        .get("depth")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| exec_err("graph_neighbors: missing 'depth' (u64)".to_string()))?;
    let paths = engine
        .graph_neighbors(&path, depth)
        .map_err(|e| exec_err(format!("graph_neighbors: {e}")))?;
    to_value(&paths, "graph_neighbors")
}
