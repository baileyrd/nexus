//! Typed AI/agent lifecycle envelope republished on the kernel bus.
//!
//! Every existing `com.nexus.ai.stream_*` and `com.nexus.agent.*` topic
//! still flows under its original name — the runtime wraps each into
//! one of these typed variants and republishes under
//! `com.nexus.ai.runtime.<variant>`. Consumers that want the typed
//! cross-subsystem stream subscribe to `com.nexus.ai.runtime.*`;
//! consumers that only want raw token streams keep their existing
//! subscription. See ADR 0028 §"Event flow" for the diagram.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{RunStatus, TaskPriority};

/// Suffix appended to [`crate::BUS_TOPIC_PREFIX`] for one of the
/// typed variants. Centralised here so the republisher and any
/// shell-side filter cannot drift.
#[must_use]
pub const fn topic_suffix(event: &AiEvent) -> &'static str {
    match event {
        AiEvent::Submitted { .. } => "submitted",
        AiEvent::Started { .. } => "started",
        AiEvent::TokenChunk { .. } => "token_chunk",
        AiEvent::ToolCalled { .. } => "tool_called",
        AiEvent::ToolResult { .. } => "tool_result",
        AiEvent::RoundProposed { .. } => "round_proposed",
        AiEvent::RoundDecided { .. } => "round_decided",
        AiEvent::Paused { .. } => "paused",
        AiEvent::Resumed { .. } => "resumed",
        AiEvent::Cancelled { .. } => "cancelled",
        AiEvent::Finished { .. } => "finished",
        AiEvent::Failed { .. } => "failed",
    }
}

/// Build the full bus topic for a given event variant. Consumers that
/// only care about a single shape (e.g. a notifications router that
/// only wants `Failed`) can subscribe to the specific topic instead
/// of the prefix.
#[must_use]
pub fn topic_for(event: &AiEvent) -> String {
    format!("{}{}", crate::BUS_TOPIC_PREFIX, topic_suffix(event))
}

/// Typed AI/agent lifecycle event.
///
/// Every variant carries `task_id` so consumers can dispatch by run.
/// `tag` (serde) is `kind` to keep TS unions ergonomic on the shell
/// side; field shapes mirror the ADR-0028 proposed types but stay
/// JSON-friendly (no `&'static str` on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AiEvent {
    /// `submit` accepted the task; the worker has not started yet.
    Submitted {
        /// Server-allocated task id.
        task_id: uuid::Uuid,
        /// `AgentTaskKind::label()` snapshot.
        kind_label: String,
        /// Priority bucket the task was queued under.
        priority: TaskPriority,
    },
    /// Worker began executing this task.
    Started {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// 1-based attempt counter — Phase 1 always emits `1`; later
        /// phases bump on retry.
        attempt: u32,
    },
    /// Wraps `com.nexus.ai.stream_chunk`.
    TokenChunk {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// Raw text chunk emitted by the underlying stream.
        text: String,
    },
    /// Reserved — Phase 2 will populate when the agent executor emits
    /// per-tool-call breadcrumbs through the runtime.
    ToolCalled {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// LLM-allocated tool-use id.
        tool_use_id: String,
        /// Tool name (e.g. `read_file`).
        name: String,
        /// Truncated args preview — never the full payload.
        args_preview: String,
    },
    /// Reserved — Phase 2 will populate alongside `ToolCalled`.
    ToolResult {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// LLM-allocated tool-use id this is the response to.
        tool_use_id: String,
        /// `true` when the tool surfaced an error.
        is_error: bool,
        /// Truncated body preview.
        summary: String,
    },
    /// Wraps `com.nexus.agent.round_proposed` — agent waiting for
    /// approval to dispatch a round of tool calls.
    RoundProposed {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// 1-based round number.
        round: u32,
        /// Agent narration accompanying the round (truncated by
        /// agent producer; not re-truncated here).
        narration: String,
    },
    /// Wraps `com.nexus.agent.round_decided`.
    RoundDecided {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// 1-based round number.
        round: u32,
        /// `"approve"` / `"deny"` / `"timeout"` — string for forward
        /// compatibility with whatever decision kinds Phase 2 adds.
        decision_kind: String,
    },
    /// Reserved — Phase 5.
    Paused {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// Human-readable reason captured from `pause` / approval
        /// timeout / etc.
        reason: String,
    },
    /// Reserved — Phase 5.
    Resumed {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
    },
    /// Reserved — Phase 5.
    Cancelled {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// Source of the cancellation (`"caller"` / `"deadline"` / …).
        by: String,
    },
    /// Worker finished cleanly. The `outcome` JSON is the verbatim
    /// reply body the underlying handler returned.
    Finished {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// Reply body from the underlying IPC dispatch.
        #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
        outcome: serde_json::Value,
    },
    /// Worker finished with an error. `error` is the serialized
    /// `PluginError`; `retriable` is reserved for Phase 2 retry
    /// policy and Phase-1 always emits `false`.
    Failed {
        /// Task id this event belongs to.
        task_id: uuid::Uuid,
        /// Serialized error string.
        error: String,
        /// Whether the scheduler should retry. Phase 1: always
        /// `false` (no retry policy).
        retriable: bool,
    },
}

