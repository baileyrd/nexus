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

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use thiserror::Error;

mod agents;
mod archetypes;
pub mod core_plugin;
pub mod custom_agent;
mod llm;
pub mod memory;
pub mod session;
pub mod tool_registry;

pub use agents::EchoAgent;
pub use archetypes::{
    build_archetype, AUDITOR_ID, AUDITOR_SYSTEM_PROMPT, COACH_ID, COACH_SYSTEM_PROMPT, CODER_ID,
    CODER_SYSTEM_PROMPT, LIBRARIAN_ID, LIBRARIAN_SYSTEM_PROMPT, RESEARCHER_ID,
    RESEARCHER_SYSTEM_PROMPT, WRITER_ID, WRITER_SYSTEM_PROMPT,
};
pub use core_plugin::{
    AgentCorePlugin, HANDLER_DELEGATE, HANDLER_HISTORY_DELETE, HANDLER_HISTORY_GET,
    HANDLER_HISTORY_LIST, HANDLER_LIST_ARCHETYPES, HANDLER_LIST_CUSTOM, HANDLER_LIST_TOOLS,
    HANDLER_MEMORY_EXPORT, HANDLER_MEMORY_PRUNE, HANDLER_MEMORY_QUERY, HANDLER_MEMORY_RECORD,
    HANDLER_PLAN, PLUGIN_ID,
};
pub use custom_agent::{
    load_from_path as load_custom_agent, parse_str as parse_custom_agent_str,
    resolve_system_prompt as resolve_custom_system_prompt, scan_forge as scan_custom_agents,
    AgentSection, CustomAgentError, CustomAgentManifest, ExecutionSection, MemorySection,
    SystemPromptSection, ToolsSection, AGENTS_DIR, MANIFEST_FILE_NAME,
};
pub use tool_registry::{
    default_tool_catalog, measure_dispatch, seed_default_tools, AgentToolAccessRecord,
    AgentToolError, AgentToolRegistry, AgentToolSpec, Capability,
};
pub use llm::{ChatDriver, LlmAgent, Proposal, ProposedToolCall, DEFAULT_SYSTEM_PROMPT};
pub use session::{
    run_session, run_session_with_config, run_session_with_id, AgentSession, AutoApproveAll,
    ProposedRound, RoundDecision, RoundDecisionEntry, RoundRecord, SessionConfig,
    SessionOutcome, SessionPolicy, ToolCallRecord, DEFAULT_MAX_ITERATIONS,
    DEFAULT_MAX_TOOL_CALLS_PER_ITERATION, LEGACY_MAX_AGENT_ROUNDS, MAX_AGENT_ROUNDS,
};

/// BL-121 — FTS5-backed search over agent `history.jsonl` logs.
pub mod transcript_search;

/// BL-120 — context compression for the session loop. Defines the
/// [`compression::Compressor`] trait plus the default LLM-backed,
/// deterministic, and no-op implementations.
pub mod compression;

/// A unit of work produced by an [`Agent`] and consumed by a
/// [`PlanExecutor`]. Steps are deliberately simple — agents that
/// need branching or loops return a flat list today and re-plan if
/// results come back differently than expected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// Reverse-DNS id of the target plugin (e.g. `"com.nexus.storage"`).
    pub target_plugin_id: String,
    /// Command id within the target plugin (e.g. `"read_file"`).
    pub command_id: String,
    /// Arbitrary JSON payload — serialized and handed to the plugin.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub args: serde_json::Value,
}

/// Adapter for the transport layer the session loop (and agents
/// generally) use when dispatching a [`ToolCall`]. Implemented by
/// callers; in tree the production implementation is a thin
/// wrapper over [`nexus_kernel::PluginContext::ipc_call`] — but
/// keeping the trait here means the agent library itself stays
/// kernel-free and tests can supply a mock dispatcher.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    /// Dispatch a tool call and return its raw JSON response.
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String>;
}

/// An agent: produces a tool-call plan for a goal. After ADR 0025
/// Phase 2 the only producer in tree is [`LlmAgent`] driving
/// `propose_tool_calls`; the trait remains so out-of-tree
/// archetypes (and the existing [`EchoAgent`] fixture) keep
/// working.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Stable archetype id, e.g. `"com.nexus.agent.writer"`. Used
    /// for observability and to let plan persistence survive
    /// across agent-impl upgrades.
    fn id(&self) -> &str;

    /// Produce a plan for the given goal. Errors when the goal
    /// can't be decomposed (e.g. missing required context).
    async fn plan(&self, goal: &str) -> Result<Plan, AgentError>;
}

/// Errors the agent library can surface to its callers.
#[derive(Debug, Error)]
pub enum AgentError {
    /// The agent couldn't produce a plan — e.g. the goal was empty
    /// or referenced unavailable tools.
    #[error("planning failed: {0}")]
    PlanningFailed(String),
}
