//! Session identity types shared across the kernel/plugin boundary.
//!
//! Defined in `nexus-plugin-api` (not in `nexus-ai-runtime`) so that
//! future `NexusEvent` variants and other leaf-crate types can reference
//! them without introducing a circular dependency.

use serde::{Deserialize, Serialize};

/// Classifies the driver behind a perceive-reason-act-observe session.
///
/// The kind determines the session's default budget tier, latency
/// target, and output destination. The supervisor enforces per-kind
/// concurrency limits via [`crate`]-level admission control.
///
/// Matches the taxonomy in the AI-native runtime design (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Driven by an explicit user prompt or action. Generous budget,
    /// low-latency target; result surfaces in the UI as a live
    /// transcript.
    #[default]
    UserDriven,
    /// Background maintenance work (wiki sync, index refresh). Strict
    /// budget, runs at background priority; writes results to memory
    /// rather than producing an interactive transcript.
    Ambient,
    /// Spawned by an external signal (file change, webhook, schedule).
    /// Short-lived, one-shot; deposits a proposal in the user's inbox
    /// on completion.
    SignalTriggered,
    /// Delegated sub-task from a parent session. Budget is an
    /// attenuated slice of the parent's remaining budget; result is
    /// returned to the parent, not surfaced directly.
    SubAgent,
}

/// Coarse outcome of a session that has reached a terminal state.
///
/// Used in session lifecycle bus events so observers can distinguish
/// clean completion from policy aborts and hard failures without
/// parsing the full agent transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/"
    )
)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SessionOutcome {
    /// The model signalled completion and the session ended cleanly.
    Completed,
    /// The session was aborted before the goal was reached — by policy
    /// (max rounds, approval timeout), by user request, or by budget
    /// exhaustion.
    Aborted {
        /// Human-readable description of why the session was aborted.
        reason: String,
    },
    /// An unrecoverable error terminated the session after the failure
    /// ladder was exhausted.
    Failed {
        /// Error description.
        error: String,
    },
}
