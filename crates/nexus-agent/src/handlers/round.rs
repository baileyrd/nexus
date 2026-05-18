//! `com.nexus.agent::round_decide` (HANDLER_ROUND_DECIDE).
//!
//! Phase 2b bus-bridge reply path — the caller pushes a
//! [`crate::RoundDecision`]-shaped reply for a pending session round.

use std::sync::Arc;

use nexus_plugins::PluginError;
use serde::Deserialize;

use super::shared::{exec_err, parse_args, PendingApprovals};

/// Wire shape of `com.nexus.agent::round_decide` args.
#[derive(Debug, Deserialize)]
pub(crate) struct RoundDecideArgs {
    session_id: String,
    #[serde(flatten)]
    decision: RoundDecideKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RoundDecideKind {
    ApproveAll,
    Abort {
        #[serde(default)]
        reason: String,
    },
    Partial {
        entries: Vec<crate::RoundDecisionEntry>,
    },
}

impl From<RoundDecideKind> for crate::RoundDecision {
    fn from(k: RoundDecideKind) -> Self {
        match k {
            RoundDecideKind::ApproveAll => crate::RoundDecision::ApproveAll,
            RoundDecideKind::Abort { reason } => crate::RoundDecision::Abort(reason),
            RoundDecideKind::Partial { entries } => crate::RoundDecision::Partial(entries),
        }
    }
}

pub(crate) async fn handle_round_decide(
    pending: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: RoundDecideArgs = parse_args(args, "round_decide")?;
    let entry = {
        let mut map = pending
            .lock()
            .map_err(|e| exec_err(format!("round_decide: pending lock poisoned: {e}")))?;
        map.remove(&parsed.session_id)
    };
    let Some(entry) = entry else {
        return Err(exec_err(format!(
            "round_decide: no pending approval for session '{}'",
            parsed.session_id
        )));
    };
    if entry.tx.send(parsed.decision.into()).is_err() {
        return Err(exec_err(format!(
            "round_decide: session '{}' is no longer awaiting a decision",
            parsed.session_id
        )));
    }
    Ok(serde_json::json!({ "delivered": true, "session_id": parsed.session_id }))
}
