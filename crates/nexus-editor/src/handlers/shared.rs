//! Cross-cutting helpers for editor handler modules.
//!
//! Lifted out of `core_plugin.rs` by the 2026-05-18 SOLID/DRY audit
//! SD-03 (editor split). `pub(crate)` only. Mirrors
//! `crates/nexus-storage/src/handlers/shared.rs`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::core_plugin::PLUGIN_ID;

// Bind the workspace-wide dispatch helpers (`exec_err`, `parse_args`,
// `to_value`, `string_arg`) to this plugin id. Same shape as the
// in-`core_plugin.rs` invocation; handler modules import from here
// rather than reaching across into the parent module.
const _: &str = PLUGIN_ID;
nexus_plugins::define_dispatch_helpers!(pub(crate));

/// Re-exports of session-state helpers and shared constants still
/// owned by `core_plugin`. Once the SD-03 split for the editor lands
/// in full, these can move here outright; during the chunk-by-chunk
/// migration we keep them in place to minimise churn.
pub(crate) use crate::core_plugin::{
    acquire_session_entry, get_session_entry, insert_session_entry, publish_changed,
    remove_session_entry, resolve_within, sessions_poisoned, snapshot_of, snapshot_to_value,
    DATABASE_PLUGIN_ID, MULTIBUFFER_RELPATH_PREFIX, STORAGE_IPC_TIMEOUT, STORAGE_PLUGIN_ID,
};

/// Convenience wrapper — the editor handlers historically used
/// `relpath_arg(args, "cmd")` rather than spelling
/// `string_arg(args, "cmd", "relpath")` at every call site. Keeps the
/// idiom while routing through the workspace-wide macro-emitted
/// `string_arg`.
pub(crate) fn relpath_arg(args: &Value, command: &str) -> Result<String, PluginError> {
    string_arg(args, command, "relpath")
}
