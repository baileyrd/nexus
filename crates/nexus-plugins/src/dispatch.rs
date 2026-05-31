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
///
/// Closes D3 of the 2026-05-21 audit. Every handler error in the
/// workspace flows through this chokepoint, so a single `warn!` here
/// guarantees handler-specific context surfaces in logs — previously
/// errors were `?`-propagated to the caller with no log line at all.
/// Per-handler reason strings (`{command}: {detail}`) carry the
/// command name; service crates that want richer context (path, key,
/// etc.) include it in the reason verbatim.
pub fn exec_err(plugin_id: &str, reason: String) -> PluginError {
    tracing::warn!(
        plugin_id = plugin_id,
        %reason,
        "handler returned ExecutionFailed",
    );
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
        return serde_json::from_value(serde_json::json!({}))
            .map_err(|e| exec_err(plugin_id, format!("{command}: default args invalid: {e}")));
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

/// Execute the canonical IPC-handler shape: **decode args → call domain →
/// wrap error → encode reply** in one call.
///
/// This collapses the per-arm boilerplate that previously occupied 5+
/// lines per dispatch handler (`let a: T = parse_args(...)?;`
/// `let r = f(...).map_err(|e| exec_err(format!("name: {e}")))?;`
/// `to_value(&r, "name")`) into a single line:
///
/// ```ignore
/// HANDLER_CSV_EXPORT => typed_call(PLUGIN_ID, "csv_export", args, |a: CsvExportArgs| {
///     // domain logic returning Result<R, E: Display>
/// })
/// ```
///
/// The closure may return any `Result<R, E>` where `E: Display`; the
/// error is reformatted as `"{command}: {e}"` and attributed to
/// `plugin_id`. For infallible bodies, annotate the closure's return
/// as `Result<R, std::convert::Infallible>`.
///
/// See `docs/0.1.2/audits/solid-dry-assessment-2026-05-18.md` SD-02.
///
/// # Errors
///
/// - [`PluginError::ExecutionFailed`] if `args` cannot decode into `A`.
/// - [`PluginError::ExecutionFailed`] (wrapping `e`) if `f` fails.
/// - [`PluginError::ExecutionFailed`] if the reply cannot serialize.
pub fn typed_call<A, R, F, E>(
    plugin_id: &str,
    command: &str,
    args: &serde_json::Value,
    f: F,
) -> Result<serde_json::Value, PluginError>
where
    A: DeserializeOwned,
    R: Serialize,
    E: std::fmt::Display,
    F: FnOnce(A) -> Result<R, E>,
{
    let a: A = parse_args(plugin_id, command, args)?;
    let r = f(a).map_err(|e| exec_err(plugin_id, format!("{command}: {e}")))?;
    to_value(plugin_id, command, &r)
}

/// Infallible companion to [`typed_call`] for handlers whose domain
/// call cannot fail (pure transformations, lookups returning an
/// `Option` already serialized to JSON, etc.). Skips the
/// `Result<_, Infallible>` ceremony at the call site.
///
/// # Errors
///
/// - [`PluginError::ExecutionFailed`] if `args` cannot decode into `A`.
/// - [`PluginError::ExecutionFailed`] if the reply cannot serialize.
pub fn typed_call_pure<A, R, F>(
    plugin_id: &str,
    command: &str,
    args: &serde_json::Value,
    f: F,
) -> Result<serde_json::Value, PluginError>
where
    A: DeserializeOwned,
    R: Serialize,
    F: FnOnce(A) -> R,
{
    let a: A = parse_args(plugin_id, command, args)?;
    let r = f(a);
    to_value(plugin_id, command, &r)
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
        assert_eq!(
            parsed,
            Args {
                name: None,
                count: 0
            }
        );
    }

    #[test]
    fn parse_args_accepts_empty_object() {
        let v = serde_json::json!({});
        let parsed: Args = parse_args(TEST_ID, "cmd", &v).unwrap();
        assert_eq!(
            parsed,
            Args {
                name: None,
                count: 0
            }
        );
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

    #[test]
    fn typed_call_decodes_calls_and_encodes() {
        let v = serde_json::json!({"name": "x", "count": 3});
        let out: serde_json::Value = typed_call(
            TEST_ID,
            "cmd",
            &v,
            |a: Args| -> Result<u32, std::convert::Infallible> { Ok(a.count + 1) },
        )
        .unwrap();
        assert_eq!(out, serde_json::json!(4));
    }

    #[test]
    fn typed_call_wraps_domain_error_with_command_prefix() {
        let v = serde_json::json!({});
        let err = typed_call(TEST_ID, "cmd", &v, |_: Args| Err::<(), _>("boom")).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("cmd: boom"), "got: {msg}");
    }

    #[test]
    fn typed_call_propagates_arg_decode_failure() {
        let v = serde_json::Value::Null;
        let err =
            typed_call::<Required, (), _, std::convert::Infallible>(TEST_ID, "cmd", &v, |_| {
                unreachable!("closure must not run when decode fails")
            })
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("default args invalid"), "got: {msg}");
    }
}
