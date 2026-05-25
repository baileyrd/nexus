//! BL-134 / ADR 0028 — unified AI/agent event loop.
//!
//! This crate is the home for three primitives that previously lived as
//! prototypes scattered across `nexus-agent`, `nexus-ai`, and
//! `nexus-workflow`:
//!
//! 1. **Agent task scheduler** — typed [`AgentTask`] / [`AgentRun`]
//!    enqueued via the `submit` IPC handler.
//! 2. **Typed AI event channel** — every existing
//!    `com.nexus.ai.stream_*` and `com.nexus.agent.*` bus topic gets a
//!    typed [`AiEvent`] envelope republished under `com.nexus.ai.runtime.*`.
//! 3. **Dedicated worker pool** — a multi-thread tokio runtime owned
//!    by the plugin so long-running LLM rounds cannot starve the host
//!    runtime serving UI / IPC traffic.
//!
//! ## Phase 1 scope (what this commit ships)
//!
//! Only [`AgentTaskKind::Session`] is wired. `submit` enqueues the
//! task, the worker pool drives `com.nexus.agent::session_run` to
//! completion, the bus republisher emits typed [`AiEvent`]s, and the
//! `events` IPC handler returns the per-run replay buffer.
//!
//! Cancel / pause / resume, agent-side `delegate` shim, workflow
//! `async = true` step parsing, and indexing-daemon migration land in
//! later phases per ADR 0028 §Migration.
//!
//! ## What is NOT in this crate
//!
//! - `SessionRunArgs` / `StreamChatArgs` — those types live in
//!   `nexus-agent` / `nexus-ai`. We carry them across IPC as
//!   `serde_json::Value` bodies; the runtime only needs to know the
//!   target plugin + command + arg shape, not their Rust types. This
//!   keeps the crate dependency surface minimal (no edge to
//!   `nexus-agent` / `nexus-ai`).
//! - The republisher's tap into `com.nexus.ai.*` / `com.nexus.agent.*`
//!   topics — see [`events`] for the typed wrapper but the actual
//!   subscription happens in `core_plugin::wire_context` once the
//!   plugin owns its `KernelPluginContext`.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

pub mod core_plugin;
pub mod events;
pub mod pool;
pub mod republisher;
pub mod scheduler;
pub mod session;
pub mod supervisor;

/// BL-134 Phase 4 — process-wide shared tokio handle accessor.
/// Re-exported from [`pool::shared_pool_handle`] for ergonomic
/// `nexus_ai_runtime::shared_pool_handle()` calls from sibling
/// subsystems (today: `nexus-ai::indexing_daemon`).
pub use pool::shared_pool_handle;

/// Re-export session identity types so callers only need one `use`
/// path for the AI-runtime's session surface.
pub use session::{Budget, Session, SessionId, SessionKind, SessionOutcome, SessionState, Step};
/// Re-export the Supervisor and its admission-control config.
pub use supervisor::{AdmissionConfig, Supervisor};

/// Reverse-DNS plugin id — also the bus-topic prefix the republisher
/// owns (`com.nexus.ai.runtime.*`).
pub const PLUGIN_ID: &str = "com.nexus.ai.runtime";

/// Bus-topic prefix used by every typed [`AiEvent`] republished by
/// the runtime. Consumers subscribe with
/// `EventFilter::CustomPrefix(BUS_TOPIC_PREFIX.into())` to receive the
/// full typed stream regardless of which inner topic carried the
/// underlying raw event.
pub const BUS_TOPIC_PREFIX: &str = "com.nexus.ai.runtime.";

/// Hard cap on the per-run replay buffer returned by the `events` IPC
/// handler. The bus stream is the canonical live channel; the buffer
/// is a bounded ring so a popout window opening late can backfill the
/// last N events without unbounded memory growth.
pub const PER_RUN_EVENT_BUFFER_CAP: usize = 256;

// ─── Wire types ──────────────────────────────────────────────────────────────

/// Priority bucket for an [`AgentTask`]. The Phase-1 scheduler treats
/// every priority equally (FIFO across the single queue); BL-134
/// Phase 5 splits the pool into per-priority `JoinSet`s so `Critical`
/// can preempt a `Background` burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    /// Indexing-daemon-style work the user is not waiting on.
    Background,
    /// Default. Interactive agent runs the user is watching.
    #[default]
    Interactive,
    /// Reserved for Phase 5 — preempts other priorities.
    Critical,
}

