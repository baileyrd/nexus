//! Tag-domain handlers: `list_tags`, `create_tag`, `delete_tag`,
//! `push_tags`.
//!
//! #190 / R7 — these handlers previously read `name` / `remote` off
//! `serde_json::Value` via `key_string` and emitted ad-hoc
//! `json!({"ok": true})` replies, so they were invisible to both the
//! `ipc_strictness` gate and the schemars schema generator. Routing
//! through `parse_args::<…>` and `to_value(&GitOk { ok: true })`
//! brings them under the same `deny_unknown_fields` + drift
//! guarantees the rest of the storage / branch handlers already
//! have.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{GitCreateTagArgs, GitDeleteTagArgs, GitOk, GitPushTagsArgs, GitTagInfo};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value};

pub(crate) fn list_tags(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let tags = h.with(|e| e.list_tags()).map_err(map_err)?;
    let arr: Vec<GitTagInfo> = tags
        .into_iter()
        .map(|t| GitTagInfo {
            name: t.name,
            target_hash: t.target_hash,
            is_annotated: t.is_annotated,
            message: t.message,
        })
        .collect();
    to_value(&arr, "list_tags")
}

pub(crate) fn create_tag(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitCreateTagArgs { name, message } = parse_args(args, "create_tag")?;
    h.with(move |e| e.create_tag(&name, message.as_deref()))
        .map_err(map_err)?;
    to_value(&GitOk { ok: true }, "create_tag")
}

pub(crate) fn delete_tag(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitDeleteTagArgs { name } = parse_args(args, "delete_tag")?;
    h.with(move |e| e.delete_tag(&name)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "delete_tag")
}

pub(crate) fn push_tags(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitPushTagsArgs { remote } = parse_args(args, "push_tags")?;
    h.with(move |e| e.push_tags(&remote)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "push_tags")
}
