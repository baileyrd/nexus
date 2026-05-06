//! Wire-mirror IPC types for `com.nexus.security`.
//!
//! All credential operations are namespaced by the caller's plugin ID so
//! a plugin cannot read another plugin's secrets. The vault key stored in
//! the OS keyring is `"{plugin_id}:{name}"`.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── get_secret ────────────────────────────────────────────────────────────────

/// Args for `com.nexus.security::get_secret` (handler id `1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GetSecretArgs {
    /// Caller's plugin ID — used as the key namespace.
    pub plugin_id: String,
    /// Short credential name (e.g. `"ssh_passphrase"`, `"api_key"`).
    pub name: String,
}

/// Return type for `com.nexus.security::get_secret`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GetSecretResult {
    /// The stored value, or `null` if not found.
    pub value: Option<String>,
}

// ── set_secret ────────────────────────────────────────────────────────────────

/// Args for `com.nexus.security::set_secret` (handler id `2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SetSecretArgs {
    /// Caller's plugin ID — used as the key namespace.
    pub plugin_id: String,
    /// Short credential name.
    pub name: String,
    /// Secret value to store.
    pub value: String,
}

/// Return type for `com.nexus.security::set_secret`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SetSecretResult {
    /// `true` when the credential was stored successfully.
    pub ok: bool,
}

// ── delete_secret ─────────────────────────────────────────────────────────────

/// Args for `com.nexus.security::delete_secret` (handler id `3`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DeleteSecretArgs {
    /// Caller's plugin ID — used as the key namespace.
    pub plugin_id: String,
    /// Short credential name to delete.
    pub name: String,
}

/// Return type for `com.nexus.security::delete_secret`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DeleteSecretResult {
    /// `true` when the credential was deleted (or did not exist).
    pub ok: bool,
}

// ── list_secret_names ─────────────────────────────────────────────────────────

/// Args for `com.nexus.security::list_secret_names` (handler id `4`).
///
/// Returns only names — never values. A plugin can only enumerate its own
/// secrets (filtered by `plugin_id` prefix in the vault).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListSecretNamesArgs {
    /// Caller's plugin ID — only secrets belonging to this plugin are listed.
    pub plugin_id: String,
}

/// Return type for `com.nexus.security::list_secret_names`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListSecretNamesResult {
    /// Short credential names belonging to the calling plugin.
    /// Only names set during the current session are guaranteed to appear;
    /// names set in previous sessions and not re-set are not enumerable
    /// (the OS keyring does not support listing).
    pub names: Vec<String>,
}
