//! Filesystem-tree handlers: `list_dir`, `create_file`, `create_dir`,
//! `rename_entry`, `delete_entry`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{StorageListDirArgs, StorageOk, StorageRelpathArgs, StorageRenameEntryArgs};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn list_dir(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via existing `StorageListDirArgs`
    // (already typed; `relpath` defaults to "" via `#[serde(default)]`).
    let StorageListDirArgs { relpath } = parse_args(args, "list_dir")?;
    let entries = engine
        .list_dir(&relpath)
        .map_err(|e| exec_err(format!("list_dir: {e}")))?;
    to_value(&entries, "list_dir")
}

pub(crate) fn create_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via shared `StorageRelpathArgs`.
    let StorageRelpathArgs { relpath } = parse_args(args, "create_file")?;
    engine
        .create_file(&relpath)
        .map_err(|e| exec_err(format!("create_file: {e}")))?;
    to_value(&StorageOk { ok: true }, "create_file")
}

pub(crate) fn create_dir(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageRelpathArgs { relpath } = parse_args(args, "create_dir")?;
    engine
        .create_dir(&relpath)
        .map_err(|e| exec_err(format!("create_dir: {e}")))?;
    to_value(&StorageOk { ok: true }, "create_dir")
}

pub(crate) fn rename_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageRenameEntryArgs`.
    let StorageRenameEntryArgs { from, to } = parse_args(args, "rename_entry")?;
    engine
        .rename_entry(&from, &to)
        .map_err(|e| exec_err(format!("rename_entry: {e}")))?;
    to_value(&StorageOk { ok: true }, "rename_entry")
}

pub(crate) fn delete_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageRelpathArgs { relpath } = parse_args(args, "delete_entry")?;
    engine
        .delete_entry(&relpath)
        .map_err(|e| exec_err(format!("delete_entry: {e}")))?;
    to_value(&StorageOk { ok: true }, "delete_entry")
}
