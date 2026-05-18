//! Tree-domain handlers: `get_tree`, `get_markdown`, `stamp_block`,
//! `list_open`.
//!
//! Lifted from `core_plugin.rs` by SD-03 editor chunk 1
//! (2026-05-18 SOLID/DRY audit).

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::PluginError;
use serde_json::Value;

use crate::core_plugin::SessionMap;
use crate::markdown::MarkdownSerializer;

use super::shared::{
    acquire_session_entry, exec_err, publish_changed, relpath_arg, sessions_poisoned, snapshot_of,
    snapshot_to_value,
};

pub(crate) fn get_tree(sessions: &SessionMap, args: &Value) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_tree")?;
    let entry = acquire_session_entry(sessions, &relpath, "get_tree")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    snapshot_to_value(&snapshot_of(&s), "get_tree")
}

/// Serialize the session's block tree to markdown and return it as a
/// bare JSON string. Matches `serialize_session` but surfaces the
/// result over IPC rather than routing it to disk.
pub(crate) fn get_markdown(sessions: &SessionMap, args: &Value) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "get_markdown")?;
    let entry = acquire_session_entry(sessions, &relpath, "get_markdown")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    let markdown = MarkdownSerializer::serialize(&s.tree);
    Ok(Value::String(markdown))
}

pub(crate) fn list_open(sessions: &SessionMap) -> Result<Value, PluginError> {
    let guard = sessions.lock().map_err(|_| sessions_poisoned())?;
    let mut paths: Vec<String> = guard.keys().cloned().collect();
    paths.sort();
    serde_json::to_value(paths).map_err(|e| exec_err(format!("list_open: serialize: {e}")))
}

/// Stamp the addressed block with a fresh v4 stable id so the next
/// `save` writes a `<!-- ^<uuid> -->` marker and the id survives
/// upstream insertions on reload (ADR 0017). Idempotent: a second
/// call against an already-stamped block returns the existing stamp
/// without bumping the session revision or publishing a changed
/// event.
///
/// The block is rekeyed via [`crate::BlockTree::rekey`] from its
/// current positional id to the fresh stamp; references in the
/// parent's `children` list, `root_blocks`, and child blocks'
/// `parent_id` are all updated together. After rekey, the block's
/// `id` and `stable_id` are equal — the lookup `block_id` arg passed
/// in is returned as `block_id` in the response so the caller can
/// still reference it, while `stable_id` carries the new uuid that's
/// now the canonical key.
pub(crate) fn stamp_block(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "stamp_block")?;
    let block_id_str = args
        .get("block_id")
        .and_then(Value::as_str)
        .ok_or_else(|| exec_err("stamp_block: missing 'block_id' string".to_string()))?;
    let block_id = uuid::Uuid::parse_str(block_id_str)
        .map_err(|e| exec_err(format!("stamp_block: invalid 'block_id': {e}")))?;

    let entry = acquire_session_entry(sessions, &relpath, "stamp_block")?;
    let (stable_id, newly_stamped, revision) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s = &mut *guard;
        let block = s.tree.get(block_id).ok_or_else(|| {
            exec_err(format!(
                "stamp_block: block '{block_id}' not present in '{relpath}'"
            ))
        })?;
        if let Some(existing) = block.stable_id {
            // Already stamped: return the existing stamp untouched.
            (existing, false, s.revision)
        } else {
            let new_id = uuid::Uuid::new_v4();
            s.tree
                .rekey(block_id, new_id)
                .map_err(|e| exec_err(format!("stamp_block: rekey: {e}")))?;
            s.revision = s.revision.saturating_add(1);
            (new_id, true, s.revision)
        }
    };

    if newly_stamped {
        publish_changed(event_bus, &relpath, revision, None);
    }

    Ok(serde_json::json!({
        "block_id": block_id,
        "stable_id": stable_id,
        "newly_stamped": newly_stamped,
    }))
}
