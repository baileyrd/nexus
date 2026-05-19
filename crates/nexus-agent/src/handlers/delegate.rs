//! DG-37 agent-to-agent delegation (HANDLER_DELEGATE).
//!
//! ## BL-134 Phase 2b
//!
//! `delegate` no longer drives the sub-session inline via
//! `handle_session_run`. It packs the same args into an
//! `AgentTaskKind::Session` envelope, submits to
//! `com.nexus.ai.runtime::submit`, then blocks on
//! `com.nexus.ai.runtime::wait_for` until the run reaches a terminal
//! status. The wire-level reply shape is preserved — callers still
//! get the underlying `session_run` reply JSON — so `delegate.v1`
//! (the ADR 0021 explicit-version alias) continues to satisfy
//! existing callers without a version bump.
//!
//! Routing through the runtime gives delegate three properties it
//! didn't have before:
//!
//! 1. **Sub-task runs on the dedicated AI worker pool**, not the
//!    kernel's tokio runtime — long tool-loops can't head-of-line
//!    block IPC dispatches against storage / editor.
//! 2. **Sub-task is observable** through `list` / `events` /
//!    `pool_stats`; the shell's observability panel sees it for free.
//! 3. **Parent/child linkage** is recorded via `AgentTask.parent` so
//!    a future task-DAG visualiser can render delegate fan-outs.

use std::sync::Arc;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::shared::{exec_err, parse_args};

/// Args for `com.nexus.agent::delegate` (handler id 24).
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// Target archetype short name (one of the ids returned by
    /// `list_archetypes`).
    pub archetype: String,
    /// Natural-language goal for the sub-session.
    pub goal: String,
    /// Optional override for the sub-session's system prompt.
    #[serde(default)]
    pub system: Option<String>,
    /// Auto-approve the sub-session's rounds. Defaults to `true`.
    #[serde(default = "default_delegate_auto_approve")]
    pub auto_approve: bool,
    /// Approval-callback timeout when `auto_approve = false`.
    #[serde(default)]
    pub approval_timeout_secs: Option<u64>,
    /// Prompt for every round when `auto_approve = false`.
    #[serde(default)]
    pub strict_approval: bool,
}

const fn default_delegate_auto_approve() -> bool {
    true
}

/// Per-call IPC timeout for the `submit` round-trip to
/// `com.nexus.ai.runtime`. Generous because the runtime's own dispatch
/// may serialize a non-trivial task graph; the actual execution time
/// is bounded by the runtime's `wait_for` below, not this dial.
const SUBMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Per-call IPC timeout for the `wait_for` round-trip. The runtime
/// itself owns the deadline of the actual run via its own
/// `SESSION_RUN_TIMEOUT` (2 h) and the agent's per-round
/// `approval_timeout_secs`. We pass `timeout_ms: None` to wait
/// indefinitely on the runtime side; this outer IPC timeout is a
/// safety net for the unlikely "runtime IPC channel itself is wedged"
/// case, set well above the runtime's internal ceiling so it's never
/// the binding constraint under normal operation.
const WAIT_FOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3 * 3600);

pub(crate) async fn handle_delegate(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DelegateArgs = parse_args(args, "delegate")?;
    if a.archetype.trim().is_empty() {
        return Err(exec_err("delegate: `archetype` must be non-empty".into()));
    }
    if a.goal.trim().is_empty() {
        return Err(exec_err("delegate: `goal` must be non-empty".into()));
    }

    let session_args = serde_json::json!({
        "goal": a.goal,
        "archetype": a.archetype,
        "system": a.system,
        "auto_approve": a.auto_approve,
        "approval_timeout_secs": a.approval_timeout_secs,
        "strict_approval": a.strict_approval,
    });

    // ── Submit ──────────────────────────────────────────────────
    let submit_args = serde_json::json!({
        "task": { "kind": "session", "args": session_args },
        "priority": "interactive",
    });
    let submit_reply = ctx
        .ipc_call(
            "com.nexus.ai.runtime",
            "submit",
            submit_args,
            SUBMIT_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("delegate: runtime submit: {e}")))?;
    let task_id = submit_reply
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            exec_err(format!(
                "delegate: runtime submit reply missing task_id: {submit_reply}"
            ))
        })?
        .to_string();

    // ── Wait ────────────────────────────────────────────────────
    let wait_args = serde_json::json!({
        "task_id": task_id,
        // Unbounded — the runtime + the agent's per-round
        // `approval_timeout_secs` already enforce ceilings.
        "timeout_ms": serde_json::Value::Null,
    });
    let wait_reply = ctx
        .ipc_call(
            "com.nexus.ai.runtime",
            "wait_for",
            wait_args,
            WAIT_FOR_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("delegate: runtime wait_for ({task_id}): {e}")))?;

    extract_session_outcome(&task_id, &wait_reply)
}

