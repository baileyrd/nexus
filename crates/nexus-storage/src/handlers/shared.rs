//! Cross-cutting helpers for storage handler modules.
//!
//! Lifted out of `core_plugin.rs` by the 2026-05-18 SOLID/DRY audit
//! SD-03 (Phase A). `pub(crate)` only.

use nexus_plugins::PluginError;

use crate::core_plugin::PLUGIN_ID;

// Bind the workspace-wide dispatch helpers (`exec_err`, `parse_args`,
// `to_value`, `string_arg`) to this plugin id. Same shape as the
// in-`core_plugin.rs` invocation; handler modules import from here
// rather than reaching across into the parent module.
const _: &str = PLUGIN_ID;
nexus_plugins::define_dispatch_helpers!(pub(crate));

/// Read the `kind` string argument used by `config_read` /
/// `config_reset`. Returns the borrowed `&str` so callers can match
/// against literals without an extra allocation.
pub(crate) fn config_kind(args: &serde_json::Value) -> Result<&str, PluginError> {
    args.get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("config: missing 'kind' string argument".to_string()))
}

/// Convenience wrappers — the storage handlers historically named the
/// three common string-arg fields (`path`, `relpath`, `name`) via
/// dedicated helpers so call sites read as `path_arg(args, "cmd")`
/// rather than `string_arg(args, "cmd", "path")`. Keeping the wrappers
/// preserves call-site clarity while delegating the lookup logic to
/// the workspace-wide macro-emitted `string_arg`.
pub(crate) fn path_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    string_arg(value, command, "path")
}

pub(crate) fn relpath_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    string_arg(value, command, "relpath")
}

pub(crate) fn name_arg(value: &serde_json::Value, command: &str) -> Result<String, PluginError> {
    string_arg(value, command, "name")
}

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
