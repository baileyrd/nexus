//! Filesystem-tree handlers: `list_dir`, `create_file`, `create_dir`,
//! `rename_entry`, `delete_entry`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageListDirArgs, StorageOk, StorageRelpathArgs, StorageRenameEntryArgs,
    StorageRenameEntryResult, StorageTrashEmptyArgs, StorageTrashEmptyResult,
    StorageTrashEntryArgs, StorageTrashEntryResult, StorageTrashListResult,
    StorageTrashRestoreArgs, StorageTrashRestoreResult, StorageTrashRow,
};
use crate::{DeleteDestination, StorageEngine};

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
    let StorageRenameEntryArgs {
        from,
        to,
        update_links,
    } = parse_args(args, "rename_entry")?;
    // C2 (#355) — optionally rewrite inbound links in referencing files.
    let (files_rewritten, links_updated) = engine
        .rename_entry_with_links(&from, &to, update_links)
        .map_err(|e| exec_err(format!("rename_entry: {e}")))?;
    to_value(
        &StorageRenameEntryResult {
            ok: true,
            files_rewritten,
            links_updated,
        },
        "rename_entry",
    )
}

pub(crate) fn delete_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageRelpathArgs { relpath } = parse_args(args, "delete_entry")?;
    engine
        .delete_entry(&relpath)
        .map_err(|e| exec_err(format!("delete_entry: {e}")))?;
    to_value(&StorageOk { ok: true }, "delete_entry")
}

// ── C3 (#356) — trash verbs ──────────────────────────────────────────────────

pub(crate) fn trash_entry(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageTrashEntryArgs {
        relpath,
        destination,
    } = parse_args(args, "trash_entry")?;
    let dest = match destination.as_deref() {
        None | Some("forge") => DeleteDestination::ForgeTrash,
        Some("system") => DeleteDestination::SystemTrash,
        Some(other) => {
            return Err(exec_err(format!(
                "trash_entry: unknown destination '{other}' (expected 'forge' or 'system')"
            )))
        }
    };
    let trash_id = engine
        .delete_entry_to(&relpath, dest)
        .map_err(|e| exec_err(format!("trash_entry: {e}")))?;
    to_value(&StorageTrashEntryResult { ok: true, trash_id }, "trash_entry")
}

pub(crate) fn trash_list(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // Accept `{}` — no parameters.
    let _: serde_json::Map<String, Value> = parse_args(args, "trash_list")?;
    let entries = engine
        .trash_list()
        .map_err(|e| exec_err(format!("trash_list: {e}")))?
        .into_iter()
        .map(|b| StorageTrashRow {
            trash_id: b.trash_id,
            original_path: b.meta.original_path,
            deleted_at_ms: b.meta.deleted_at_ms,
            is_dir: b.meta.is_dir,
            size_bytes: b.size_bytes,
        })
        .collect();
    to_value(&StorageTrashListResult { entries }, "trash_list")
}

pub(crate) fn trash_restore(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageTrashRestoreArgs { trash_id } = parse_args(args, "trash_restore")?;
    let restored_path = engine
        .trash_restore(&trash_id)
        .map_err(|e| exec_err(format!("trash_restore: {e}")))?;
    to_value(&StorageTrashRestoreResult { restored_path }, "trash_restore")
}

pub(crate) fn trash_empty(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageTrashEmptyArgs { older_than_days } = parse_args(args, "trash_empty")?;
    let removed = engine
        .trash_empty(older_than_days)
        .map_err(|e| exec_err(format!("trash_empty: {e}")))?;
    to_value(&StorageTrashEmptyResult { removed }, "trash_empty")
}
