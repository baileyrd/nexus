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

/// Legacy hard cap on round count (the BL-119-pre default). Kept
/// `pub` so external tests that pin against the ADR 0024 Phase 2a
/// shipping value still compile. New callers should read
/// [`SessionConfig::max_iterations`] instead — which defaults to
/// [`DEFAULT_MAX_ITERATIONS`].
pub const MAX_AGENT_ROUNDS: u32 = LEGACY_MAX_AGENT_ROUNDS;

/// The original Phase 2a shipping cap (8 rounds). Exposed so the
/// existing regression test can pin the legacy behaviour through
/// an explicit `SessionConfig` override (per BL-119 DoD).
pub const LEGACY_MAX_AGENT_ROUNDS: u32 = 8;

/// BL-119 — new default for [`SessionConfig::max_iterations`].
/// Raised from the Phase 2a value of 8 to 32 per the Hermes Feature 1
/// recommendation ("8 iterations is the floor for non-trivial
/// multi-step tasks"). Callers can lower it explicitly through
/// `SessionConfig`; raising it further is a config-level decision
/// rather than a kernel-side default change.
pub const DEFAULT_MAX_ITERATIONS: u32 = 32;

/// BL-119 — default cap on tool calls per iteration. The agent
/// dispatcher already accepts whatever the model emits; this cap
/// guards against a runaway round that emits 100 tool_use blocks
/// at once. Excess calls are dropped (with a tracing warning) at
/// the start of [`execute_round`] so the dispatcher's downstream
/// work and the per-call result fan-out stay bounded.
pub const DEFAULT_MAX_TOOL_CALLS_PER_ITERATION: u32 = 16;

/// BL-119 — provider-routing + budget knobs for a single agent
/// session.
///
/// Constructed via [`SessionConfig::default`] (recommended) or
/// [`SessionConfig::legacy_phase2a`] when a caller needs to pin the
/// pre-BL-119 cap. The struct is the wire shape too — `session_run`
/// accepts it directly as `session_config`.
///
/// All fields default to "use the documented default"; an
/// explicitly-zero `max_iterations` is treated as a configuration
/// error and clamped to `1` so a misconfigured caller can't deadlock
/// the dispatch loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// Hard cap on the number of model-driven rounds. Defaults to
    /// [`DEFAULT_MAX_ITERATIONS`] (32). A session that hits the cap
    /// exits with [`SessionOutcome::MaxRounds`].
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Cap on tool calls per round. The dispatcher drops excess
    /// calls (with a tracing warning) so a runaway round can't fan
    /// out into hundreds of nested operations. Defaults to
    /// [`DEFAULT_MAX_TOOL_CALLS_PER_ITERATION`] (16).
    #[serde(default = "default_max_tool_calls_per_iteration")]
    pub max_tool_calls_per_iteration: u32,

    /// Context-budget guardrail consumed by BL-120's compression
    /// pass. `0` means "unbounded" — the v1 dispatch loop honours
    /// this field by passing it through but does not compress
    /// turns; BL-120 wires the compressor to read this value.
    #[serde(default)]
    pub max_context_tokens: u32,

    /// Provider-routing hint. v1 accepts the field for forward-
    /// compat with BL-119's "provider-routing hints" DoD bullet but
    /// the dispatch loop does not yet consult it — the AI plugin's
    /// configured provider is still authoritative. A future BL
    /// (Hermes Features 2–3) will let an agent pick a different
    /// provider per session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_hint: Option<String>,
}

fn default_max_iterations() -> u32 {
    DEFAULT_MAX_ITERATIONS
}

fn default_max_tool_calls_per_iteration() -> u32 {
    DEFAULT_MAX_TOOL_CALLS_PER_ITERATION
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_tool_calls_per_iteration: DEFAULT_MAX_TOOL_CALLS_PER_ITERATION,
            max_context_tokens: 0,
            provider_hint: None,
        }
    }
}

impl SessionConfig {
    /// Pre-BL-119 default — `max_iterations = 8`. Existing tests
    /// pin this to keep behaviour identical to the Phase 2a
    /// shipping value while the new default rolls out elsewhere.
    #[must_use]
    pub fn legacy_phase2a() -> Self {
        Self {
            max_iterations: LEGACY_MAX_AGENT_ROUNDS,
            ..Self::default()
        }
    }

    /// Clamp obviously-bogus values into a runnable shape. `0`
    /// iterations would deadlock the loop; same for `0` tool calls
    /// per iteration. Returns a fresh config; the original is left
    /// untouched.
    #[must_use]
    pub fn sanitized(&self) -> Self {
        Self {
            max_iterations: self.max_iterations.max(1),
            max_tool_calls_per_iteration: self.max_tool_calls_per_iteration.max(1),
            max_context_tokens: self.max_context_tokens,
            provider_hint: self.provider_hint.clone(),
        }
    }
}

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
    run_session_with_config(
        driver,
        dispatcher,
        policy,
        goal,
        system,
        archetype,
        id,
        SessionConfig::default(),
    )
    .await
}