/// Discriminator + payload for a queued task. Phase 1 only handles
/// [`AgentTaskKind::Session`]; the other variants are reserved so the
/// wire shape doesn't break when later phases add them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentTaskKind {
    /// Drive `com.nexus.agent::session_run` to completion. The body
    /// is forwarded verbatim to the agent plugin.
    Session {
        /// Args passed to `com.nexus.agent::session_run`.
        #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
        args: serde_json::Value,
    },
    /// Reserved for Phase 2+ — single AI streaming turn.
    #[serde(rename = "ai_stream")]
    AiStream {
        /// Args passed to `com.nexus.ai::stream_chat`.
        #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
        args: serde_json::Value,
    },
    /// BL-134 Phase 3 — async workflow step. The workflow executor
    /// packages the underlying IPC dispatch (e.g.
    /// `("com.nexus.ai", "ask", …)` for `ai_prompt`;
    /// `("com.nexus.notifications", "send", …)` for `notify`) and
    /// hands it to the runtime so the step doesn't block the
    /// workflow's per-step await loop. The runtime worker fires the
    /// `ipc_call` exactly as the workflow would have, and the
    /// run's terminal `Finished` event carries the reply verbatim
    /// (no kind-specific shaping). `workflow` + `step` are recorded
    /// for observability — the run-history panel can group async
    /// steps under their parent workflow run.
    WorkflowAiStep {
        /// Reverse-DNS plugin id to dispatch into — e.g.
        /// `"com.nexus.ai"` for `ai_prompt` / `ai_decision`,
        /// `"com.nexus.notifications"` for `notify`.
        target_plugin: String,
        /// Command name on the target plugin (e.g. `"ask"`).
        command: String,
        /// Args passed to the underlying IPC handler.
        #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
        args: serde_json::Value,
        /// Workflow id (observability — runtime stores it on the run).
        workflow: String,
        /// Step index within the workflow (also observability-only).
        step: u32,
    },
}

impl AgentTaskKind {
    /// Stable short label for tracing / [`AiEvent::Submitted`].
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Session { .. } => "session",
            Self::AiStream { .. } => "ai_stream",
            Self::WorkflowAiStep { .. } => "workflow_ai_step",
        }
    }
}

/// Submission envelope. The IPC `submit` handler accepts this shape
/// directly; `task_id` is generated server-side and returned in the
/// reply (callers MAY pre-supply one but the Phase-1 server ignores
/// it to keep id allocation centralised).
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
pub struct AiRuntimeSubmitArgs {
    /// What kind of task to run + the args.
    pub task: AgentTaskKind,
    /// Priority bucket. Defaults to [`TaskPriority::Interactive`].
    #[serde(default)]
    pub priority: TaskPriority,
    /// Session kind — determines budget tier, latency target, and
    /// output destination. Defaults to [`SessionKind::UserDriven`].
    /// Callers that don't supply this field continue to work unchanged.
    #[serde(default)]
    pub kind: SessionKind,
    /// Optional parent task id — Phase 2+ uses this for delegate /
    /// fan-out composition. Phase 1 records the value for
    /// observability but does not act on it.
    #[serde(default)]
    pub parent: Option<uuid::Uuid>,
}

/// Reply from `submit`. The caller subscribes to
/// `com.nexus.ai.runtime.*` (filtered by `task_id`) for live updates,
/// or polls `events`/`get`.
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
pub struct AiRuntimeSubmitReply {
    /// Server-allocated task id.
    pub task_id: uuid::Uuid,
}

/// Live state of a submitted task, returned by `get` and embedded in
/// `list` summaries. The `events` field carries the bounded replay
/// buffer (most recent up to [`PER_RUN_EVENT_BUFFER_CAP`]) so a fresh
/// observer can backfill what they missed.
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
pub struct AgentRun {
    /// Server-allocated task id.
    pub task_id: uuid::Uuid,
    /// Discriminator label (`"session"` / `"ai_stream"` / …) — the
    /// full `AgentTaskKind` payload is not echoed back to keep this
    /// reply small.
    pub kind: String,
    /// Priority the task was submitted with.
    pub priority: TaskPriority,
    /// Optional parent task id passed to `submit`.
    pub parent: Option<uuid::Uuid>,
    /// Caller plugin id captured at submission time.
    pub caller_plugin_id: String,
    /// Current run status.
    pub status: RunStatus,
    /// Submission timestamp (UTC).
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    /// Worker-start timestamp; `None` while queued.
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Terminal-state timestamp; `None` while running.
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Replay buffer of typed events for this run. Bounded; older
    /// events fall off when the buffer reaches
    /// [`PER_RUN_EVENT_BUFFER_CAP`].
    pub events: Vec<events::AiEvent>,
}

