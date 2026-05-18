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
pub(crate) fn config_kind(
    args: &serde_json::Value,
) -> Result<&str, PluginError> {
    args.get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("config: missing 'kind' string argument".to_string()))
}