/// BL-119 — full entry point taking an explicit [`SessionConfig`].
/// Existing [`run_session`] / [`run_session_with_id`] wrappers
/// delegate here with `SessionConfig::default()`, so a caller that
/// wants the legacy 8-round cap (or any other override) drops in
/// at this surface. The struct is `Clone` + cheap to construct;
/// the loop sanitises the values internally so a misconfigured
/// `max_iterations = 0` clamps to 1 rather than deadlocking.
pub async fn run_session_with_config<D, P, T>(
    driver: &D,
    dispatcher: &T,
    policy: &P,
    goal: &str,
    system: &str,
    archetype: Option<String>,
    id: String,
    config: SessionConfig,
) -> AgentSession
where
    D: ChatDriver + ?Sized,
    P: SessionPolicy + ?Sized,
    T: ToolDispatcher + ?Sized,
{
    let config = config.sanitized();
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

    for round_idx in 1..=config.max_iterations {
        // Ask the model for this round's tool calls.
        let mut proposal = match driver.propose(system, &current_prompt).await {
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

        // BL-119 — guard against runaway rounds. Truncate excess
        // tool calls so the dispatcher's downstream work stays
        // bounded; drops are logged so the user sees a clear
        // signal in operator logs.
        let cap = config.max_tool_calls_per_iteration as usize;
        if proposal.tool_calls.len() > cap {
            tracing::warn!(
                round = round_idx,
                proposed = proposal.tool_calls.len(),
                cap,
                "BL-119: dropping excess tool calls; raise max_tool_calls_per_iteration to keep them"
            );
            proposal.tool_calls.truncate(cap);
        }

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

        if round_idx == config.max_iterations {
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
        // BL-119 — the new default is 32 iterations; pin the legacy
        // Phase 2a cap (8) through an explicit `SessionConfig` so
        // this regression test still asserts the same behaviour.
        let session = run_session_with_config(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "loop forever",
            "system",
            None,
            "legacy-cap".to_string(),
            SessionConfig::legacy_phase2a(),
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::MaxRounds);
        assert_eq!(session.rounds.len() as u32, LEGACY_MAX_AGENT_ROUNDS);
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

    // ── BL-119 SessionConfig tests ─────────────────────────────────

    #[test]
    fn session_config_defaults_match_hermes_recommendation() {
        let cfg = SessionConfig::default();
        assert_eq!(cfg.max_iterations, DEFAULT_MAX_ITERATIONS);
        assert_eq!(cfg.max_iterations, 32);
        assert_eq!(
            cfg.max_tool_calls_per_iteration,
            DEFAULT_MAX_TOOL_CALLS_PER_ITERATION,
        );
        assert_eq!(cfg.max_context_tokens, 0);
        assert!(cfg.provider_hint.is_none());
    }

    #[test]
    fn legacy_phase2a_preserves_old_cap() {
        let cfg = SessionConfig::legacy_phase2a();
        assert_eq!(cfg.max_iterations, LEGACY_MAX_AGENT_ROUNDS);
        assert_eq!(cfg.max_iterations, 8);
    }

    #[test]
    fn session_config_sanitized_clamps_zero_to_one() {
        let cfg = SessionConfig {
            max_iterations: 0,
            max_tool_calls_per_iteration: 0,
            max_context_tokens: 0,
            provider_hint: None,
        };
        let s = cfg.sanitized();
        assert_eq!(s.max_iterations, 1);
        assert_eq!(s.max_tool_calls_per_iteration, 1);
    }

    #[test]
    fn session_config_serde_round_trips_with_partial_input() {
        // BL-119 callers should be able to pass `{}` and get the
        // BL-119 defaults; the IPC arg path uses #[serde(default)].
        let cfg: SessionConfig = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(cfg.max_iterations, DEFAULT_MAX_ITERATIONS);
        // Explicit override echoes through.
        let cfg: SessionConfig = serde_json::from_value(
            serde_json::json!({ "max_iterations": 12, "provider_hint": "anthropic" }),
        )
        .unwrap();
        assert_eq!(cfg.max_iterations, 12);
        assert_eq!(cfg.provider_hint.as_deref(), Some("anthropic"));
    }

    #[tokio::test]
    async fn default_max_iterations_supports_more_than_legacy_cap() {
        // Driver yields tool calls for 16 rounds (exceeds legacy
        // cap of 8) then a terminal text-only round. With the
        // default config the session should complete normally.
        let mut replies = Vec::new();
        for i in 0..16 {
            replies.push(Proposal {
                text: String::new(),
                tool_calls: vec![read_tool(&format!("r{i}"), "x.md")],
            });
        }
        replies.push(Proposal {
            text: "all done".into(),
            tool_calls: Vec::new(),
        });
        let driver = ScriptedDriver::new(replies);
        let dispatcher = CountingDispatcher::new();
        let session = run_session(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "go far",
            "system",
            None,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::Complete);
        assert!(session.rounds.len() > LEGACY_MAX_AGENT_ROUNDS as usize);
    }

    #[tokio::test]
    async fn excess_tool_calls_per_iteration_are_truncated() {
        // Single round with 5 tool calls; cap at 2.
        let many = vec![
            read_tool("a", "1.md"),
            read_tool("b", "2.md"),
            read_tool("c", "3.md"),
            read_tool("d", "4.md"),
            read_tool("e", "5.md"),
        ];
        let driver = ScriptedDriver::new(vec![
            Proposal {
                text: "round 1".into(),
                tool_calls: many,
            },
            Proposal {
                text: "done".into(),
                tool_calls: Vec::new(),
            },
        ]);
        let dispatcher = CountingDispatcher::new();
        let cfg = SessionConfig {
            max_tool_calls_per_iteration: 2,
            ..SessionConfig::default()
        };
        let session = run_session_with_config(
            &driver,
            &dispatcher,
            &AutoApproveAll,
            "cap-tools",
            "system",
            None,
            "cap-test".to_string(),
            cfg,
        )
        .await;
        assert_eq!(session.outcome, SessionOutcome::Complete);
        // Only 2 calls dispatched in round 1.
        assert_eq!(session.rounds[0].tool_calls.len(), 2);
    }
}
