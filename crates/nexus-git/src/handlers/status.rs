//! Status-domain handlers: `status`, `file_status`, `file_statuses`,
//! `blame`, `lfs_status`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{map_err, path_arg};

pub(crate) fn status(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let state = h.with(|e| e.state()).map_err(map_err)?;
    Ok(json!({
        "branch": state.branch,
        "head": state.head_oid,
        "is_dirty": state.is_dirty,
        "repo_state": format!("{:?}", state.repo_state),
    }))
}

pub(crate) fn file_status(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let status = h.with(move |e| e.file_status(&path)).map_err(map_err)?;
    Ok(json!(status.marker()))
}

pub(crate) fn file_statuses(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let statuses = h.with(|e| e.file_statuses()).map_err(map_err)?;
    let arr: Vec<_> = statuses
        .iter()
        .map(|s| {
            json!({
                "path": s.path.to_string_lossy(),
                "status": format!("{:?}", s.status),
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

pub(crate) fn blame(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    // BL-079 — wraps `BlameEntry` into the wire-mirror shape. The impl
    // type doesn't derive `Serialize` and carries a `chrono::DateTime`
    // we need to render as ISO-8601 for the shell side.
    let path = path_arg(args, forge_root)?;
    let entries = h.with(move |e| e.blame(&path)).map_err(map_err)?;
    let arr: Vec<_> = entries
        .iter()
        .map(|e| {
            json!({
                "commit_hash": e.commit_hash,
                "author": e.author,
                "date": e.date.to_rfc3339(),
                "message": e.message,
                "start_line": e.start_line,
                "end_line": e.end_line,
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

/// BL-091 — snapshot of Git-LFS state for `lfs_status`.
///
/// Inspects `<forge>/.gitattributes` for `filter=lfs` rules and (if
/// the `git-lfs` binary is on `PATH`) shells out to `git lfs
/// ls-files` to classify tracked files as pointer-only vs locally-
/// materialised. Designed to be robust to `git-lfs` being absent:
/// in that case `git_lfs_installed = false`, `tracked_patterns` is
/// still populated from `.gitattributes`, and the file lists are
/// empty (signalling "we know LFS is in use here but cannot inspect
/// availability").
pub(crate) fn lfs_status(forge_root: &Path) -> Value {
    let tracked_patterns = read_lfs_patterns(forge_root);
    let git_lfs_installed = std::process::Command::new("git")
        .args(["lfs", "version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir(forge_root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let (pointer_files, available_files) = if git_lfs_installed {
        match std::process::Command::new("git")
            .args(["lfs", "ls-files"])
            .current_dir(forge_root)
            .output()
        {
            Ok(o) if o.status.success() => parse_lfs_ls_files(&o.stdout),
            Ok(o) => {
                tracing::warn!(
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "BL-091: `git lfs ls-files` exited non-zero",
                );
                (Vec::new(), Vec::new())
            }
            Err(e) => {
                tracing::warn!(error = %e, "BL-091: failed to spawn `git lfs ls-files`");
                (Vec::new(), Vec::new())
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };

    json!({
        "tracked_patterns": tracked_patterns,
        "pointer_files": pointer_files,
        "available_files": available_files,
        "git_lfs_installed": git_lfs_installed,
    })
}

fn read_lfs_patterns(forge_root: &Path) -> Vec<String> {
    let path = forge_root.join(".gitattributes");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.contains("filter=lfs") {
            continue;
        }
        if let Some(pat) = line.split_whitespace().next() {
            out.push(pat.to_string());
        }
    }
    out
}

fn parse_lfs_ls_files(stdout: &[u8]) -> (Vec<String>, Vec<String>) {
    let text = String::from_utf8_lossy(stdout);
    let mut pointer_files = Vec::new();
    let mut available_files = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let _oid = parts.next();
        let flag = parts.next();
        let path = parts.next();
        let (Some(flag), Some(path)) = (flag, path) else {
            continue;
        };
        match flag {
            "*" => available_files.push(path.to_string()),
            "-" => pointer_files.push(path.to_string()),
            _ => {}
        }
    }
    (pointer_files, available_files)
}
