//! Cross-cutting helpers for storage handler modules.
//!
//! Lifted out of `core_plugin.rs` by the 2026-05-18 SOLID/DRY audit
//! SD-03 (Phase A). `pub(crate)` only.

use crate::core_plugin::PLUGIN_ID;

// Bind the workspace-wide dispatch helpers (`exec_err`, `parse_args`,
// `to_value`, `string_arg`) to this plugin id. Same shape as the
// in-`core_plugin.rs` invocation; handler modules import from here
// rather than reaching across into the parent module.
const _: &str = PLUGIN_ID;
nexus_plugins::define_dispatch_helpers!(pub(crate));

/// True iff `path` is a forge-relative path inside the `.forge/`
/// metadata directory (the namespace `HANDLER_WRITE_VAULT_FILE` is
/// documented to own — workspace.json, kv.sqlite3 sidecars, plugin
/// state, etc.). Accepts both `/`-separated POSIX paths and
/// `\`-separated Windows-style paths so the check does the right
/// thing regardless of the platform-native separator the caller
/// happens to send.
pub(crate) fn is_forge_metadata_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized == ".forge" || normalized.starts_with(".forge/")
}
