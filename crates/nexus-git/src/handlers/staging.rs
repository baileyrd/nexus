//! Staging-domain handlers: `stage_file`, `unstage_file`, `stage_all`,
//! `unstage_all`, `stage_hunks`, `unstage_hunks`, `discard_hunks`,
//! `commit`.
//!
//! #190 / R7 — all eight handlers previously read fields off
//! `serde_json::Value` via hand-rolled `path_arg` / `hunk_indices_arg`
//! / `key_string` helpers and emitted ad-hoc `json!({"ok": true})`
//! replies. The typed shapes (`GitPathArgs`, `GitHunkArgs`,
//! `GitCommitArgs`, `GitCommitReply`, `GitOk`) all live in
//! `crate::ipc` with `deny_unknown_fields`. The handlers now route
//! through `parse_args::<…>` for argument parsing (strict against
//! typos like `{ pathh: "foo" }`) and `to_value(…)` for replies, so
//! the family is policed by the `ipc_strictness` gate.
//!
//! Path validation against `forge_root` still happens — it's just
//! moved to the post-parse `validate_path` helper since
//! `GitHunkArgs` carries `{path, hunk_indices}` and the original
//! `path_arg` helper can't strict-parse a multi-field shape.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{GitCommitArgs, GitCommitReply, GitHunkArgs, GitOk, GitPathArgs};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value, validate_path};

pub(crate) fn stage_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let GitPathArgs { path } = parse_args(args, "stage_file")?;
    let path = validate_path(forge_root, &path)?;
    h.with(move |e| e.stage_file(&path)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "stage_file")
}

pub(crate) fn unstage_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let GitPathArgs { path } = parse_args(args, "unstage_file")?;
    let path = validate_path(forge_root, &path)?;
    h.with(move |e| e.unstage_file(&path)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "unstage_file")
}

pub(crate) fn stage_all(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.stage_all()).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "stage_all")
}

pub(crate) fn unstage_all(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.unstage_all()).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "unstage_all")
}

fn hunk_indices_usize(indices: Vec<u64>, command: &str) -> Result<Vec<usize>, PluginError> {
    indices
        .into_iter()
        .map(|n| {
            usize::try_from(n).map_err(|_| {
                super::shared::exec_err(format!("{command}: hunk_indices entry {n} exceeds usize"))
            })
        })
        .collect()
}

pub(crate) fn stage_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let GitHunkArgs { path, hunk_indices } = parse_args(args, "stage_hunks")?;
    let path = validate_path(forge_root, &path)?;
    let indices = hunk_indices_usize(hunk_indices, "stage_hunks")?;
    h.with(move |e| e.stage_hunks(&path, &indices))
        .map_err(map_err)?;
    to_value(&GitOk { ok: true }, "stage_hunks")
}

pub(crate) fn unstage_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let GitHunkArgs { path, hunk_indices } = parse_args(args, "unstage_hunks")?;
    let path = validate_path(forge_root, &path)?;
    let indices = hunk_indices_usize(hunk_indices, "unstage_hunks")?;
    h.with(move |e| e.unstage_hunks(&path, &indices))
        .map_err(map_err)?;
    to_value(&GitOk { ok: true }, "unstage_hunks")
}

pub(crate) fn discard_hunks(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let GitHunkArgs { path, hunk_indices } = parse_args(args, "discard_hunks")?;
    let path = validate_path(forge_root, &path)?;
    let indices = hunk_indices_usize(hunk_indices, "discard_hunks")?;
    h.with(move |e| e.discard_hunks(&path, &indices))
        .map_err(map_err)?;
    to_value(&GitOk { ok: true }, "discard_hunks")
}

pub(crate) fn commit(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitCommitArgs { message } = parse_args(args, "commit")?;
    let hash = h.with(move |e| e.commit(&message)).map_err(map_err)?;
    to_value(&GitCommitReply { hash }, "commit")
}
