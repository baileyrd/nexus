//! Task + block handlers: `query_tasks`, `toggle_task`, `query_blocks`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::{StorageEngine, TaskFilter};

use super::shared::{exec_err, parse_args, path_arg, to_value};

pub(crate) fn query_tasks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let filter: TaskFilter = parse_args(args, "query_tasks")?;
    let records = engine
        .query_tasks(&filter)
        .map_err(|e| exec_err(format!("query_tasks: {e}")))?;
    to_value(&records, "query_tasks")
}

pub(crate) fn toggle_task(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let task_id = args
        .get("task_id")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| exec_err("toggle_task: missing 'task_id' (u64)".to_string()))?;
    let record = engine
        .toggle_task(task_id)
        .map_err(|e| exec_err(format!("toggle_task: {e}")))?;
    to_value(&record, "toggle_task")
}

pub(crate) fn query_blocks(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "query_blocks")?;
    let blocks = engine
        .query_blocks_by_path(&path)
        .map_err(|e| exec_err(format!("query_blocks: {e}")))?;
    to_value(&blocks, "query_blocks")
}