/// Compact `list` row — same fields as [`AgentRun`] minus the events
/// buffer (callers can pull the full record via `get` if they need
/// the replay).
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
pub struct AgentRunSummary {
    /// Server-allocated task id.
    pub task_id: uuid::Uuid,
    /// Discriminator label.
    pub kind: String,
    /// Priority bucket.
    pub priority: TaskPriority,
    /// Caller plugin id.
    pub caller_plugin_id: String,
    /// Current run status.
    pub status: RunStatus,
    /// Submission timestamp (UTC).
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    /// Terminal-state timestamp; `None` while running.
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Coarse-grained run lifecycle. Mapped from per-event transitions in
/// [`scheduler::Store::record_event`]. Failed/aborted runs carry the
/// error message in their last [`events::AiEvent::Failed`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Submitted, not yet picked up by a worker.
    Queued,
    /// Worker has begun executing.
    Running,
    /// Reserved for Phase 5 — worker is suspended pending `resume`.
    Paused,
    /// Reserved for Phase 5 — caller asked to cancel before completion.
    Cancelled,
    /// Worker finished and the underlying call returned `Ok`.
    Completed,
    /// Worker finished and the underlying call returned `Err`. The
    /// `error` string lives in the run's terminal `Failed` event.
    Failed,
}

/// `cancel` / `pause` / `resume` arg envelope. Phase 1 ships only the
/// shapes; Phase 5 wires the actual control plane.
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
pub struct AiRuntimeControlArgs {
    /// Target task id.
    pub task_id: uuid::Uuid,
    /// Optional human-readable reason — captured in the corresponding
    /// `Cancelled` / `Paused` event for audit traceability.
    #[serde(default)]
    pub reason: Option<String>,
}

/// `get` arg shape.
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
pub struct AiRuntimeGetArgs {
    /// Task to fetch.
    pub task_id: uuid::Uuid,
}

/// `list` arg envelope. All filters are optional; an empty body
/// returns every run currently in the store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AiRuntimeListArgs {
    /// Filter to a single status. `None` returns every status.
    #[serde(default)]
    pub status: Option<RunStatus>,
    /// Hard cap on returned rows. `None` returns every match.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Only return rows submitted at or after this timestamp.
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

/// `events` arg envelope.
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
pub struct AiRuntimeEventsArgs {
    /// Task whose replay buffer to drain.
    pub task_id: uuid::Uuid,
    /// Sequence number to start after — when omitted, the full
    /// bounded buffer is returned.
    #[serde(default)]
    pub since_seq: Option<u64>,
}

