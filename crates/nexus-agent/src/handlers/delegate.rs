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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::subagent::{SubagentRunner, SubagentSpec};

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
    /// Workspace isolation for the sub-session (RFC 0007). Defaults to `none`
    /// (the shared parent forge). `worktree` runs the sub-session as an
    /// isolated headless child process on a git-worktree checkout and merges
    /// its branch back into the parent.
    #[serde(default)]
    pub isolation: Isolation,
}

/// Workspace isolation model for a delegated sub-session (RFC 0007).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    /// Run the sub-session in the shared parent forge via the ai-runtime, as
    /// before. The default.
    #[default]
    None,
    /// Run the sub-session as a headless child `nexus` process pointed at a
    /// git-worktree checkout, then merge the resulting branch back into the
    /// parent forge (RFC 0007 Option A).
    Worktree,
}

const fn default_delegate_auto_approve() -> bool {
    true
}

/// Per-call IPC timeout for the git worktree / merge round-trips in the
/// isolated delegate path. These are local libgit2 operations — generous but
/// finite, well under the subagent run's own ceiling.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

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

    // `isolation` is `Copy`, so matching on it doesn't borrow `a` — the chosen
    // branch takes ownership.
    match a.isolation {
        Isolation::None => delegate_shared(ctx, a).await,
        Isolation::Worktree => delegate_isolated(ctx, a).await,
    }
}

/// Shared-forge delegation (the `isolation = "none"` default, BL-134 Phase 2b):
/// pack the sub-session into an `AgentTaskKind::Session`, submit it to
/// `com.nexus.ai.runtime`, and block on `wait_for` until it reaches a terminal
/// state. Behaviour is unchanged from before RFC 0007.
async fn delegate_shared(
    ctx: Arc<KernelPluginContext>,
    a: DelegateArgs,
) -> Result<serde_json::Value, PluginError> {
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

/// Outcome of merging an isolated subagent's branch back into the parent.
enum MergeStatus {
    /// The subagent made no changes — there was nothing to merge.
    NoDelta,
    /// The branch merged cleanly; carries the merge commit hash (or `Null`).
    Merged(Value),
    /// The merge hit conflicts; carries the conflicted paths. The parent's
    /// working tree was restored (merge aborted) and the branch was kept.
    Conflicts(Vec<Value>),
}

/// RFC 0007 Option A — run the sub-session as a headless child `nexus` process
/// on a git-worktree checkout, then merge its branch back into the parent forge.
///
/// Failure semantics: an infra failure (no git, worktree create, spawn) is a
/// hard error; a subagent that exits non-zero or times out keeps its worktree +
/// branch for inspection and errors; a clean run merges the branch (or, on
/// conflict, aborts the merge and returns the branch for manual resolution).
async fn delegate_isolated(
    ctx: Arc<KernelPluginContext>,
    a: DelegateArgs,
) -> Result<Value, PluginError> {
    // Isolation needs a git-backed forge. `status` returns JSON null when the
    // git plugin is passive (the forge root isn't a repo).
    let git_status = ctx
        .ipc_call("com.nexus.git", "status", serde_json::json!({}), GIT_TIMEOUT)
        .await
        .map_err(|e| exec_err(format!("delegate: git status probe: {e}")))?;
    if !forge_is_git(&git_status) {
        return Err(exec_err(
            "delegate: isolation=\"worktree\" requires a git-backed forge; \
             run `git init` in the forge or use isolation=\"none\""
                .into(),
        ));
    }

    let short = short_id(&uuid::Uuid::new_v4());
    let name = format!("subagent-{short}");
    let branch = format!("nexus/subagent/{short}");

    // Create the worktree (its branch is created at the parent's HEAD).
    let wt = ctx
        .ipc_call(
            "com.nexus.git",
            "worktree_create",
            serde_json::json!({ "name": name, "branch": branch }),
            GIT_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("delegate: worktree_create: {e}")))?;
    let wt_path = wt
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| exec_err(format!("delegate: worktree_create reply missing path: {wt}")))?
        .to_string();

    // Run the subagent headlessly against the worktree forge.
    let runner = SubagentRunner::resolve(None)
        .map_err(|e| exec_err(format!("delegate: locate nexus binary: {e}")))?;
    let mut spec = SubagentSpec::new(PathBuf::from(&wt_path), a.goal.clone());
    spec.archetype = Some(a.archetype.clone());
    let outcome = runner
        .run(&spec)
        .await
        .map_err(|e| exec_err(format!("delegate: spawn isolated subagent: {e}")))?;

    if !outcome.succeeded() {
        // Keep the worktree + branch for inspection (RFC 0007 failure
        // semantics): no merge, no cleanup.
        return Err(exec_err(format!(
            "delegate: isolated subagent did not complete (exit_code={:?}, timed_out={}); \
             worktree '{name}' and branch '{branch}' kept for inspection{tail}",
            outcome.exit_code,
            outcome.timed_out,
            tail = stderr_suffix(&outcome.stderr),
        )));
    }

    let subagent_outcome = outcome
        .transcript
        .as_ref()
        .and_then(|t| t.get("outcome"))
        .cloned()
        .unwrap_or(Value::Null);

    // Commit the worktree's edits to the task branch.
    let commit_reply = ctx
        .ipc_call(
            "com.nexus.git",
            "worktree_commit",
            serde_json::json!({ "name": name, "message": format!("subagent: {}", a.goal) }),
            GIT_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("delegate: worktree_commit: {e}")))?;
    let committed = commit_reply
        .get("commit_hash")
        .and_then(Value::as_str)
        .map(str::to_string);

    let merge_status = if committed.is_none() {
        MergeStatus::NoDelta
    } else {
        // Merge the branch into the parent's current HEAD.
        let merge_reply = ctx
            .ipc_call(
                "com.nexus.git",
                "merge",
                serde_json::json!({ "branch": branch }),
                GIT_TIMEOUT,
            )
            .await
            .map_err(|e| exec_err(format!("delegate: merge {branch}: {e}")))?;
        let conflicts = merge_reply
            .get("conflicts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if conflicts.is_empty() {
            MergeStatus::Merged(merge_reply.get("commit_hash").cloned().unwrap_or(Value::Null))
        } else {
            // Restore the parent's working tree; keep the branch for manual
            // resolution. Best-effort — a failed abort still leaves the branch.
            let _ = ctx
                .ipc_call(
                    "com.nexus.git",
                    "abort_merge",
                    serde_json::json!({}),
                    GIT_TIMEOUT,
                )
                .await;
            MergeStatus::Conflicts(conflicts)
        }
    };

    // The branch is always preserved; the worktree is disposable now that its
    // work is committed. Cleanup failure is non-fatal.
    if let Err(e) = remove_worktree(&ctx, &name).await {
        tracing::warn!(worktree = %name, error = %e, "delegate: worktree cleanup failed");
    }

    Ok(build_isolated_result(
        &branch,
        &merge_status,
        subagent_outcome,
        outcome.exit_code,
        outcome.timed_out,
    ))
}

