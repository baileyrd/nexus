//! Agent system scaffold (PRD-15).
//!
//! Provides the core types every agent archetype will specialize:
//!
//! - [`Agent`] — trait implemented by concrete archetypes (Writer,
//!   Coder, Researcher, …). Given a natural-language `goal`, produces
//!   a [`Plan`] of [`Step`]s.
//! - [`Plan`] / [`Step`] — ordered list of atomic actions, optionally
//!   carrying a [`ToolCall`] the executor dispatches over kernel IPC.
//! - [`PlanExecutor`] — runs a plan step-by-step, delegating tool
//!   calls to an injected [`ToolDispatcher`] so the agent library
//!   doesn't depend on `nexus-kernel` directly.
//! - [`Observation`] — what the executor hands back to the agent (and
//!   eventually the UI): which steps ran, whether they succeeded,
//!   what tool calls produced.
//!
//! # What this is NOT (yet)
//!
//! - A core plugin. The `com.nexus.agent` dispatch surface will land
//!   once the planner + executor shape settles. The library-first
//!   posture matches PRDs 09/10/11 (`nexus-terminal`, `nexus-database`,
//!   `nexus-git`) — trait + tests first, IPC bridge second.
//! - A real planner. [`EchoAgent`] returns a single-step "respond with
//!   the goal" plan so the executor + observation shape can be
//!   exercised end-to-end. Real LLM-driven planners go through
//!   `com.nexus.ai` once the tool-calling handler lands.
//! - User-approval gates. PRD-15 §3.4 calls for per-step confirmation
//!   on destructive actions. [`StepPolicy`] reserves the slot; the
//!   executor honours it today by aborting when the policy rejects a
//!   step.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod agents;
mod archetypes;
pub mod core_plugin;
mod executor;
mod llm;
pub mod orchestrator;

pub use agents::EchoAgent;
pub use archetypes::{
    build_archetype, CODER_ID, CODER_SYSTEM_PROMPT, RESEARCHER_ID, RESEARCHER_SYSTEM_PROMPT,
    WRITER_ID, WRITER_SYSTEM_PROMPT,
};
pub use core_plugin::{
    AgentCorePlugin, HANDLER_DELEGATE, HANDLER_EXECUTE_STEP, HANDLER_HISTORY_DELETE,
    HANDLER_HISTORY_GET, HANDLER_HISTORY_LIST, HANDLER_LIST_ARCHETYPES, HANDLER_PARALLEL,
    HANDLER_PIPELINE, HANDLER_PLAN, HANDLER_RUN, HANDLER_RUN_PLAN, HANDLER_TRACE_GET,
    PLUGIN_ID,
};
pub use executor::{PlanExecutor, StepResult, StepStatus};
pub use llm::{ChatDriver, LlmAgent, DEFAULT_SYSTEM_PROMPT};
pub use orchestrator::{AgentOrchestrator, TraceEntry};

/// A unit of work produced by an [`Agent`] and consumed by a
/// [`PlanExecutor`]. Steps are deliberately simple — agents that
/// need branching or loops return a flat list today and re-plan if
/// results come back differently than expected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Unique id within the owning plan. Used for correlation in
    /// [`Observation`] and for UI affordances (approve / retry).
    pub id: String,
    /// Short human-readable description. Surfaced in approval
    /// prompts and the plan-progress view.
    pub description: String,
    /// Optional tool call. `None` means the step is informational —
    /// e.g. the agent is announcing a milestone or handing back a
    /// response to the user — and the executor treats it as a no-op
    /// that still appears in the observation log.
    pub tool_call: Option<ToolCall>,
}

/// A plan: ordered list of [`Step`]s produced by [`Agent::plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Opaque id — uniquely identifies this planning session for
    /// persistence and resume.
    pub id: String,
    /// The original natural-language goal. Kept so the UI (and future
    /// re-planners) can display it without callers threading it
    /// separately.
    pub goal: String,
    /// Steps, executed in order.
    pub steps: Vec<Step>,
}

impl Plan {
    /// Build a plan with a fresh UUID and the provided goal + steps.
    #[must_use]
    pub fn new(goal: impl Into<String>, steps: Vec<Step>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            goal: goal.into(),
            steps,
        }
    }
}

/// A single tool call — the unit of effect an [`Agent`] can request.
/// Shape mirrors `nexus_kernel::PluginContext::ipc_call` args so the
/// [`ToolDispatcher`] adapter can forward directly without reshaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Reverse-DNS id of the target plugin (e.g. `"com.nexus.storage"`).
    pub target_plugin_id: String,
    /// Command id within the target plugin (e.g. `"read_file"`).
    pub command_id: String,
    /// Arbitrary JSON payload — serialized and handed to the plugin.
    pub args: serde_json::Value,
}

/// Whether the executor may proceed with a given step. The
/// user-approval flow wraps a `StepPolicy` impl around a prompt; the
/// default [`AutoApprove`] policy accepts every step.
pub trait StepPolicy: Send + Sync {
    /// Called once per step before it's dispatched. Return
    /// [`PolicyDecision::Approve`] to run it, [`PolicyDecision::Deny`]
    /// to abort the plan.
    fn allow(&self, step: &Step) -> PolicyDecision;
}

/// Response a [`StepPolicy`] can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Run this step.
    Approve,
    /// Abort the plan with a human-readable reason.
    Deny(String),
}

/// Auto-approve every step. Useful for scripted / trusted agents;
/// interactive agents should pass their own policy through
/// [`PlanExecutor::run_with_policy`].
pub struct AutoApprove;

impl StepPolicy for AutoApprove {
    fn allow(&self, _step: &Step) -> PolicyDecision {
        PolicyDecision::Approve
    }
}

/// Adapter for the transport layer a [`PlanExecutor`] uses when a
/// step carries a [`ToolCall`]. Implemented by callers; in tree the
/// production implementation is a thin wrapper over
/// [`nexus_kernel::PluginContext::ipc_call`] — but keeping the trait
/// here means the agent library itself stays kernel-free and tests
/// can supply a mock dispatcher.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    /// Dispatch a tool call and return its raw JSON response. Errors
    /// are surfaced as [`AgentError::ToolFailed`] by the executor.
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String>;
}

/// An agent: produces plans from goals.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Stable archetype id, e.g. `"com.nexus.agent.writer"`. Used for
    /// observability and to let plan persistence survive across
    /// agent-impl upgrades.
    fn id(&self) -> &str;

    /// Produce a plan for the given goal. Errors when the goal can't
    /// be decomposed (e.g. missing required context).
    async fn plan(&self, goal: &str) -> Result<Plan, AgentError>;
}

/// What the executor hands back after running a [`Plan`]. A single
/// observation carries one entry per attempted step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// Plan id this observation describes.
    pub plan_id: String,
    /// Outcome per step, in execution order.
    pub steps: Vec<StepResult>,
    /// `true` when every step succeeded and the policy allowed each.
    pub success: bool,
}

/// Errors the agent library can surface to its callers.
#[derive(Debug, Error)]
pub enum AgentError {
    /// The agent couldn't produce a plan — e.g. the goal was empty
    /// or referenced unavailable tools.
    #[error("planning failed: {0}")]
    PlanningFailed(String),

    /// A step's tool call returned an error.
    #[error("tool call failed at step '{step_id}': {reason}")]
    ToolFailed {
        /// The failing step's id.
        step_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// The policy denied a step.
    #[error("step '{step_id}' denied by policy: {reason}")]
    StepDenied {
        /// The denied step's id.
        step_id: String,
        /// Denial reason.
        reason: String,
    },
}
