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
use thiserror::Error;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

mod agents;
mod archetypes;
/// BL-133 follow-up — auto-notify subscriber for completed agent sessions.
pub mod auto_notify;
pub mod core_plugin;
pub mod custom_agent;
mod handlers;
mod llm;
pub mod memory;
pub mod session;
// C29 (#382) — session write-snapshots + revert.
pub mod snapshots;
/// RFC 0007 PR 1 — headless subagent process spawning (the lowest layer of
/// process-level subagent isolation; not yet wired into `delegate`).
pub mod subagent;
mod todo;
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
    AgentSection, CustomAgentError, CustomAgentManifest, ExecutionSection, ManifestPolicyGate,
    ManifestToolPolicy, MemorySection, SystemPromptSection, ToolsSection, AGENTS_DIR,
    MANIFEST_FILE_NAME,
};
pub use llm::{
    flatten_turns_to_prompt, AgentChatTurn, AgentTurnToolCall, ChatDriver, LlmAgent, Proposal,
    ProposedToolCall, DEFAULT_SYSTEM_PROMPT,
};
pub use session::{
    is_retryable_tool_error, run_session, run_session_resumed, run_session_with_config,
    run_session_with_id, AgentSession, AutoApproveAll, ProposedRound, RoundDecision,
    RoundDecisionEntry, RoundRecord, SessionCheckpoint, SessionConfig, SessionOutcome,
    SessionPolicy, ToolCallRecord, DEFAULT_MAX_ITERATIONS, DEFAULT_MAX_TOOL_CALLS_PER_ITERATION,
    DEFAULT_TOOL_RETRY_BACKOFF_MS, LEGACY_MAX_AGENT_ROUNDS, MAX_AGENT_ROUNDS,
};
pub use tool_registry::{
    default_tool_catalog, measure_dispatch, seed_default_tools, AgentToolAccessRecord,
    AgentToolError, AgentToolRegistry, AgentToolSpec, Capability,
};

/// BL-121 — FTS5-backed search over agent `history.jsonl` logs.
pub mod transcript_search;

/// BL-120 — context compression for the session loop. Defines the
/// [`compression::Compressor`] trait plus the default LLM-backed,
/// deterministic, and no-op implementations.
pub mod compression;

/// BL-131 — pre-invocation context sanitisation. Four pure passes
/// (dedup repeated results, strip base64 data URIs, compress stale
/// browser snapshots, hard-trim to budget) applied to the assembled
/// prompt just before each chat-driver invocation.
pub mod context_sanitize;

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

/// How a [`ToolDispatcher`] failure should be treated by the session
/// retry policy ([`session::SessionConfig::max_tool_retries`]).
///
/// Before typed dispatch errors the trait returned a bare `String`, and
/// the loop recovered an *approximate* retry decision by string-sniffing
/// it ([`is_retryable_tool_error`]). A dispatcher that knows the exact
/// nature of a failure — the kernel IPC bridge, whose `IpcError` carries
/// an authoritative `retryable` flag — classifies it precisely instead,
/// so a `Timeout` is always retried and a `CapabilityDenied` never is,
/// regardless of how their messages happen to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    /// The failure looks transient — a retry may succeed (timeout,
    /// transport reset, rate limit, 5xx, a cancelled call).
    Transient,
    /// The failure is permanent — a retry cannot change the outcome
    /// (not-found, validation, capability / policy denial).
    Permanent,
    /// The dispatcher could not classify the failure. The retry policy
    /// falls back to the [`is_retryable_tool_error`] heuristic over
    /// [`ToolDispatchError::message`]. This is what every `String` /
    /// `&str` conversion produces, so dispatchers that only have a
    /// message string keep their pre-typed behaviour exactly.
    Unknown,
}

/// Structured error returned by [`ToolDispatcher::dispatch`].
///
/// Carries the human-readable `message` (what the dispatcher used to
/// return as a bare `String`, surfaced verbatim in the transcript) plus a
/// [`ToolErrorKind`] the session loop consults to decide whether to retry.
/// `From<String>` / `From<&str>` produce an [`ToolErrorKind::Unknown`]
/// error so existing `map_err(|e| e.to_string())`-style call sites migrate
/// with a single `.into()`.
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct ToolDispatchError {
    /// Human-readable failure message. Surfaced in the transcript and,
    /// for [`ToolErrorKind::Unknown`] errors, sniffed by the retry
    /// heuristic.
    pub message: String,
    /// Retry classification — see [`ToolErrorKind`].
    pub kind: ToolErrorKind,
}

impl ToolDispatchError {
    /// A failure classified as transient (retryable).
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToolErrorKind::Transient,
        }
    }

    /// A failure classified as permanent (never retried).
    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToolErrorKind::Permanent,
        }
    }

    /// An unclassified failure — the retry policy sniffs the message via
    /// [`is_retryable_tool_error`].
    #[must_use]
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToolErrorKind::Unknown,
        }
    }

    /// Build from an explicit `retryable` flag — e.g. an
    /// `IpcErrorEnvelope`'s `retryable`, the authoritative source of
    /// truth for IPC failures. Maps `true` to [`ToolErrorKind::Transient`]
    /// and `false` to [`ToolErrorKind::Permanent`].
    #[must_use]
    pub fn classified(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            message: message.into(),
            kind: if retryable {
                ToolErrorKind::Transient
            } else {
                ToolErrorKind::Permanent
            },
        }
    }

    /// Whether the session loop should retry this failure. Exact for
    /// [`ToolErrorKind::Transient`] / [`ToolErrorKind::Permanent`]; for
    /// [`ToolErrorKind::Unknown`] it falls back to the
    /// [`is_retryable_tool_error`] message heuristic.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self.kind {
            ToolErrorKind::Transient => true,
            ToolErrorKind::Permanent => false,
            ToolErrorKind::Unknown => is_retryable_tool_error(&self.message),
        }
    }
}

impl From<String> for ToolDispatchError {
    fn from(message: String) -> Self {
        Self::unknown(message)
    }
}

impl From<&str> for ToolDispatchError {
    fn from(message: &str) -> Self {
        Self::unknown(message)
    }
}

/// Adapter for the transport layer the session loop (and agents
/// generally) use when dispatching a [`ToolCall`]. Implemented by
/// callers; in tree the production implementation is a thin
/// wrapper over [`nexus_kernel::PluginContext::ipc_call`] — but
/// keeping the trait here means the agent library itself stays
/// kernel-free and tests can supply a mock dispatcher.
#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    /// Dispatch a tool call and return its raw JSON response, or a
    /// [`ToolDispatchError`] whose [`ToolErrorKind`] drives the retry
    /// policy. Dispatchers that can classify the failure exactly should;
    /// those that only have a message can return it via `.into()`
    /// (classified as [`ToolErrorKind::Unknown`]).
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, ToolDispatchError>;
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
