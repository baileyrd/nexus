//! Per-session identity and state-machine types for the AI-native runtime.
//!
//! A [`Session`] is one perceive-reason-act-observe loop instance. The
//! [`crate::supervisor::Supervisor`] owns all live sessions; external callers
//! interact through IPC using [`crate::AiRuntimeSubmitArgs`].
//!
//! ## State machine
//!
//! ```text
//! Idle → Perceiving → Reasoning { call_id }
//!                   ↓
//!                Acting { proposal_id }
//!                   ↓
//!                Observing → Perceiving   (next cycle)
//!                          → Terminal     (done / aborted / failed)
//! ```
//!
//! Every variant carries exactly the data it needs; the compiler rejects
//! impossible states (e.g. `Acting` without a `proposal_id`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

pub use nexus_plugin_api::session::{SessionKind, SessionOutcome};
use nexus_plugin_api::token::CapabilityToken;

/// Opaque session identifier — a `Uuid` newtype that prevents accidental
/// interchange with task IDs or run IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct SessionId(pub uuid::Uuid);

impl SessionId {
    /// Allocate a fresh random session id.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<uuid::Uuid> for SessionId {
    fn from(u: uuid::Uuid) -> Self {
        Self(u)
    }
}

/// Fine-grained loop state of a live session.
///
/// A typed sum type so the compiler rejects impossible combinations
/// (e.g. `Reasoning` without a `call_id`). Every non-terminal state
/// transition is observable on the bus via the session lifecycle topics
/// defined in [`crate::supervisor`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    /// Constructed and queued but not yet processing an event.
    Idle,
    /// Consuming and classifying incoming events into structured context.
    Perceiving,
    /// Waiting on a model response; `call_id` identifies the in-flight
    /// call for correlation and cancellation.
    Reasoning {
        /// Opaque identifier for the in-flight model call.
        call_id: uuid::Uuid,
    },
    /// Model produced a proposal; pending capability-gate + snapshot
    /// approval before the action is committed.
    Acting {
        /// Opaque identifier for the pending proposal (correlates with
        /// the snapshot layer once Move 3 lands).
        proposal_id: uuid::Uuid,
    },
    /// Collecting and recording the outcome of a committed action.
    Observing,
    /// Terminal — the session loop has exited. Inspect the inner
    /// [`SessionOutcome`] to distinguish normal completion from aborts
    /// and hard failures.
    Terminal(SessionOutcome),
}

impl SessionState {
    /// `true` if the session has reached a non-live terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

/// Resource envelope for a session. The supervisor checks each stage
/// against the applicable ceiling and transitions the session to
/// `Terminal(Aborted)` when any ceiling is exceeded.
///
/// All `*_used` fields are updated by the supervisor as work proceeds.
/// Wall-time and cost ceilings are advisory in Phase 1 (logged on
/// breach; hard enforcement lands when the billing integration ships).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct Budget {
    /// Maximum tokens the session may spend on model calls.
    pub max_tokens: u32,
    /// Wall-clock ceiling for the entire session in seconds.
    pub max_wall_secs: u32,
    /// Soft cost ceiling in USD.
    pub max_cost_usd: f64,
    /// Tokens consumed across all model calls so far.
    pub tokens_used: u32,
    /// Elapsed wall-clock seconds so far.
    pub wall_secs_used: u32,
    /// Estimated USD cost consumed so far.
    pub cost_usd_used: f64,
}

impl Budget {
    /// Construct a budget with the given ceilings; all `*_used` fields
    /// start at zero.
    #[must_use]
    pub fn new(max_tokens: u32, max_wall_secs: u32, max_cost_usd: f64) -> Self {
        Self {
            max_tokens,
            max_wall_secs,
            max_cost_usd,
            tokens_used: 0,
            wall_secs_used: 0,
            cost_usd_used: 0.0,
        }
    }

    /// `true` when the token ceiling is at or exceeded.
    #[must_use]
    pub fn tokens_exhausted(&self) -> bool {
        self.tokens_used >= self.max_tokens
    }

    /// `true` when the wall-clock ceiling has elapsed.
    #[must_use]
    pub fn wall_time_exhausted(&self) -> bool {
        self.wall_secs_used >= self.max_wall_secs
    }

    /// Remaining token headroom (saturating at zero).
    #[must_use]
    pub fn tokens_remaining(&self) -> u32 {
        self.max_tokens.saturating_sub(self.tokens_used)
    }

    /// Returns a child budget with the given token share carved out of
    /// this budget's remaining headroom. Used by sub-agent delegation
    /// (Move 2 / sub-session spawning).
    #[must_use]
    pub fn derive_slice(&self, max_tokens: u32, max_wall_secs: u32) -> Self {
        Self::new(
            max_tokens.min(self.tokens_remaining()),
            max_wall_secs.min(self.max_wall_secs.saturating_sub(self.wall_secs_used)),
            self.max_cost_usd - self.cost_usd_used,
        )
    }
}

/// Sensible defaults for an interactive user-driven session.
impl std::fmt::Display for Budget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tokens={}/{} wall={}/{}s cost=${:.4}/${:.4}",
            self.tokens_used,
            self.max_tokens,
            self.wall_secs_used,
            self.max_wall_secs,
            self.cost_usd_used,
            self.max_cost_usd,
        )
    }
}

