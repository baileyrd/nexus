//! DG-37 agent-to-agent delegation (HANDLER_DELEGATE).

use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::session::handle_session_run;
use super::shared::{exec_err, parse, PendingApprovals};

/// Args for `com.nexus.agent::delegate` (handler id 24).
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// Target archetype short name (one of the ids returned by
    /// `list_archetypes`).
    pub archetype: String,
    /// Natural-language goal for the sub-session.
    pub goal: String,
    /// Optional override for the sub-session's system prompt.
    #[serde(default)]
    pub system: Option<String>,
    /// Auto-approve the sub-session's rounds. Defaults to `true`.
    #[serde(default = "default_delegate_auto_approve")]
    pub auto_approve: bool,
    /// Approval-callback timeout when `auto_approve = false`.
    #[serde(default)]
    pub approval_timeout_secs: Option<u64>,
    /// Prompt for every round when `auto_approve = false`.
    #[serde(default)]
    pub strict_approval: bool,
}

const fn default_delegate_auto_approve() -> bool {
    true
}

pub(crate) async fn handle_delegate(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DelegateArgs = parse(args, "delegate")?;
    if a.archetype.trim().is_empty() {
        return Err(exec_err("delegate: `archetype` must be non-empty".into()));
    }
    if a.goal.trim().is_empty() {
        return Err(exec_err("delegate: `goal` must be non-empty".into()));
    }
    let session_args = serde_json::json!({
        "goal": a.goal,
        "archetype": a.archetype,
        "system": a.system,
        "auto_approve": a.auto_approve,
        "approval_timeout_secs": a.approval_timeout_secs,
        "strict_approval": a.strict_approval,
    });
    handle_session_run(ctx, pending_approvals, &session_args).await
}