/// Pull the underlying `session_run` reply body out of the runtime's
/// `wait_for` envelope. The envelope shape is `{ run, timed_out }`;
/// the session reply lives in the last `Finished` event of
/// `run.events`. `Failed` events become an `ExecutionFailed` with the
/// error string verbatim so the caller still gets a structured error.
///
/// Factored out of the IPC body so it's unit-testable without spinning
/// up a runtime — the shape is a pure JSON projection.
fn extract_session_outcome(
    task_id: &str,
    wait_reply: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    if wait_reply
        .get("timed_out")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Err(exec_err(format!(
            "delegate: runtime wait_for ({task_id}): exceeded WAIT_FOR_TIMEOUT before terminal state"
        )));
    }
    let events = wait_reply
        .get("run")
        .and_then(|r| r.get("events"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            exec_err(format!(
                "delegate: runtime wait_for ({task_id}) reply missing run.events: {wait_reply}"
            ))
        })?;
    // Scan tail-first — the terminal event is always the last
    // entry the worker pushed, regardless of how many TokenChunk /
    // RoundProposed entries preceded it.
    for event in events.iter().rev() {
        match event.get("kind").and_then(serde_json::Value::as_str) {
            Some("finished") => {
                return Ok(event
                    .get("outcome")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
            Some("failed") => {
                let error = event
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("(no error message)");
                return Err(exec_err(format!(
                    "delegate: runtime sub-task ({task_id}) failed: {error}"
                )));
            }
            Some("cancelled") => {
                let by = event
                    .get("by")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("(unknown)");
                return Err(exec_err(format!(
                    "delegate: runtime sub-task ({task_id}) cancelled by {by}"
                )));
            }
            _ => continue,
        }
    }
    Err(exec_err(format!(
        "delegate: runtime sub-task ({task_id}) reached wait_for boundary without a terminal event"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_reply_with_events(events: serde_json::Value, timed_out: bool) -> serde_json::Value {
        serde_json::json!({
            "run": { "events": events },
            "timed_out": timed_out,
        })
    }

    #[test]
    fn extract_returns_finished_outcome() {
        let reply = wait_reply_with_events(
            serde_json::json!([
                { "kind": "submitted", "task_id": "abc", "kind_label": "session", "priority": "interactive" },
                { "kind": "started", "task_id": "abc", "attempt": 1 },
                { "kind": "finished", "task_id": "abc", "outcome": { "session_id": "s1", "outcome": "Complete" } }
            ]),
            false,
        );
        let outcome = extract_session_outcome("abc", &reply).unwrap();
        assert_eq!(
            outcome,
            serde_json::json!({ "session_id": "s1", "outcome": "Complete" })
        );
    }

    #[test]
    fn extract_returns_error_on_failed_event() {
        let reply = wait_reply_with_events(
            serde_json::json!([
                { "kind": "started", "task_id": "abc", "attempt": 1 },
                { "kind": "failed", "task_id": "abc", "error": "session_run: ai.chat denied", "retriable": false }
            ]),
            false,
        );
        let err = extract_session_outcome("abc", &reply).unwrap_err();
        assert!(format!("{err}").contains("session_run: ai.chat denied"));
        assert!(format!("{err}").contains("abc"));
    }

    #[test]
    fn extract_returns_error_on_timed_out_envelope() {
        let reply = wait_reply_with_events(serde_json::json!([]), true);
        let err = extract_session_outcome("abc", &reply).unwrap_err();
        assert!(format!("{err}").contains("WAIT_FOR_TIMEOUT"));
    }

    #[test]
    fn extract_returns_error_when_no_terminal_event_present() {
        let reply = wait_reply_with_events(
            serde_json::json!([
                { "kind": "submitted", "task_id": "abc", "kind_label": "session", "priority": "interactive" },
                { "kind": "started", "task_id": "abc", "attempt": 1 }
            ]),
            false,
        );
        let err = extract_session_outcome("abc", &reply).unwrap_err();
        assert!(format!("{err}").contains("without a terminal event"));
    }

    #[test]
    fn extract_surfaces_cancelled_event() {
        let reply = wait_reply_with_events(
            serde_json::json!([
                { "kind": "cancelled", "task_id": "abc", "by": "deadline" }
            ]),
            false,
        );
        let err = extract_session_outcome("abc", &reply).unwrap_err();
        assert!(format!("{err}").contains("cancelled by deadline"));
    }

    #[test]
    fn extract_scans_tail_first_when_terminal_buried_after_chunks() {
        // Real runs include many TokenChunk / RoundProposed events
        // before the terminal one. The rev-scan must still surface
        // the last Finished and ignore the noise.
        let mut events: Vec<serde_json::Value> = (0..50)
            .map(|i| {
                serde_json::json!({
                    "kind": "token_chunk",
                    "task_id": "abc",
                    "text": format!("chunk-{i}")
                })
            })
            .collect();
        events.push(serde_json::json!({
            "kind": "finished",
            "task_id": "abc",
            "outcome": { "ok": true }
        }));
        let reply = wait_reply_with_events(serde_json::json!(events), false);
        let outcome = extract_session_outcome("abc", &reply).unwrap();
        assert_eq!(outcome, serde_json::json!({ "ok": true }));
    }
}
