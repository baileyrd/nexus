//! `com.nexus.workflow::validate` handler — parse a `.workflow.toml`
//! body and (when a kernel context is wired) cross-check `terminal`
//! step slugs against the live `com.nexus.terminal::saved_list`.
//!
//! Lifted out of `core_plugin.rs` by the BL-137 oversized-file
//! decomposition. The sync entry point is the simple parse-only path;
//! the async entry point adds the IPC slug check.

use std::sync::Arc;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;

use crate::core_plugin::ValidateWorkflowArgs;
use crate::parse_workflow_text;

use super::shared::{exec_err, parse_args, to_value, DEFAULT_STEP_TIMEOUT};

pub(crate) fn handle_sync(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: ValidateWorkflowArgs = parse_args(args, "validate")?;
    match parse_workflow_text(&a.text) {
        Ok(w) => to_value(&w, "validate"),
        Err(err) => Err(exec_err(format!("invalid workflow: {err}"))),
    }
}

/// BL-056 — async validate. Runs the same TOML parse the sync path
/// does, and additionally — when one or more `type = "terminal"`
/// steps are present and the plugin has a kernel context — checks
/// every step's `slug` against `com.nexus.terminal::saved_list`. An
/// unknown slug fails validation with a clear error so a workflow
/// author can't ship a workflow that's syntactically valid but
/// references a saved command that doesn't exist.
///
/// When `ctx` is `None` (test runtime, plugin booted without a forge),
/// the slug check is skipped and the call falls back to the parse-only
/// result. The caller may still invoke the sync path for backwards
/// compatibility.
pub(crate) async fn handle_async(
    ctx: Option<&Arc<KernelPluginContext>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: ValidateWorkflowArgs = parse_args(args, "validate")?;
    let workflow =
        parse_workflow_text(&a.text).map_err(|err| exec_err(format!("invalid workflow: {err}")))?;

    // Collect all slugs referenced by terminal steps. The vast
    // majority of validate calls have none, in which case we skip the
    // IPC entirely.
    let terminal_slugs: Vec<String> = workflow
        .steps
        .iter()
        .filter(|s| s.step_type == "terminal")
        .filter_map(|s| {
            s.extra
                .get("slug")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect();
    if terminal_slugs.is_empty() {
        return to_value(&workflow, "validate");
    }
    let Some(ctx) = ctx else {
        // Without a kernel context we can't reach the saved store;
        // fall back to the parse-only result so test runtimes don't
        // fail just because the IPC plumbing isn't wired.
        return to_value(&workflow, "validate");
    };
    let saved = ctx
        .ipc_call(
            "com.nexus.terminal",
            "saved_list",
            serde_json::json!({}),
            DEFAULT_STEP_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("validate: terminal saved_list failed: {e}")))?;
    let known: std::collections::HashSet<String> = saved
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("slug").and_then(|s| s.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for slug in &terminal_slugs {
        if !known.contains(slug) {
            return Err(exec_err(format!(
                "invalid workflow: terminal step references unknown saved-command slug '{slug}'"
            )));
        }
    }
    to_value(&workflow, "validate")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a kernel context, `validate_async` falls back to the
    /// parse-only result — even when the workflow has terminal steps —
    /// so the test runtime that doesn't wire IPC plumbing still gets a
    /// usable validate path.
    #[tokio::test]
    async fn validate_async_with_terminal_step_falls_back_to_parse_when_ctx_absent() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "terminal"
slug = "dev-server"
"#;
        let result = handle_async(None, &serde_json::json!({ "text": src }))
            .await
            .expect("validate ok");
        assert_eq!(result["workflow"]["name"], "T");
        assert_eq!(result["steps"][0]["type"], "terminal");
    }

    /// Workflows with no terminal steps are validated through the
    /// parse-only path even when a context is present — the IPC
    /// shouldn't fire when there's nothing to check.
    #[tokio::test]
    async fn validate_async_without_terminal_steps_skips_ipc() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "manual"

[[steps]]
type = "noop"
"#;
        let result = handle_async(None, &serde_json::json!({ "text": src }))
            .await
            .expect("validate ok");
        assert_eq!(result["workflow"]["name"], "T");
    }

    #[tokio::test]
    async fn validate_async_propagates_parse_errors() {
        let err = handle_async(None, &serde_json::json!({ "text": "not toml {{" }))
            .await
            .expect_err("must fail");
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("invalid workflow"), "got: {reason}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
