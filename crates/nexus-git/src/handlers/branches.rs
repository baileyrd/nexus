//! Branch-domain handlers: `branches`, `switch_branch`,
//! `create_branch`, `delete_branch`, `push`.

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{key_string, map_err};

pub(crate) fn branches(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let branches = h.with(|e| e.branches()).map_err(map_err)?;
    let arr: Vec<_> = branches
        .iter()
        .map(|b| {
            json!({
                "name": b.name,
                "is_head": b.is_head,
                "upstream": b.upstream,
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

pub(crate) fn switch_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let name = key_string(args, "name")?;
    h.with(move |e| e.switch_branch(&name)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn create_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let name = key_string(args, "name")?;
    h.with(move |e| e.create_branch(&name)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn delete_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let name = key_string(args, "name")?;
    h.with(move |e| e.delete_branch(&name)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn push(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let remote = key_string(args, "remote")?;
    let branch = key_string(args, "branch")?;
    h.with(move |e| e.push(&remote, &branch)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}
