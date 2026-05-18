//! Tag-domain handlers: `list_tags`, `create_tag`, `delete_tag`,
//! `push_tags`.

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{key_string, map_err};

pub(crate) fn list_tags(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let tags = h.with(|e| e.list_tags()).map_err(map_err)?;
    let arr: Vec<_> = tags
        .iter()
        .map(|t| {
            json!({
                "name":         t.name,
                "target_hash":  t.target_hash,
                "is_annotated": t.is_annotated,
                "message":      t.message,
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

pub(crate) fn create_tag(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let name = key_string(args, "name")?;
    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    h.with(move |e| e.create_tag(&name, message.as_deref()))
        .map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn delete_tag(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let name = key_string(args, "name")?;
    h.with(move |e| e.delete_tag(&name)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn push_tags(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let remote = key_string(args, "remote")?;
    h.with(move |e| e.push_tags(&remote)).map_err(map_err)?;
    Ok(json!({"ok": true}))
}
