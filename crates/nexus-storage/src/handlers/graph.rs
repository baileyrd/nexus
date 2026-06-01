//! Knowledge-graph handlers: `backlinks`, `backlinks_to_block`,
//! `outgoing_links`, `unresolved_links`, `list_all_links`,
//! `graph_stats`, `graph_neighbors`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{StorageBacklinksToBlockArgs, StorageGraphNeighborsArgs, StoragePathArgs};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn backlinks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StoragePathArgs { path } = parse_args(args, "backlinks")?;
    let results = engine
        .backlinks(&path)
        .map_err(|e| exec_err(format!("backlinks: {e}")))?;
    to_value(&results, "backlinks")
}

pub(crate) fn backlinks_to_block(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBacklinksToBlockArgs`.
    let StorageBacklinksToBlockArgs { path, block_id } = parse_args(args, "backlinks_to_block")?;
    let results = engine
        .backlinks_to_block(&path, &block_id)
        .map_err(|e| exec_err(format!("backlinks_to_block: {e}")))?;
    to_value(&results, "backlinks_to_block")
}

pub(crate) fn outgoing_links(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StoragePathArgs { path } = parse_args(args, "outgoing_links")?;
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
    // #190 / R7 — strict-parse via `StorageGraphNeighborsArgs`.
    let StorageGraphNeighborsArgs { path, depth } = parse_args(args, "graph_neighbors")?;
    let depth = usize::try_from(depth)
        .map_err(|e| exec_err(format!("graph_neighbors: depth out of range: {e}")))?;
    let paths = engine
        .graph_neighbors(&path, depth)
        .map_err(|e| exec_err(format!("graph_neighbors: {e}")))?;
    to_value(&paths, "graph_neighbors")
}
