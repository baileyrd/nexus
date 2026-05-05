//! Multi-round agent session loop (ADR 0024 Phase 2a).
//!
//! Replaces the agent's plan-then-execute split with a tool-loop
//! that runs the model + dispatch + approval policy in lockstep.
//! One [`run_session`] call drives N rounds: each round asks the
//! [`ChatDriver`] for the model's next move, presents the proposed
//! tool calls to a [`SessionPolicy`], and dispatches the approved
//! subset before looping again.
//!
//! See ADR 0024 for the full design rationale.
//!
//! ## Library-only in Phase 2a
//!
//! Phase 2a ships the library API. The agent's IPC `session_run`
//! handler that this module backs accepts `auto_approve: true`
//! only — interactive approval (bus-event + `round_decide`
//! callback) is Phase 2b.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{ChatDriver, ProposedToolCall, ToolCall, ToolDispatcher};

/// Hard cap on round count, mirroring the AI plugin's
/// `MAX_TOOL_ROUNDS`. A session that hits the cap exits with
/// [`SessionOutcome::MaxRounds`] — the caller may resume with a
/// follow-up `run_session` if the goal isn't done.
pub const MAX_AGENT_ROUNDS: u32 = 8;

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum SessionOutcome {
    /// Model emitted no tool calls and no text on its final turn,
    /// or it emitted final-text without further tool requests.
    Complete,
    /// A round's policy returned [`RoundDecision::Abort`] or every
    /// tool call in a round was denied.
    Aborted,
    /// A dispatched tool call returned an error AND the loop chose
    /// to stop rather than feed the error back. (Phase 2a never
    /// chooses to stop on tool error — error results are fed back
    /// to the model. Reserved for future error policies.)
    Errored,
    /// The session ran to [`MAX_AGENT_ROUNDS`] without completing.
    MaxRounds,
    /// A round's policy didn't return a decision within the
    /// configured timeout. Phase 2b — emitted by `BusBridgePolicy`
    /// when no `round_decide` IPC arrives before the deadline.
    ApprovalTimeout,
}

/// One model turn the policy is asked to approve.
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
pub struct ProposedRound {
    /// 1-based round index.
    pub round: u32,
    /// Narration text the model emitted alongside the tool calls
    /// in this round (empty if it only emitted tool calls).
    pub text: String,
    /// Tool calls proposed in this round, ready for dispatch.
    pub tool_calls: Vec<ProposedToolCall>,
}

