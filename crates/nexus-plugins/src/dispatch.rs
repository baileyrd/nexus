//! Shared dispatch helpers for [`CorePlugin`](crate::CorePlugin) implementors.
//!
//! Every service crate's `core_plugin.rs` historically declared the same
//! private `exec_err` / `parse_args` / `to_value` / `*_arg` helpers (19
//! redefinitions counted by the 2026-05-18 SOLID/DRY audit). This module
//! is their single source of truth — the per-crate wrappers are now
//! emitted by [`crate::define_dispatch_helpers!`] so error formatting
//! stays uniform across the workspace.
//!
//! Callers don't typically reference these free fns directly. Each
//! service crate invokes [`crate::define_dispatch_helpers!`] once in
//! the module that owns its `PLUGIN_ID` constant, which expands to
//! local `exec_err` / `parse_args` / `to_value` / `string_arg` fns
//! that close over `PLUGIN_ID` and delegate here.
//!
//! See `docs/0.1.2/audits/solid-dry-assessment-2026-05-18.md` SD-01.

use serde::{de::DeserializeOwned, Serialize};

use crate::error::PluginError;

/// Build a [`PluginError::ExecutionFailed`] attributed to `plugin_id`.
///
/// `reason` is a `String` to keep call-site type inference simple —
/// `format!(...)` and `.to_string()` both produce `String` directly,
/// while a bare `impl Into<String>` parameter creates inference
/// ambiguity when crates like `bytes` / `reqwest` / `winnow` are in
/// the dep graph (multiple `From<&str>` impls).
pub fn exec_err(plugin_id: &str, reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: plugin_id.to_string(),
        reason,
    }
}

/// Decode IPC `args` into a typed struct.
///
/// **Wire-shape contract** (matches the historical storage variant —
/// issue #84): both JSON `null` and an empty object `{}` are accepted
/// as "no args provided" so default-flag callers (CLI, TUI, shell)
/// don't trip serde's missing-field check on arg structs whose fields
/// are all `Option<>`. Required-field structs still fail at the
/// `from_value` step with `default args invalid: missing field …`.
///
/// # Errors
///
/// Returns [`PluginError::ExecutionFailed`] (built via [`exec_err`])
/// when `args` does not satisfy `T`'s shape.
pub fn parse_args<T: DeserializeOwned>(
    plugin_id: &str,
    command: &str,
    value: &serde_json::Value,
) -> Result<T, PluginError> {
    if value.is_null() || matches!(value.as_object(), Some(o) if o.is_empty()) {
        return serde_json::from_value(serde_json::json!({})).map_err(|e| {
            exec_err(
                plugin_id,
                format!("{command}: default args invalid: {e}"),
            )
        });
    }
    serde_json::from_value(value.clone())
        .map_err(|e| exec_err(plugin_id, format!("{command}: invalid args: {e}")))
}

/// Serialize a typed response into `serde_json::Value`.
///
/// # Errors
///
/// Returns [`PluginError::ExecutionFailed`] when `v` cannot be
/// represented as JSON (e.g. a struct containing `f64::NAN`).
pub fn to_value<T: Serialize>(
    plugin_id: &str,
    command: &str,
    v: &T,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v)
        .map_err(|e| exec_err(plugin_id, format!("{command}: serialize failed: {e}")))
}

/// Extract a required string field from an IPC args object.
///
/// Used to subsume the historical per-crate `path_arg` / `relpath_arg`
/// / `name_arg` helpers — callers pass the field name explicitly.
///
/// # Errors
///
/// Returns [`PluginError::ExecutionFailed`] when `field` is missing
/// or not a JSON string.
pub fn string_arg(
    plugin_id: &str,
    command: &str,
    value: &serde_json::Value,
    field: &str,
) -> Result<String, PluginError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            exec_err(
                plugin_id,
                format!("{command}: missing '{field}' string argument"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Args {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        count: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Required {
        name: String,
    }

    const TEST_ID: &str = "com.test.dispatch";

    #[test]
    fn exec_err_attributes_to_plugin_id() {
        let e = exec_err(TEST_ID, "boom".to_string());
        match e {
            PluginError::ExecutionFailed { plugin_id, reason } => {
                assert_eq!(plugin_id, TEST_ID);
                assert_eq!(reason, "boom");
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn parse_args_accepts_null_as_empty_object() {
        let v = serde_json::Value::Null;
        let parsed: Args = parse_args(TEST_ID, "cmd", &v).unwrap();
        assert_eq!(parsed, Args { name: None, count: 0 });
    }

    #[test]
    fn parse_args_accepts_empty_object() {
        let v = serde_json::json!({});
        let parsed: Args = parse_args(TEST_ID, "cmd", &v).unwrap();
        assert_eq!(parsed, Args { name: None, count: 0 });
    }

    #[test]
    fn parse_args_decodes_real_payload() {
        let v = serde_json::json!({"name": "hi", "count": 2});
        let parsed: Args = parse_args(TEST_ID, "cmd", &v).unwrap();
        assert_eq!(
            parsed,
            Args {
                name: Some("hi".into()),
                count: 2
            }
        );
    }

    #[test]
    fn parse_args_required_field_fails_clearly() {
        let v = serde_json::Value::Null;
        let err = parse_args::<Required>(TEST_ID, "cmd", &v).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("default args invalid"), "got: {msg}");
    }

    #[test]
    fn to_value_round_trips_struct() {
        #[derive(Serialize)]
        struct R {
            ok: bool,
        }
        let out = to_value(TEST_ID, "cmd", &R { ok: true }).unwrap();
        assert_eq!(out, serde_json::json!({"ok": true}));
    }

    #[test]
    fn string_arg_extracts_named_field() {
        let v = serde_json::json!({"path": "notes/foo.md"});
        let got = string_arg(TEST_ID, "read", &v, "path").unwrap();
        assert_eq!(got, "notes/foo.md");
    }

    #[test]
    fn string_arg_missing_field_includes_field_name() {
        let v = serde_json::json!({"other": 1});
        let err = string_arg(TEST_ID, "read", &v, "path").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("'path'"), "got: {msg}");
    }
}