/// One step the session loop should take after a state transition.
///
/// Returned by the supervisor's state-advance logic. Keeping `Step` as
/// a pure value (not a closure or future) lets the supervisor's
/// scheduler decide execution order and enables replay-testing without
/// running a real model.
#[derive(Debug)]
pub enum Step {
    /// Build context from the current perceived event and call the model.
    /// The `call_id` is pre-allocated so it can be stored in
    /// `SessionState::Reasoning` before the async call is dispatched.
    Reason(uuid::Uuid),
    /// Submit a typed proposal to the capability gate for review.
    Propose(uuid::Uuid),
    /// Commit an approved proposal through the capability system.
    Commit(uuid::Uuid),
    /// Record an observation and prepare the next perceive cycle.
    Observe(serde_json::Value),
    /// No work to do this cycle; yield to the runtime scheduler.
    Yield,
    /// The session has reached a terminal state.
    Done(SessionOutcome),
}

/// Lightweight session record held by the [`crate::supervisor::Supervisor`].
///
/// The full agent transcript (tool calls, model turns, compaction
/// records) lives in `nexus-agent`'s session store, keyed by the
/// matching string form of [`SessionId`]. This struct is the
/// supervisor's ledger: identity, lifecycle state, budget accounting,
/// and the link back to the runtime's task store.
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique session identifier threaded through bus topics and
    /// `com.nexus.agent::session_run` args.
    pub id: SessionId,
    /// What kind of work this session is doing.
    pub kind: SessionKind,
    /// Fine-grained loop state.
    pub state: SessionState,
    /// Resource envelope — updated by the supervisor as work proceeds.
    pub budget: Budget,
    /// Live, revocable capability envelope for this session. Minted by
    /// the Supervisor at submission time; revoking it immediately
    /// invalidates all capability checks for this session and any
    /// child tokens derived via [`CapabilityToken::attenuate`].
    pub capabilities: CapabilityToken,
    /// When the supervisor accepted this session.
    pub submitted_at: DateTime<Utc>,
    /// When a worker first picked it up; `None` while queued.
    pub started_at: Option<DateTime<Utc>>,
    /// When the session reached a terminal state; `None` while live.
    pub finished_at: Option<DateTime<Utc>>,
}

impl Session {
    /// Create a new session in [`SessionState::Idle`].
    #[must_use]
    pub fn new(
        id: SessionId,
        kind: SessionKind,
        budget: Budget,
        capabilities: CapabilityToken,
    ) -> Self {
        Self {
            id,
            kind,
            state: SessionState::Idle,
            budget,
            capabilities,
            submitted_at: Utc::now(),
            started_at: None,
            finished_at: None,
        }
    }

    /// Mark the session as started (transitions from `Idle` → `Perceiving`).
    /// No-op if already past `Idle`.
    pub fn mark_started(&mut self) {
        if matches!(self.state, SessionState::Idle) {
            self.started_at = Some(Utc::now());
            self.state = SessionState::Perceiving;
        }
    }

    /// Transition to a terminal state and record the finish timestamp.
    pub fn mark_terminal(&mut self, outcome: SessionOutcome) {
        self.finished_at = Some(Utc::now());
        self.state = SessionState::Terminal(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_plugin_api::CapabilitySet;

    fn test_token(id: &SessionId) -> CapabilityToken {
        CapabilityToken::new(id.as_uuid(), CapabilitySet::default())
    }

    #[test]
    fn session_starts_idle() {
        let id = SessionId::new();
        let s = Session::new(
            id,
            SessionKind::UserDriven,
            Budget::default(),
            test_token(&id),
        );
        assert!(matches!(s.state, SessionState::Idle));
        assert!(!s.state.is_terminal());
    }

    #[test]
    fn mark_started_transitions_to_perceiving() {
        let id = SessionId::new();
        let mut s = Session::new(id, SessionKind::Ambient, Budget::default(), test_token(&id));
        s.mark_started();
        assert!(matches!(s.state, SessionState::Perceiving));
        assert!(s.started_at.is_some());
    }

    #[test]
    fn mark_started_is_idempotent_past_idle() {
        let id = SessionId::new();
        let mut s = Session::new(id, SessionKind::Ambient, Budget::default(), test_token(&id));
        s.mark_started();
        s.state = SessionState::Reasoning {
            call_id: uuid::Uuid::new_v4(),
        };
        s.mark_started(); // must not overwrite state
        assert!(matches!(s.state, SessionState::Reasoning { .. }));
    }

    #[test]
    fn mark_terminal_sets_finished_at_and_outcome() {
        let id = SessionId::new();
        let mut s = Session::new(
            id,
            SessionKind::UserDriven,
            Budget::default(),
            test_token(&id),
        );
        s.mark_terminal(SessionOutcome::Completed);
        assert!(s.state.is_terminal());
        assert!(s.finished_at.is_some());
    }

    #[test]
    fn budget_tokens_exhausted() {
        let mut b = Budget::new(100, 3600, 10.0);
        assert!(!b.tokens_exhausted());
        b.tokens_used = 100;
        assert!(b.tokens_exhausted());
    }

    #[test]
    fn budget_derive_slice_caps_at_remaining() {
        let mut parent = Budget::new(1000, 7200, 5.0);
        parent.tokens_used = 800;
        let child = parent.derive_slice(500, 3600);
        assert_eq!(child.max_tokens, 200); // capped at remaining 200
        assert_eq!(child.max_wall_secs, 3600);
    }

    #[test]
    fn session_id_roundtrips_via_uuid() {
        let id = SessionId::new();
        let uuid = id.as_uuid();
        let round = SessionId::from(uuid);
        assert_eq!(id, round);
    }
}