/// Per-tool decision inside a [`RoundDecision::Partial`] round.
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
pub struct RoundDecisionEntry {
    /// Provider-issued id from the corresponding
    /// [`ProposedToolCall`].
    pub tool_use_id: String,
    /// `true` to dispatch; `false` to feed an error back to the
    /// model in place of the result.
    pub approve: bool,
    /// Reason surfaced to the model on denial. Empty for approvals.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// What a [`SessionPolicy`] returns from
/// [`SessionPolicy::allow_round`].
#[derive(Debug, Clone)]
pub enum RoundDecision {
    /// Run every tool call in the round.
    ApproveAll,
    /// Mixed approve/deny — denied calls feed back as
    /// `is_error: true` tool-result turns so the model can recover
    /// next round.
    Partial(Vec<RoundDecisionEntry>),
    /// Stop the session. No more dispatches; loop exits with
    /// [`SessionOutcome::Aborted`].
    Abort(String),
    /// Stop the session because the policy didn't decide in time.
    /// Loop exits with [`SessionOutcome::ApprovalTimeout`].
    /// Phase 2b — used by `BusBridgePolicy` when the
    /// `round_decide` IPC doesn't arrive before its deadline.
    Timeout(String),
}

/// Approval policy consulted once per round. Strictly more
/// expressive than [`crate::StepPolicy`]: the existing
/// [`crate::AutoApprove`] maps to [`AutoApproveAll`] here.
#[async_trait]
pub trait SessionPolicy: Send + Sync {
    /// Decide what to do with a proposed round. Async because the
    /// production implementation will go to disk / IPC / a UI
    /// prompt.
    async fn allow_round(&self, round: &ProposedRound) -> RoundDecision;
}

/// Trivial policy that approves every round. Suitable for
/// scripted / headless sessions and the `auto_approve: true` IPC
/// path.
pub struct AutoApproveAll;

#[async_trait]
impl SessionPolicy for AutoApproveAll {
    async fn allow_round(&self, _round: &ProposedRound) -> RoundDecision {
        RoundDecision::ApproveAll
    }
}

/// Outcome of one tool call within a recorded round. Mirrors the
/// transcript shape ADR 0024 documents.
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
pub struct ToolCallRecord {
    /// Provider-issued id from the model.
    pub id: String,
    /// Tool name as advertised in the registry.
    pub name: String,
    /// Resolved IPC dispatch target (post-`dispatch_target`).
    pub tool_call: ToolCall,
    /// Whether the policy approved this call.
    pub approved: bool,
    /// Approval/denial reason. Empty for unconditional approvals.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// JSON response from the dispatcher when the call ran.
    /// `None` for denied calls and dispatch failures (which surface
    /// in `error` instead).
    #[cfg_attr(feature = "ts-export", ts(type = "unknown | null"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
    /// Stringified error from a failed dispatch (or the denial
    /// reason fed back to the model). Empty when the call ran
    /// successfully.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// One round in a recorded session.
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
pub struct RoundRecord {
    /// 1-based round index.
    pub round: u32,
    /// Narration text the model emitted this round.
    pub text: String,
    /// Per-tool-call records. Empty for the final text-only round.
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Full session transcript persisted to
/// `<forge>/.forge/agent/sessions/<id>.json`.
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
pub struct AgentSession {
    /// UUID v4 — assigned at session creation.
    pub id: String,
    /// Original natural-language goal.
    pub goal: String,
    /// Optional archetype id (`com.nexus.agent.writer`, etc.). When
    /// `None`, the default planner system prompt was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archetype: Option<String>,
    /// RFC 3339 UTC timestamp of session start.
    pub started_at: String,
    /// RFC 3339 UTC timestamp of session end (success or failure).
    pub ended_at: String,
    /// Per-round records, in execution order.
    pub rounds: Vec<RoundRecord>,
    /// Why the session ended.
    pub outcome: SessionOutcome,
}

/// Run a session against `driver` (the LLM) and `dispatcher` (the
/// tool transport), driving rounds until the model is done, the
/// policy aborts, every tool in a round is denied, or
/// [`MAX_AGENT_ROUNDS`] is hit.
///
/// `system` is the planner system prompt — passed through to the
/// driver verbatim. The default and archetype prompts in
/// [`crate::archetypes`] are the standard sources.
///
/// # Errors
/// Returns the partial transcript via the [`AgentSession`] return
/// value rather than a `Result` — every recoverable issue is
/// recorded in the round it happened in. The function only
/// short-circuits if `goal` is empty.
pub async fn run_session<D, P, T>(
    driver: &D,
    dispatcher: &T,
    policy: &P,
    goal: &str,
    system: &str,
    archetype: Option<String>,
) -> AgentSession
where
    D: ChatDriver + ?Sized,
    P: SessionPolicy + ?Sized,
    T: ToolDispatcher + ?Sized,
{
    let id = uuid::Uuid::new_v4().to_string();
    run_session_with_id(driver, dispatcher, policy, goal, system, archetype, id).await
}

/// Like [`run_session`] but accepts a caller-supplied session id.
/// Used when the policy needs the id before the loop starts —
/// e.g. `BusBridgePolicy` (Phase 2b) embeds the id in the
/// `round_proposed` event payload so the caller can correlate
/// approvals back to the right session.
pub async fn run_session_with_id<D, P, T>(
    driver: &D,
    dispatcher: &T,
    policy: &P,
    goal: &str,
    system: &str,
    archetype: Option<String>,
    id: String,
) -> AgentSession
where
    D: ChatDriver + ?Sized,
    P: SessionPolicy + ?Sized,
    T: ToolDispatcher + ?Sized,
{
    let started_at = chrono::Utc::now().to_rfc3339();
    let mut session = AgentSession {
        id: id.clone(),
        goal: goal.to_string(),
        archetype,
        started_at,
        ended_at: String::new(),
        rounds: Vec::new(),
        outcome: SessionOutcome::Complete,
    };

    if goal.trim().is_empty() {
        session.ended_at = chrono::Utc::now().to_rfc3339();
        session.outcome = SessionOutcome::Aborted;
        return session;
    }

    // Conversation transcript fed back to the driver each round.
    // Phase 2a uses a simple "current-user-prompt" formulation:
    // each round, we restate the goal and append the prior
    // round's results as bullet points. A future Phase 2c can
    // upgrade this to provider-native ChatTurn linkage once the
    // driver surface supports multi-turn directly.
    let mut current_prompt = goal.to_string();

    for round_idx in 1..=MAX_AGENT_ROUNDS {
        // Ask the model for this round's tool calls.
        let proposal = match driver.propose(system, &current_prompt).await {
            Ok(p) => p,
            Err(e) => {
                session.rounds.push(RoundRecord {
                    round: round_idx,
                    text: format!("driver error: {e}"),
                    tool_calls: Vec::new(),
                });
                session.outcome = SessionOutcome::Errored;
                break;
            }
        };

        // Terminal text-only round — the model is done.
        if proposal.tool_calls.is_empty() {
            session.rounds.push(RoundRecord {
                round: round_idx,
                text: proposal.text,
                tool_calls: Vec::new(),
            });
            session.outcome = SessionOutcome::Complete;
            break;
        }

        // Approval gate.
        let proposed = ProposedRound {
            round: round_idx,
            text: proposal.text.clone(),
            tool_calls: proposal.tool_calls.clone(),
        };
        let decision = policy.allow_round(&proposed).await;

        let (records, stop_reason) =
            execute_round(dispatcher, round_idx, &proposal.text, proposal.tool_calls, decision)
                .await;

        let any_approved = records.iter().any(|r| r.approved);
        let all_errored = !records.is_empty() && records.iter().all(|r| !r.error.is_empty());
        session.rounds.push(RoundRecord {
            round: round_idx,
            text: proposal.text,
            tool_calls: records,
        });

        if let Some(stop) = stop_reason {
            let (outcome, label, reason) = match stop {
                RoundStopReason::Aborted(r) => {
                    (SessionOutcome::Aborted, "session aborted", r)
                }
                RoundStopReason::Timeout(r) => {
                    (SessionOutcome::ApprovalTimeout, "approval timeout", r)
                }
            };
            session.outcome = outcome;
            // Record the stop reason as a synthetic narration round so
            // the transcript shows why the loop stopped.
            session.rounds.push(RoundRecord {
                round: round_idx + 1,
                text: format!("{label}: {reason}"),
                tool_calls: Vec::new(),
            });
            break;
        }

        if !any_approved {
            // Every call denied → no point feeding empty results
            // back. Treat as an abort so the transcript is honest.
            session.outcome = SessionOutcome::Aborted;
            break;
        }

        // Build the next round's prompt from the approved results.
        // Stay deliberately minimal — the goal is to give the model
        // enough context to pick a sensible next step without
        // bloating the prompt.
        current_prompt = compose_followup_prompt(goal, &session.rounds, all_errored);

        if round_idx == MAX_AGENT_ROUNDS {
            session.outcome = SessionOutcome::MaxRounds;
        }
    }

    session.ended_at = chrono::Utc::now().to_rfc3339();
    session
}

/// Marker indicating why a round caused the session to stop.
/// Returned alongside per-call records by [`execute_round`].
enum RoundStopReason {
    Aborted(String),
    Timeout(String),
}

/// Apply a [`RoundDecision`] to a list of proposed tool calls,
/// returning the per-call records produced this round and an
/// optional reason for the session to stop.
async fn execute_round<T: ToolDispatcher + ?Sized>(
    dispatcher: &T,
    _round_idx: u32,
    _text: &str,
    proposed: Vec<ProposedToolCall>,
    decision: RoundDecision,
) -> (Vec<ToolCallRecord>, Option<RoundStopReason>) {
    match decision {
        RoundDecision::Abort(reason) => (
            proposed
                .into_iter()
                .map(|p| ToolCallRecord {
                    id: p.id,
                    name: p.name,
                    tool_call: p.tool_call,
                    approved: false,
                    reason: reason.clone(),
                    response: None,
                    error: format!("session aborted: {reason}"),
                })
                .collect(),
            Some(RoundStopReason::Aborted(reason)),
        ),
        RoundDecision::Timeout(reason) => (
            proposed
                .into_iter()
                .map(|p| ToolCallRecord {
                    id: p.id,
                    name: p.name,
                    tool_call: p.tool_call,
                    approved: false,
                    reason: reason.clone(),
                    response: None,
                    error: format!("approval timeout: {reason}"),
                })
                .collect(),
            Some(RoundStopReason::Timeout(reason)),
        ),
        RoundDecision::ApproveAll => {
            let mut out = Vec::with_capacity(proposed.len());
            for p in proposed {
                let record = dispatch_one(dispatcher, p, true, String::new()).await;
                out.push(record);
            }
            (out, None)
        }
        RoundDecision::Partial(entries) => {
            let mut out = Vec::with_capacity(proposed.len());
            for p in proposed {
                // Look up the matching decision; missing entries
                // default to "deny" — the caller forgot to mention
                // this id, so safer to reject than to guess.
                let entry = entries.iter().find(|e| e.tool_use_id == p.id);
                let (approve, reason) = entry
                    .map(|e| (e.approve, e.reason.clone()))
                    .unwrap_or((false, "no decision provided".to_string()));
                let record = dispatch_one(dispatcher, p, approve, reason).await;
                out.push(record);
            }
            (out, None)
        }
    }
}

async fn dispatch_one<T: ToolDispatcher + ?Sized>(
    dispatcher: &T,
    proposed: ProposedToolCall,
    approved: bool,
    reason: String,
) -> ToolCallRecord {
    if !approved {
        return ToolCallRecord {
            id: proposed.id,
            name: proposed.name,
            tool_call: proposed.tool_call,
            approved: false,
            reason: reason.clone(),
            response: None,
            error: if reason.is_empty() {
                "denied by policy".to_string()
            } else {
                reason
            },
        };
    }
    match dispatcher.dispatch(&proposed.tool_call).await {
        Ok(value) => ToolCallRecord {
            id: proposed.id,
            name: proposed.name,
            tool_call: proposed.tool_call,
            approved: true,
            reason,
            response: Some(value),
            error: String::new(),
        },
        Err(e) => ToolCallRecord {
            id: proposed.id,
            name: proposed.name,
            tool_call: proposed.tool_call,
            approved: true,
            reason,
            response: None,
            error: e,
        },
    }
}

/// Compose the prompt for the next round. Phase 2a uses a flat
/// re-statement with a compact summary of prior rounds; a future
/// upgrade can switch to provider-native multi-turn chat.
fn compose_followup_prompt(goal: &str, rounds: &[RoundRecord], all_errored: bool) -> String {
    let mut out = String::new();
    out.push_str("Original goal: ");
    out.push_str(goal);
    out.push_str("\n\nResults so far:\n");
    for r in rounds {
        if r.tool_calls.is_empty() {
            continue;
        }
        for tc in &r.tool_calls {
            if tc.approved && tc.error.is_empty() {
                out.push_str(&format!(
                    "- round {}: {} ok\n",
                    r.round, tc.name
                ));
            } else if !tc.error.is_empty() {
                out.push_str(&format!(
                    "- round {}: {} failed: {}\n",
                    r.round, tc.name, tc.error
                ));
            }
        }
    }
    if all_errored {
        out.push_str(
            "\nThe last round's tool calls all failed. Consider trying \
             a different approach or stopping if the goal is unreachable.",
        );
    } else {
        out.push_str(
            "\nDecide the next tool call(s), or respond with text and no \
             tool calls if the goal is complete.",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Proposal;
    use std::sync::Mutex;

    /// Driver that returns a different proposal per call, in order.
    /// Panics if `propose` is called more times than the queued list.
    struct ScriptedDriver {
        replies: Mutex<std::collections::VecDeque<Proposal>>,
    }
    impl ScriptedDriver {
        fn new(replies: Vec<Proposal>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
            }
        }
    }
    #[async_trait]
    impl ChatDriver for ScriptedDriver {
        async fn propose(&self, _system: &str, _user: &str) -> Result<Proposal, String> {
            let mut q = self.replies.lock().unwrap();
            Ok(q.pop_front().expect("scripted driver exhausted"))
        }
    }

    /// Counting dispatcher that records calls and returns canned values.
    struct CountingDispatcher {
        calls: Mutex<Vec<ToolCall>>,
    }
    impl CountingDispatcher {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }
    #[async_trait]
    impl ToolDispatcher for CountingDispatcher {
        async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String> {
            self.calls.lock().unwrap().push(call.clone());
            Ok(serde_json::json!({"ok": true}))
        }
    }

    fn read_tool(id: &str, path: &str) -> ProposedToolCall {
        ProposedToolCall {
            id: id.into(),
            name: "read_file".into(),
            tool_call: ToolCall {
                target_plugin_id: "com.nexus.storage".into(),
                command_id: "read_file".into(),
                args: serde_json::json!({ "path": path }),
            },
        }
    }

    #[tokio::test]
    async fn auto_approves_runs_until_text_only_terminator() {
        let driver = ScriptedDriver::new(vec![
            Proposal {
                text: "fetching".into(),
                tool_calls: vec![read_tool("u1", "a.md")],
            },
            Proposal {
                text: "summary: hello".into(),
                tool_calls: Vec::new(),
            },
        ]);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "summarise notes",
            "you are a planner",
            None,
        )
        .await;

        assert_eq!(session.outcome, SessionOutcome::Complete);
        assert_eq!(session.rounds.len(), 2);
        assert_eq!(session.rounds[0].tool_calls.len(), 1);
        assert!(session.rounds[0].tool_calls[0].approved);
        assert!(session.rounds[0].tool_calls[0].error.is_empty());
        assert_eq!(session.rounds[1].text, "summary: hello");
        assert!(session.rounds[1].tool_calls.is_empty());
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn empty_goal_short_circuits_to_abort() {
        let driver = ScriptedDriver::new(Vec::new());
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "   \t  ",
            "system",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::Aborted);
        assert!(session.rounds.is_empty());
    }

    #[tokio::test]
    async fn abort_decision_stops_loop_and_records_reason() {
        struct AbortPolicy;
        #[async_trait]
        impl SessionPolicy for AbortPolicy {
            async fn allow_round(&self, _round: &ProposedRound) -> RoundDecision {
                RoundDecision::Abort("user said no".into())
            }
        }
        let driver = ScriptedDriver::new(vec![Proposal {
            text: String::new(),
            tool_calls: vec![read_tool("u1", "a.md")],
        }]);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &AbortPolicy,
            "do thing",
            "system",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::Aborted);
        // Two rounds: the proposed one (denied) + the synthetic
        // narration that records the abort reason.
        assert_eq!(session.rounds.len(), 2);
        assert!(session.rounds[1].text.contains("user said no"));
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn partial_decision_dispatches_subset_and_denies_rest() {
        struct PartialPolicy;
        #[async_trait]
        impl SessionPolicy for PartialPolicy {
            async fn allow_round(&self, round: &ProposedRound) -> RoundDecision {
                let entries = round
                    .tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| RoundDecisionEntry {
                        tool_use_id: tc.id.clone(),
                        approve: i == 0,
                        reason: if i == 0 { String::new() } else { "skip the second".into() },
                    })
                    .collect();
                RoundDecision::Partial(entries)
            }
        }
        let driver = ScriptedDriver::new(vec![
            Proposal {
                text: String::new(),
                tool_calls: vec![read_tool("u1", "a.md"), read_tool("u2", "b.md")],
            },
            Proposal {
                text: "done".into(),
                tool_calls: Vec::new(),
            },
        ]);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &PartialPolicy,
            "compare notes",
            "system",
            None,
        )
        .await;

