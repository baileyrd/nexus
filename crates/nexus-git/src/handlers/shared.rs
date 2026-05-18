//! Cross-cutting helpers shared by every git handler module.
//!
//! Lifted out of `core_plugin.rs` by the 2026-05-18 SOLID/DRY audit
//! SD-03 decomposition. `pub(crate)` only — these aren't part of the
//! plugin's public surface.

use std::path::{Path, PathBuf};

use nexus_plugins::PluginError;

use crate::{core_plugin::PLUGIN_ID, GitError};

// PLUGIN_ID is consumed by the `define_dispatch_helpers!` invocation
// below — the macro expects an in-scope binding by that name.
const _: &str = PLUGIN_ID;

// Bind the workspace-wide dispatch helpers (`exec_err`, `parse_args`,
// `to_value`, `string_arg`) to this plugin id so they're available to
// every handler module. Closes the SD-01 gap that this crate skipped
// originally because it had no private `exec_err` to consolidate.
nexus_plugins::define_dispatch_helpers!(pub(crate));

/// Extract a forge-relative `path` argument, validate it stays inside
/// `forge_root`, and return it as a `PathBuf` for libgit2's path-based
/// APIs.
///
/// libgit2's path-based API takes a path relative to the repo root,
/// so we discard the joined absolute path and return the validated
/// raw relpath.
pub(crate) fn path_arg(
    args: &serde_json::Value,
    forge_root: &Path,
) -> Result<PathBuf, PluginError> {
    let raw = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| exec_err("missing 'path' argument".to_string()))?;
    nexus_types::paths::resolve_within(forge_root, raw)
        .map_err(|e| exec_err(format!("invalid 'path': {e}")))?;
    Ok(PathBuf::from(raw))
}

/// Extract the `hunk_indices` array from IPC args as `Vec<usize>`.
pub(crate) fn hunk_indices_arg(
    args: &serde_json::Value,
) -> Result<Vec<usize>, PluginError> {
    let arr = args
        .get("hunk_indices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| exec_err("missing 'hunk_indices' array argument".to_string()))?;
    arr.iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| {
                    exec_err(
                        "hunk_indices entries must be non-negative integers".to_string(),
                    )
                })
        })
        .collect()
}

/// Extract a plain string argument by its key name.
///
/// Distinct from the macro-emitted `string_arg(value, command, field)`
/// which requires a command name for error messages. Keeping the
/// historical two-arg shape here means existing call sites don't have
/// to thread a command label.
pub(crate) fn key_string(
    args: &serde_json::Value,
    key: &str,
) -> Result<String, PluginError> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("missing '{key}' argument")))
}

// Passed as a function pointer to `.map_err(map_err)`; wrapping in a
// closure would re-trip `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn map_err(e: GitError) -> PluginError {
    exec_err(e.to_string())
}
