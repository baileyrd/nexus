//! File-domain handlers: `query_files`, `read_file`, `write_file`,
//! `delete_file`, `file_exists`, `write_vault_file`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::{FileFilter, StorageEngine};

use super::shared::{exec_err, is_forge_metadata_path, parse_args, path_arg, to_value};

pub(crate) fn query_files(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let filter: FileFilter = parse_args(args, "query_files")?;
    let records = engine
        .query_files(&filter)
        .map_err(|e| exec_err(format!("query_files: {e}")))?;
    to_value(&records, "query_files")
}

pub(crate) fn read_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "read_file")?;
    match engine.read_file(&path) {
        Ok(bytes) => Ok(serde_json::json!({ "bytes": bytes })),
        // Missing files are an expected outcome for callers probing
        // `.forge/workspace.json` on first boot, etc. Return a typed
        // null rather than an error so the IPC bridge doesn't surface
        // it as `PluginCrashedDuringCall`.
        Err(crate::StorageError::FileNotFound(_)) => Ok(serde_json::json!({ "bytes": null })),
        Err(e) => Err(exec_err(format!("read_file '{path}': {e}"))),
    }
}

pub(crate) fn write_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "write_file")?;
    let bytes: Vec<u8> = args
        .get("bytes")
        .ok_or_else(|| exec_err(format!("write_file '{path}': missing 'bytes'")))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("write_file '{path}': bytes decode: {e}")))
        })?;
    let meta = engine
        .write_file(&path, &bytes)
        .map_err(|e| exec_err(format!("write_file '{path}' ({} bytes): {e}", bytes.len())))?;
    to_value(&meta, "write_file")
}

pub(crate) fn delete_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "delete_file")?;
    engine
        .delete_file(&path)
        .map_err(|e| exec_err(format!("delete_file '{path}': {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn file_exists(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "file_exists")?;
    let exists = engine
        .file_exists(&path)
        .map_err(|e| exec_err(format!("file_exists '{path}': {e}")))?;
    Ok(serde_json::json!({ "exists": exists }))
}

pub(crate) fn write_vault_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "write_vault_file")?;
    // The handler is documented as ".forge/-prefixed shell metadata
    // only" — `write_raw` skips FTS, graph, and watcher updates, so a
    // vault path (e.g. `notes/foo.md`) written here would silently
    // diverge from the index. Confine to the `.forge/` subdirectory;
    // user-facing writes must go through `HANDLER_WRITE_FILE`. See
    // issue #80.
    if !is_forge_metadata_path(&path) {
        return Err(exec_err(format!(
            "write_vault_file: '{path}' is outside the .forge/ \
             metadata namespace; vault writes must go through write_file"
        )));
    }
    let bytes: Vec<u8> = args
        .get("bytes")
        .ok_or_else(|| exec_err(format!("write_vault_file '{path}': missing 'bytes'")))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("write_vault_file '{path}': bytes decode: {e}")))
        })?;
    engine.write_raw(&path, &bytes).map_err(|e| {
        exec_err(format!(
            "write_vault_file '{path}' ({} bytes): {e}",
            bytes.len()
        ))
    })?;
    Ok(serde_json::json!({}))
}