/// Remove an isolated subagent's worktree (force-pruning its working tree). The
/// branch is left intact.
async fn remove_worktree(ctx: &KernelPluginContext, name: &str) -> Result<(), PluginError> {
    ctx.ipc_call(
        "com.nexus.git",
        "worktree_remove",
        serde_json::json!({ "name": name, "force": true }),
        GIT_TIMEOUT,
    )
    .await
    .map(|_| ())
    .map_err(|e| exec_err(format!("delegate: worktree_remove {name}: {e}")))
}

/// `com.nexus.git::status` returns JSON null when the forge is not a git repo
/// (the plugin's passive mode); any non-null reply means it's git-backed.
fn forge_is_git(status: &Value) -> bool {
    !status.is_null()
}

/// First 8 hex chars of a UUID — enough to disambiguate concurrent subagents
/// while keeping worktree / branch names short and readable.
fn short_id(id: &uuid::Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

/// A trailing `; stderr: …` fragment (last line, truncated to 200 chars) for a
/// failed subagent's error message, or empty when stderr is blank.
fn stderr_suffix(stderr: &str) -> String {
    let last = stderr.trim().lines().last().unwrap_or("").trim();
    if last.is_empty() {
        return String::new();
    }
    let snippet: String = last.chars().take(200).collect();
    format!("; stderr: {snippet}")
}

/// Shape the JSON reply for an isolated delegation from its pieces. Pure, so the
/// reply contract is unit-testable without a runtime.
fn build_isolated_result(
    branch: &str,
    status: &MergeStatus,
    subagent_outcome: Value,
    exit_code: Option<i32>,
    timed_out: bool,
) -> Value {
    let mut obj = serde_json::json!({
        "isolation": "worktree",
        "branch": branch,
        "outcome": subagent_outcome,
        "subagent": { "exit_code": exit_code, "timed_out": timed_out },
    });
    let map = obj.as_object_mut().expect("json object literal");
    match status {
        MergeStatus::NoDelta => {
            map.insert("delta".into(), Value::Bool(false));
            map.insert("merged".into(), Value::Bool(false));
        }
        MergeStatus::Merged(commit) => {
            map.insert("delta".into(), Value::Bool(true));
            map.insert("merged".into(), Value::Bool(true));
            map.insert("commit".into(), commit.clone());
        }
        MergeStatus::Conflicts(conflicts) => {
            map.insert("delta".into(), Value::Bool(true));
            map.insert("merged".into(), Value::Bool(false));
            map.insert("conflicts".into(), Value::Array(conflicts.clone()));
        }
    }
    obj
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

    // ── RFC 0007 — isolation arg + isolated-result shaping ───────

    #[test]
    fn delegate_args_default_isolation_is_none() {
        let a: DelegateArgs =
            serde_json::from_value(serde_json::json!({ "archetype": "coder", "goal": "x" }))
                .unwrap();
        assert_eq!(a.isolation, Isolation::None);
    }

    #[test]
    fn delegate_args_parses_worktree_isolation() {
        let a: DelegateArgs = serde_json::from_value(serde_json::json!({
            "archetype": "coder",
            "goal": "x",
            "isolation": "worktree",
        }))
        .unwrap();
        assert_eq!(a.isolation, Isolation::Worktree);
    }

    #[test]
    fn forge_is_git_detects_passive_null() {
        assert!(!forge_is_git(&Value::Null));
        assert!(forge_is_git(&serde_json::json!({ "branch": "main" })));
    }

    #[test]
    fn short_id_is_8_hex_chars() {
        assert_eq!(short_id(&uuid::Uuid::nil()), "00000000");
        let r = short_id(&uuid::Uuid::new_v4());
        assert_eq!(r.len(), 8);
        assert!(r.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn stderr_suffix_blank_for_empty() {
        assert_eq!(stderr_suffix(""), "");
        assert_eq!(stderr_suffix("  \n  "), "");
    }

    #[test]
    fn stderr_suffix_takes_last_line() {
        assert_eq!(
            stderr_suffix("line one\nfinal error here\n"),
            "; stderr: final error here"
        );
    }

    #[test]
    fn isolated_result_merged_shape() {
        let v = build_isolated_result(
            "nexus/subagent/abc",
            &MergeStatus::Merged(serde_json::json!("deadbee")),
            serde_json::json!("complete"),
            Some(0),
            false,
        );
        assert_eq!(v["isolation"], "worktree");
        assert_eq!(v["branch"], "nexus/subagent/abc");
        assert_eq!(v["merged"], true);
        assert_eq!(v["delta"], true);
        assert_eq!(v["commit"], "deadbee");
        assert_eq!(v["outcome"], "complete");
        assert_eq!(v["subagent"]["exit_code"], 0);
        assert_eq!(v["subagent"]["timed_out"], false);
    }

    #[test]
    fn isolated_result_conflicts_shape() {
        let v = build_isolated_result(
            "nexus/subagent/abc",
            &MergeStatus::Conflicts(vec![serde_json::json!("a.md")]),
            Value::Null,
            None,
            false,
        );
        assert_eq!(v["merged"], false);
        assert_eq!(v["delta"], true);
        assert_eq!(v["conflicts"][0], "a.md");
        assert!(v.get("commit").is_none(), "no commit on a conflicted merge");
    }

    #[test]
    fn isolated_result_no_delta_shape() {
        let v = build_isolated_result("b", &MergeStatus::NoDelta, Value::Null, Some(0), false);
        assert_eq!(v["merged"], false);
        assert_eq!(v["delta"], false);
        assert!(v.get("commit").is_none());
        assert!(v.get("conflicts").is_none());
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