        assert_eq!(session.outcome, SessionOutcome::Complete);
        let r0 = &session.rounds[0];
        assert_eq!(r0.tool_calls.len(), 2);
        assert!(r0.tool_calls[0].approved);
        assert!(!r0.tool_calls[1].approved);
        assert_eq!(r0.tool_calls[1].reason, "skip the second");
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn max_rounds_cap_is_honoured() {
        // Driver emits an unending stream of tool-call rounds.
        let mut replies = Vec::new();
        for i in 0..100 {
            replies.push(Proposal {
                text: String::new(),
                tool_calls: vec![read_tool(&format!("u{i}"), "x.md")],
            });
        }
        let driver = ScriptedDriver::new(replies);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "loop forever",
            "system",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::MaxRounds);
        assert_eq!(session.rounds.len() as u32, MAX_AGENT_ROUNDS);
    }

    /// Phase 2b smoke test: a policy that returns
    /// [`RoundDecision::Timeout`] flips the session outcome to
    /// `ApprovalTimeout` and records a synthetic stop-reason round.
    #[tokio::test]
    async fn timeout_decision_flips_outcome_to_approval_timeout() {
        struct TimeoutPolicy;
        #[async_trait]
        impl SessionPolicy for TimeoutPolicy {
            async fn allow_round(&self, _round: &ProposedRound) -> RoundDecision {
                RoundDecision::Timeout("no decision within 5 seconds".into())
            }
        }
        let driver = ScriptedDriver::new(vec![Proposal {
            text: String::new(),
            tool_calls: vec![read_tool("u1", "x.md")],
        }]);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &TimeoutPolicy,
            "do thing",
            "system",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::ApprovalTimeout);
        assert_eq!(session.rounds.len(), 2);
        assert!(session.rounds[1].text.contains("approval timeout"));
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn driver_error_records_outcome_errored() {
        struct FailingDriver;
        #[async_trait]
        impl ChatDriver for FailingDriver {
            async fn propose(&self, _: &str, _: &str) -> Result<Proposal, String> {
                Err("network down".into())
            }
        }
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &FailingDriver,
            &dispatcher,
            &AutoApproveAll,
            "do",
            "s",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::Errored);
        assert!(session.rounds[0].text.contains("driver error"));
    }
}
