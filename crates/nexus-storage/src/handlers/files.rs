//! File-domain handlers: `query_files`, `read_file`, `write_file`,
//! `delete_file`, `file_exists`, `write_vault_file`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageFileExistsResult, StorageOk, StoragePathArgs, StorageReadFileArgs,
    StorageReadFileResult, StorageWriteFileArgs,
};
use crate::{FileFilter, StorageEngine};

use super::shared::{exec_err, is_forge_metadata_path, parse_args, to_value};

pub(crate) fn query_files(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let filter: FileFilter = parse_args(args, "query_files")?;
    let records = engine
        .query_files(&filter)
        .map_err(|e| exec_err(format!("query_files: {e}")))?;
    to_value(&records, "query_files")
}

pub(crate) fn read_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — typed args + result via the existing
    // `StorageReadFileArgs` / `StorageReadFileResult` in `ipc.rs`,
    // both of which carry `#[serde(deny_unknown_fields)]`. The
    // previous hand-rolled `path_arg` lookup + `json!` reply was
    // invisible to both the `ipc_strictness` gate and the schemars
    // schema generator (see `crates/nexus-bootstrap/tests/
    // ipc_strictness.rs`); routing through `parse_args`/`to_value`
    // brings it under the same drift + unknown-field guarantees the
    // rest of the storage handlers already have.
    let typed: StorageReadFileArgs = parse_args(args, "read_file")?;
    let path = typed.path;
    let bytes = match engine.read_file(&path) {
        Ok(b) => Some(b),
        // Missing files are an expected outcome for callers probing
        // `.forge/workspace.json` on first boot, etc. Return a typed
        // null rather than an error so the IPC bridge doesn't surface
        // it as `PluginCrashedDuringCall`.
        Err(crate::StorageError::FileNotFound(_)) => None,
        Err(e) => return Err(exec_err(format!("read_file '{path}': {e}"))),
    };
    to_value(&StorageReadFileResult { bytes }, "read_file")
}

pub(crate) fn write_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageWriteFileArgs`.
    let StorageWriteFileArgs { path, bytes } = parse_args(args, "write_file")?;
    let meta = engine
        .write_file(&path, &bytes)
        .map_err(|e| exec_err(format!("write_file '{path}' ({} bytes): {e}", bytes.len())))?;
    to_value(&meta, "write_file")
}

pub(crate) fn delete_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via the shared `StoragePathArgs`.
    let StoragePathArgs { path } = parse_args(args, "delete_file")?;
    engine
        .delete_file(&path)
        .map_err(|e| exec_err(format!("delete_file '{path}': {e}")))?;
    to_value(&StorageOk { ok: true }, "delete_file")
}

pub(crate) fn file_exists(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via the shared `StoragePathArgs`,
    // typed reply via `StorageFileExistsResult`.
    let StoragePathArgs { path } = parse_args(args, "file_exists")?;
    let exists = engine
        .file_exists(&path)
        .map_err(|e| exec_err(format!("file_exists '{path}': {e}")))?;
    to_value(&StorageFileExistsResult { exists }, "file_exists")
}

pub(crate) fn write_vault_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageWriteFileArgs` (same wire
    // shape as `write_file`). The metadata-namespace confinement
    // check runs after the parse so a typo in `bytes` surfaces as
    // an invalid-args error rather than getting masked by the
    // namespace error.
    let StorageWriteFileArgs { path, bytes } = parse_args(args, "write_vault_file")?;
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
    engine.write_raw(&path, &bytes).map_err(|e| {
        exec_err(format!(
            "write_vault_file '{path}' ({} bytes): {e}",
            bytes.len()
        ))
    })?;
    to_value(&StorageOk { ok: true }, "write_vault_file")
}
