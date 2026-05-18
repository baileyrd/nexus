//! Filesystem-tree handlers: `list_dir`, `create_file`, `create_dir`,
//! `rename_entry`, `delete_entry`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, relpath_arg, to_value};

pub(crate) fn list_dir(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let relpath = args
        .get("relpath")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let entries = engine
        .list_dir(&relpath)
        .map_err(|e| exec_err(format!("list_dir: {e}")))?;
    to_value(&entries, "list_dir")
}

pub(crate) fn create_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "create_file")?;
    engine
        .create_file(&relpath)
        .map_err(|e| exec_err(format!("create_file: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn create_dir(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "create_dir")?;
    engine
        .create_dir(&relpath)
        .map_err(|e| exec_err(format!("create_dir: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn rename_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let from = args
        .get("from")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("rename_entry: missing 'from' string".to_string()))?;
    let to = args
        .get("to")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("rename_entry: missing 'to' string".to_string()))?;
    engine
        .rename_entry(from, to)
        .map_err(|e| exec_err(format!("rename_entry: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn delete_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "delete_entry")?;
    engine
        .delete_entry(&relpath)
        .map_err(|e| exec_err(format!("delete_entry: {e}")))?;
    Ok(serde_json::json!({}))
}
