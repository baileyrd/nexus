//! Stash-domain handlers: `stash_push`, `stash_list`, `stash_pop`,
//! `stash_drop`.

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::map_err;

fn index_arg(args: &Value) -> usize {
    args.get("index")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0)
}

pub(crate) fn stash_push(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let idx = h
        .with(move |e| e.stash_push(message.as_deref()))
        .map_err(map_err)?;
    Ok(json!({"ok": true, "index": idx}))
}

pub(crate) fn stash_list(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let entries = h.with(|e| e.stash_list()).map_err(map_err)?;
    let arr: Vec<_> = entries
        .iter()
        .map(|s| {
            json!({
                "index":   s.index,
                "message": s.message,
                "oid":     s.oid,
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

pub(crate) fn stash_pop(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let idx = index_arg(args);
    h.with(move |e| e.stash_pop(idx)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn stash_drop(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let idx = index_arg(args);
    h.with(move |e| e.stash_drop(idx)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}