impl AiEvent {
    /// Stable suffix without allocating a full topic string.
    #[must_use]
    pub fn suffix(&self) -> &'static str {
        topic_suffix(self)
    }

    /// Borrow the task id this event applies to.
    #[must_use]
    pub fn task_id(&self) -> uuid::Uuid {
        match self {
            Self::Submitted { task_id, .. }
            | Self::Started { task_id, .. }
            | Self::TokenChunk { task_id, .. }
            | Self::ToolCalled { task_id, .. }
            | Self::ToolResult { task_id, .. }
            | Self::RoundProposed { task_id, .. }
            | Self::RoundDecided { task_id, .. }
            | Self::Paused { task_id, .. }
            | Self::Resumed { task_id, .. }
            | Self::Cancelled { task_id, .. }
            | Self::Finished { task_id, .. }
            | Self::Failed { task_id, .. } => *task_id,
        }
    }

    /// Map the event to the [`RunStatus`] transition it implies.
    /// Returns `None` for events that don't change the run status
    /// (e.g. token chunks, tool calls).
    #[must_use]
    pub fn implied_status(&self) -> Option<RunStatus> {
        match self {
            Self::Submitted { .. } => Some(RunStatus::Queued),
            // Started + Resumed both transition into the Running
            // bucket — share the arm so a future enum-variant addition
            // doesn't accidentally diverge the two.
            Self::Started { .. } | Self::Resumed { .. } => Some(RunStatus::Running),
            Self::Paused { .. } => Some(RunStatus::Paused),
            Self::Cancelled { .. } => Some(RunStatus::Cancelled),
            Self::Finished { .. } => Some(RunStatus::Completed),
            Self::Failed { .. } => Some(RunStatus::Failed),
            Self::TokenChunk { .. }
            | Self::ToolCalled { .. }
            | Self::ToolResult { .. }
            | Self::RoundProposed { .. }
            | Self::RoundDecided { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_for_each_variant_is_under_the_runtime_prefix() {
        let task_id = uuid::Uuid::nil();
        let cases = [
            AiEvent::Submitted {
                task_id,
                kind_label: "session".into(),
                priority: TaskPriority::Interactive,
            },
            AiEvent::Started {
                task_id,
                attempt: 1,
            },
            AiEvent::TokenChunk {
                task_id,
                text: "x".into(),
            },
            AiEvent::Finished {
                task_id,
                outcome: serde_json::json!({}),
            },
            AiEvent::Failed {
                task_id,
                error: "boom".into(),
                retriable: false,
            },
        ];
        for evt in cases {
            assert!(topic_for(&evt).starts_with(crate::BUS_TOPIC_PREFIX));
        }
    }

    #[test]
    fn implied_status_covers_every_lifecycle_transition() {
        let task_id = uuid::Uuid::nil();
        assert_eq!(
            AiEvent::Submitted {
                task_id,
                kind_label: "session".into(),
                priority: TaskPriority::Interactive,
            }
            .implied_status(),
            Some(RunStatus::Queued)
        );
        assert_eq!(
            AiEvent::Started {
                task_id,
                attempt: 1
            }
            .implied_status(),
            Some(RunStatus::Running)
        );
        assert_eq!(
            AiEvent::Finished {
                task_id,
                outcome: serde_json::Value::Null,
            }
            .implied_status(),
            Some(RunStatus::Completed)
        );
        assert_eq!(
            AiEvent::Failed {
                task_id,
                error: "x".into(),
                retriable: false,
            }
            .implied_status(),
            Some(RunStatus::Failed)
        );
        assert_eq!(
            AiEvent::TokenChunk {
                task_id,
                text: "x".into(),
            }
            .implied_status(),
            None
        );
    }
}