/// `wait_for` arg envelope — blocks the IPC reply until the named
/// task reaches a terminal status (Completed / Failed / Cancelled) or
/// the optional timeout elapses.
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
pub struct AiRuntimeWaitForArgs {
    /// Task to wait on.
    pub task_id: uuid::Uuid,
    /// Optional wall-clock timeout in milliseconds. When the run is
    /// still non-terminal at deadline, the handler returns the run's
    /// current snapshot with `status` unchanged (`Queued` / `Running` /
    /// `Paused`). `None` waits indefinitely — appropriate for callers
    /// (e.g. agent `delegate`) that already enforce their own ceiling.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// `wait_for` reply shape. Mirrors the existing `AgentRun` payload but
/// adds a `timed_out` discriminator so the caller doesn't have to
/// inspect `status` against the terminal set to figure out whether the
/// wait completed or expired. Failed-run details still live in the
/// embedded `AgentRun.events` ring.
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
pub struct AiRuntimeWaitForReply {
    /// Live run snapshot at the moment the wait completed (or timed
    /// out). When `timed_out` is `true`, `status` is one of the
    /// non-terminal variants.
    pub run: AgentRun,
    /// `true` when the wait expired before the run reached a terminal
    /// status; `false` when the run reached a terminal state.
    pub timed_out: bool,
}

/// `pool_stats` reply shape — exposed so a shell observability panel
/// can render queue depth + utilisation without scraping logs.
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
pub struct PoolStats {
    /// Number of worker threads in the dedicated tokio runtime.
    pub workers: u32,
    /// Total tasks currently in `Queued` status.
    pub queued: u32,
    /// Total tasks currently in `Running` status.
    pub running: u32,
    /// Configured max concurrent runs (currently equals `workers`).
    pub max: u32,
}

// ─── Bounded ring buffer ─────────────────────────────────────────────────────

/// Tiny FIFO ring used by [`AgentRun::events`]. Pure value type so
/// the scheduler can hold it under a `Mutex` without unsafe
/// shenanigans.
#[derive(Debug)]
pub(crate) struct EventRing {
    inner: Mutex<RingInner>,
}

#[derive(Debug)]
struct RingInner {
    seq: u64,
    queue: VecDeque<(u64, events::AiEvent)>,
}

impl EventRing {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(RingInner {
                seq: 0,
                queue: VecDeque::with_capacity(PER_RUN_EVENT_BUFFER_CAP),
            }),
        }
    }

    /// Append one event; trim the oldest entry if the cap is hit.
    /// Returns the assigned monotonic sequence number.
    pub(crate) fn push(&self, event: events::AiEvent) -> u64 {
        let mut g = self.inner.lock().expect("event ring poisoned");
        g.seq = g.seq.saturating_add(1);
        let seq = g.seq;
        if g.queue.len() == PER_RUN_EVENT_BUFFER_CAP {
            g.queue.pop_front();
        }
        g.queue.push_back((seq, event));
        seq
    }

    /// Snapshot of every retained event in chronological order.
    pub(crate) fn snapshot(&self) -> Vec<events::AiEvent> {
        let g = self.inner.lock().expect("event ring poisoned");
        g.queue.iter().map(|(_, e)| e.clone()).collect()
    }

    /// Snapshot of every retained event whose seq is strictly greater
    /// than `after`. Used by the `events` handler when the caller
    /// passes `since_seq`.
    pub(crate) fn snapshot_after(&self, after: u64) -> Vec<events::AiEvent> {
        let g = self.inner.lock().expect("event ring poisoned");
        g.queue
            .iter()
            .filter(|(seq, _)| *seq > after)
            .map(|(_, e)| e.clone())
            .collect()
    }
}

impl Default for EventRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience alias for the runtime's owned event-ring handle —
/// `Arc`-wrapped so worker tasks can record without holding the
/// scheduler's outer lock.
pub(crate) type SharedEventRing = Arc<EventRing>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_assigns_monotonic_seqs_and_caps_at_buffer_size() {
        let ring = EventRing::new();
        for i in 0..(PER_RUN_EVENT_BUFFER_CAP + 5) {
            ring.push(events::AiEvent::TokenChunk {
                task_id: uuid::Uuid::nil(),
                text: format!("chunk-{i}"),
            });
        }
        let snap = ring.snapshot();
        assert_eq!(snap.len(), PER_RUN_EVENT_BUFFER_CAP);
        // Oldest 5 should have been dropped — the first remaining text
        // is `chunk-5` (since we pushed 0..N+5).
        if let events::AiEvent::TokenChunk { text, .. } = &snap[0] {
            assert_eq!(text, "chunk-5");
        } else {
            panic!("expected TokenChunk");
        }
    }

    #[test]
    fn ring_snapshot_after_filters_by_seq() {
        let ring = EventRing::new();
        for i in 0..5 {
            ring.push(events::AiEvent::TokenChunk {
                task_id: uuid::Uuid::nil(),
                text: format!("c{i}"),
            });
        }
        let after_2 = ring.snapshot_after(2);
        assert_eq!(after_2.len(), 3);
        if let events::AiEvent::TokenChunk { text, .. } = &after_2[0] {
            assert_eq!(text, "c2");
        }
    }

    #[test]
    fn task_kind_label_is_stable() {
        assert_eq!(
            AgentTaskKind::Session {
                args: serde_json::Value::Null
            }
            .label(),
            "session"
        );
        assert_eq!(
            AgentTaskKind::AiStream {
                args: serde_json::Value::Null
            }
            .label(),
            "ai_stream"
        );
        assert_eq!(
            AgentTaskKind::WorkflowAiStep {
                target_plugin: "com.nexus.ai".into(),
                command: "ask".into(),
                args: serde_json::Value::Null,
                workflow: "x".into(),
                step: 0,
            }
            .label(),
            "workflow_ai_step"
        );
    }
}
