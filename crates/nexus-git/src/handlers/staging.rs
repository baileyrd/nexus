//! Staging-domain handlers: `stage_file`, `unstage_file`, `stage_all`,
//! `unstage_all`, `stage_hunks`, `unstage_hunks`, `discard_hunks`,
//! `commit`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{hunk_indices_arg, key_string, map_err, path_arg};

pub(crate) fn stage_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    h.with(move |e| e.stage_file(&path)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn unstage_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    h.with(move |e| e.unstage_file(&path)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn stage_all(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.stage_all()).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn unstage_all(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.unstage_all()).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn stage_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let indices = hunk_indices_arg(args)?;
    h.with(move |e| e.stage_hunks(&path, &indices))
        .map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn unstage_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let indices = hunk_indices_arg(args)?;
    h.with(move |e| e.unstage_hunks(&path, &indices))
        .map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn discard_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let indices = hunk_indices_arg(args)?;
    h.with(move |e| e.discard_hunks(&path, &indices))
        .map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn commit(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let msg = key_string(args, "message")?;
    let hash = h.with(move |e| e.commit(&msg)).map_err(map_err)?;
    Ok(json!({"hash": hash}))
}
