//! Task + block handlers: `query_tasks`, `toggle_task`, `query_blocks`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{StoragePathArgs, StorageToggleTaskArgs};
use crate::{StorageEngine, TaskFilter};

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn query_tasks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let filter: TaskFilter = parse_args(args, "query_tasks")?;
    let records = engine
        .query_tasks(&filter)
        .map_err(|e| exec_err(format!("query_tasks: {e}")))?;
    to_value(&records, "query_tasks")
}

pub(crate) fn toggle_task(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageToggleTaskArgs`.
    let StorageToggleTaskArgs { task_id } = parse_args(args, "toggle_task")?;
    let record = engine
        .toggle_task(task_id)
        .map_err(|e| exec_err(format!("toggle_task: {e}")))?;
    to_value(&record, "toggle_task")
}

pub(crate) fn query_blocks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StoragePathArgs { path } = parse_args(args, "query_blocks")?;
    let blocks = engine
        .query_blocks_by_path(&path)
        .map_err(|e| exec_err(format!("query_blocks: {e}")))?;
    to_value(&blocks, "query_blocks")
}
