//! Session lifecycle handlers (ADR 0024 Phase 2a).
//!
//! Hosts `session_run`, `session_list`, `session_get`, `session_delete`
//! as well as the on-disk session path helper used by the round/decide
//! reply path.

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Events as _, FileSystem as _, Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::Deserialize;

use crate::DEFAULT_SYSTEM_PROMPT;

use super::shared::{
    compose_memory_preamble, drop_pending, exec_err, now_unix_ms, parse_args,
    resolve_archetype_for_run, run_session_optionally_gated_resumed, AiChatBridge, BusBridgePolicy,
    KernelToolBridge, PendingApprovals, DEFAULT_APPROVAL_TIMEOUT_SECS, DEFAULT_CHAT_TIMEOUT,
    DEFAULT_TOOL_TIMEOUT, MAX_APPROVAL_TIMEOUT_SECS, PLUGIN_ID,
};

pub(crate) const SESSION_DIR: &str = ".forge/agent/sessions";

#[derive(Debug, Deserialize)]
struct SessionRunArgs {
    goal: String,
    #[serde(default)]
    archetype: Option<String>,
    #[serde(default)]
    system: Option<String>,
    #[serde(default)]
    auto_approve: bool,
    #[serde(default)]
    approval_timeout_secs: Option<u64>,
    #[serde(default)]
    strict_approval: bool,
    #[serde(default)]
    session_config: Option<crate::session::SessionConfig>,
    /// BL-134 Phase 2b-ii — caller-supplied session id. When `Some`,
    /// the handler uses this id instead of allocating its own
    /// `Uuid::new_v4()` so the caller (today: `nexus-ai-runtime`)
    /// can correlate mid-flight bus events (`stream_chunk`,
    /// `round_proposed`) back to a runtime `task_id`. `None`
    /// preserves the legacy self-allocate behaviour — every existing
    /// non-runtime caller (CLI agent run, shell, MCP) leaves it
    /// unset and gets the same UUID-per-run shape as before.
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionIdArgs {
    id: String,
}

/// Args for `session_resume` (RFC 0008) — continue an existing session with a
/// new user message, forking a child at the parent's tip.
#[derive(Debug, Deserialize)]
struct SessionResumeArgs {
    /// The session to resume (its id).
    session_id: String,
    /// The new user message that drives the continued run.
    message: String,
    #[serde(default)]
    auto_approve: bool,
    #[serde(default)]
    approval_timeout_secs: Option<u64>,
    #[serde(default)]
    strict_approval: bool,
    #[serde(default)]
    session_config: Option<crate::session::SessionConfig>,
}

/// DG-33 follow-up — auto-record the completed session into the
/// agent's `history.jsonl`.
async fn record_session_memory(ctx: &KernelPluginContext, session: &crate::session::AgentSession) {
    let Some(archetype) = session.archetype.as_deref() else {
        return;
    };
    let agent_id = archetype.trim();
    if agent_id.is_empty() {
        return;
    }
    if let Err(e) = crate::memory::normalize_agent_id(agent_id) {
        tracing::warn!(
            plugin_id = PLUGIN_ID,
            archetype = agent_id,
            error = %e,
            "DG-33 auto-record: rejecting invalid agent id; skipping",
        );
        return;
    }
    let now = now_unix_ms();
    let events = crate::memory::events_from_session(session, now);
    if events.is_empty() {
        return;
    }
    let path = crate::memory::history_path(agent_id);
    let appended = match crate::memory::serialize_entries_jsonl(&events, &path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                plugin_id = PLUGIN_ID,
                error = %e,
                "DG-33 auto-record: serialize failed; skipping",
            );
            return;
        }
    };
    let existing = ctx.read_file(&path).await.unwrap_or_default();
    let mut combined = existing;
    combined.extend_from_slice(&appended);
    if let Err(e) = ctx.write_file(&path, &combined).await {
        tracing::warn!(
            plugin_id = PLUGIN_ID,
            path = %path.display(),
            error = %e,
            "DG-33 auto-record: write failed; not blocking session result",
        );
        return;
    }
    tracing::debug!(
        plugin_id = PLUGIN_ID,
        agent_id,
        entry_count = events.len(),
        "DG-33 auto-record: appended session events to history.jsonl",
    );
}

/// RFC 0008 — parameters for the shared run/persist core, covering both a fresh
/// `session_run` and a resumed/forked run (`session_resume`).
struct SessionRunRequest {
    goal: String,
    archetype: Option<String>,
    system: Option<String>,
    auto_approve: bool,
    approval_timeout_secs: Option<u64>,
    strict_approval: bool,
    session_config: crate::session::SessionConfig,
    session_id: String,
    seed_rounds: Vec<crate::RoundRecord>,
    follow_up: Option<String>,
    parent_id: Option<String>,
    branch_point: Option<u32>,
}

pub(crate) async fn handle_session_run(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionRunArgs = parse_args(args, "session_run")?;
    // BL-134 Phase 2b-ii — prefer the caller-supplied session id (the
    // ai-runtime worker correlating bus events); else self-allocate.
    let session_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let req = SessionRunRequest {
        goal: parsed.goal,
        archetype: parsed.archetype,
        system: parsed.system,
        auto_approve: parsed.auto_approve,
        approval_timeout_secs: parsed.approval_timeout_secs,
        strict_approval: parsed.strict_approval,
        session_config: parsed.session_config.unwrap_or_default(),
        session_id,
        seed_rounds: Vec::new(),
        follow_up: None,
        parent_id: None,
        branch_point: None,
    };
    run_and_persist_session(ctx, pending_approvals, req).await
}

/// RFC 0008 (Phase 5.4) — resume a session: fork a child at the parent's tip,
/// seeded with the parent's full (assembled) transcript plus a new user
/// `message`, then run + persist it as a delta node linked to the parent.
pub(crate) async fn handle_session_resume(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionResumeArgs = parse_args(args, "session_resume")?;
    if parsed.message.trim().is_empty() {
        return Err(exec_err("session_resume: `message` must be non-empty".into()));
    }
    let parent = assemble_session(&ctx, parsed.session_id.clone(), 0).await?;
    let branch_point = parent.rounds.last().map_or(0, |r| r.round);
    let req = SessionRunRequest {
        goal: parent.goal,
        archetype: parent.archetype,
        // The system prompt isn't persisted; re-resolve it from the archetype.
        system: None,
        auto_approve: parsed.auto_approve,
        approval_timeout_secs: parsed.approval_timeout_secs,
        strict_approval: parsed.strict_approval,
        session_config: parsed.session_config.unwrap_or_default(),
        session_id: uuid::Uuid::new_v4().to_string(),
        seed_rounds: parent.rounds,
        follow_up: Some(parsed.message),
        parent_id: Some(parsed.session_id),
        branch_point: Some(branch_point),
    };
    run_and_persist_session(ctx, pending_approvals, req).await
}

/// Shared core: build the driver/dispatcher, resolve the system prompt + manifest
/// policy, run the (possibly seeded) loop under the right approval policy, then
/// persist the node (a fork stores only its new rounds), record memory, and
/// publish completion. Returns the **assembled** (full) transcript.
async fn run_and_persist_session(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    req: SessionRunRequest,
) -> Result<serde_json::Value, PluginError> {
    let driver = AiChatBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    // Wrap the kernel dispatcher so the session-local `todo` tool is handled
    // inline (ephemeral, per-session) and every other call passes through.
    let dispatcher = crate::todo::TodoDispatcher::new(KernelToolBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_TOOL_TIMEOUT,
    });

    let resolved = match &req.archetype {
        Some(name) => Some(resolve_archetype_for_run(&ctx, Some(name)).await),
        None => None,
    };
    let mut system = match (&req.system, resolved.as_ref()) {
        (Some(s), _) => s.clone(),
        (None, Some(r)) => r.system_prompt.clone(),
        (None, None) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };
    if req.system.is_none() {
        if let Some(slug) = req.archetype.as_deref() {
            if let Some(preamble) = compose_memory_preamble(&ctx, slug).await {
                system.push_str("\n\n");
                system.push_str(&preamble);
            }
        }
    }

    let manifest_policy = resolved
        .as_ref()
        .and_then(|r| r.manifest.as_ref())
        .map(crate::ManifestToolPolicy::from_manifest)
        .filter(|p| !p.is_noop());
    if manifest_policy.is_some() {
        tracing::debug!(
            plugin_id = PLUGIN_ID,
            archetype = req.archetype.as_deref().unwrap_or(""),
            "DG-36 follow-up: applying manifest tool allow/deny policy",
        );
    }

    let mut session = if req.auto_approve {
        run_session_optionally_gated_resumed(
            &driver,
            &dispatcher,
            crate::session::AutoApproveAll,
            manifest_policy,
            &req.goal,
            &system,
            req.archetype.clone(),
            req.session_id.clone(),
            req.session_config,
            req.seed_rounds,
            req.follow_up,
        )
        .await
    } else {
        let timeout = req
            .approval_timeout_secs
            .unwrap_or(DEFAULT_APPROVAL_TIMEOUT_SECS)
            .clamp(1, MAX_APPROVAL_TIMEOUT_SECS);
        let policy = BusBridgePolicy {
            session_id: req.session_id.clone(),
            ctx: Arc::clone(&ctx),
            pending: Arc::clone(&pending_approvals),
            timeout: Duration::from_secs(timeout),
            strict_approval: req.strict_approval,
        };
        let session = run_session_optionally_gated_resumed(
            &driver,
            &dispatcher,
            policy,
            manifest_policy,
            &req.goal,
            &system,
            req.archetype.clone(),
            req.session_id.clone(),
            req.session_config,
            req.seed_rounds,
            req.follow_up,
        )
        .await;
        drop_pending(&pending_approvals, &session.id);
        session
    };

    // RFC 0008 — stamp the fork linkage on the assembled in-memory session.
    session.parent_id = req.parent_id;
    session.branch_point = req.branch_point;

    // A forked node persists only its OWN new rounds (delta); a root stores all.
    let node = if session.branch_point.is_some() {
        let mut n = session.clone();
        n.rounds = delta_rounds(&session.rounds, session.branch_point);
        n
    } else {
        session.clone()
    };

    record_session_memory(&ctx, &node).await;
    persist_session_node(&ctx, &node).await?;
    publish_session_completed(&ctx, &session);

    serde_json::to_value(&session).map_err(|e| exec_err(format!("session: encode reply: {e}")))
}

/// Write a session node to `<forge>/.forge/agent/sessions/<id>.json`.
async fn persist_session_node(
    ctx: &KernelPluginContext,
    node: &crate::session::AgentSession,
) -> Result<(), PluginError> {
    let path = session_path(&node.id)
        .ok_or_else(|| exec_err("session: refusing to write empty id".into()))?;
    let bytes = serde_json::to_vec_pretty(node)
        .map_err(|e| exec_err(format!("session: encode node: {e}")))?;
    let path_str = path
        .to_str()
        .ok_or_else(|| exec_err("session: path not UTF-8".into()))?;
    tracing::info!(
        session_id = %node.id,
        path = %path_str,
        bytes = bytes.len(),
        rounds = node.rounds.len(),
        "session: persisting transcript node"
    );
    ctx.ipc_call(
        "com.nexus.storage",
        "write_vault_file",
        serde_json::json!({ "path": path_str, "bytes": bytes }),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| {
        tracing::warn!(session_id = %node.id, error = %e, "session: persist failed");
        exec_err(format!("session: persist: {e}"))
    })?;
    Ok(())
}

/// Best-effort `com.nexus.agent.session_completed` publish for the auto-notify
/// subscriber. Uses the full (assembled) session for duration + metadata.
fn publish_session_completed(ctx: &KernelPluginContext, session: &crate::session::AgentSession) {
    let duration_ms =
        crate::auto_notify::duration_ms_between(&session.started_at, &session.ended_at)
            .unwrap_or(0);
    let outcome_str = match serde_json::to_value(session.outcome) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "completed".to_string(),
    };
    if let Err(e) = ctx.publish(
        crate::auto_notify::SESSION_COMPLETED_TOPIC,
        serde_json::json!({
            "session_id": session.id,
            "duration_ms": duration_ms,
            "outcome": outcome_str,
            "archetype": session.archetype,
            "goal": session.goal,
        }),
    ) {
        tracing::debug!(error = %e, "session: publish session_completed failed");
    }
}

pub(crate) async fn handle_session_list(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let response = match ctx
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            serde_json::json!({ "relpath": SESSION_DIR }),
            Duration::from_secs(5),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::info!(error = %e, dir = SESSION_DIR, "session_list: list_dir errored, reporting empty");
            return Ok(serde_json::json!([]));
        }
    };

    let Some(arr) = response.as_array() else {
        tracing::warn!(
            dir = SESSION_DIR,
            response = %response,
            "session_list: list_dir reply was not a JSON array"
        );
        return Ok(serde_json::json!([]));
    };
    let mut summaries: Vec<serde_json::Value> = Vec::new();
    for entry in arr {
        let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue;
        }
        let id = name.trim_end_matches(".json").to_string();
        let Some(path) = session_path(&id) else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let Ok(session) = serde_json::from_slice::<crate::AgentSession>(&bytes) else {
            continue;
        };
        summaries.push(serde_json::json!({
            "id": session.id,
            "goal": session.goal,
            "started_at": session.started_at,
            "ended_at": session.ended_at,
            "outcome": session.outcome,
            // RFC 0008 — tree linkage so a UI can render the forest.
            "parent_id": session.parent_id,
            "branch_point": session.branch_point,
        }));
    }
    summaries.sort_by(|a, b| {
        b.get("started_at")
            .and_then(serde_json::Value::as_str)
            .cmp(&a.get("started_at").and_then(serde_json::Value::as_str))
    });
    Ok(serde_json::Value::Array(summaries))
}

pub(crate) async fn handle_session_get(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: SessionIdArgs = parse_args(args, "session_get")?;
    // RFC 0008 — return the ASSEMBLED transcript: a forked node stores only its
    // own rounds, so walk the parent chain and concatenate inherited prefixes.
    let session = assemble_session(&ctx, a.id, 0).await?;
    serde_json::to_value(&session).map_err(|e| exec_err(format!("session_get: encode: {e}")))
}

/// Maximum parent-chain depth [`assemble_session`] walks before giving up
/// (guards against a corrupt `parent_id` cycle).
const MAX_SESSION_CHAIN: usize = 256;

/// Load a single session node from disk (no parent walking).
async fn load_session_node(
    ctx: &KernelPluginContext,
    id: &str,
) -> Result<crate::session::AgentSession, PluginError> {
    let path = session_path(id).ok_or_else(|| exec_err(format!("session: invalid id '{id}'")))?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("session: read '{id}': {e}")))?;
    serde_json::from_slice::<crate::session::AgentSession>(&bytes)
        .map_err(|e| exec_err(format!("session: invalid JSON for '{id}': {e}")))
}

/// Assemble a node's full transcript by walking its parent chain (RFC 0008): a
/// root node returns as-is; a forked node prepends `parent.rounds[..=bp]`.
/// Boxed so the `async` recursion compiles.
fn assemble_session(
    ctx: &KernelPluginContext,
    id: String,
    depth: usize,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<crate::session::AgentSession, PluginError>>
            + Send
            + '_,
    >,
> {
    Box::pin(async move {
        if depth > MAX_SESSION_CHAIN {
            return Err(exec_err(format!(
                "session: parent chain for '{id}' exceeds {MAX_SESSION_CHAIN} (cycle?)"
            )));
        }
        let mut node = load_session_node(ctx, &id).await?;
        if let (Some(pid), Some(bp)) = (node.parent_id.clone(), node.branch_point) {
            let parent = assemble_session(ctx, pid, depth + 1).await?;
            node.rounds = assemble_rounds(&parent.rounds, bp, std::mem::take(&mut node.rounds));
        }
        Ok(node)
    })
}

/// Inherited prefix (`parent_rounds` with `round <= branch_point`) followed by
/// the node's own rounds. Pure.
fn assemble_rounds(
    parent_rounds: &[crate::RoundRecord],
    branch_point: u32,
    mut node_rounds: Vec<crate::RoundRecord>,
) -> Vec<crate::RoundRecord> {
    let mut out: Vec<crate::RoundRecord> = parent_rounds
        .iter()
        .filter(|r| r.round <= branch_point)
        .cloned()
        .collect();
    out.append(&mut node_rounds);
    out
}

/// The rounds a node persists: a fork stores only rounds after its
/// `branch_point`; a root (`None`) stores all. Pure.
fn delta_rounds(
    full: &[crate::RoundRecord],
    branch_point: Option<u32>,
) -> Vec<crate::RoundRecord> {
    match branch_point {
        Some(bp) => full.iter().filter(|r| r.round > bp).cloned().collect(),
        None => full.to_vec(),
    }
}

pub(crate) async fn handle_session_delete(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: SessionIdArgs = parse_args(args, "session_delete")?;
    let path = session_path(&a.id)
        .ok_or_else(|| exec_err(format!("session_delete: invalid id '{}'", a.id)))?;
    ctx.delete_file(&path)
        .await
        .map_err(|e| exec_err(format!("session_delete: {e}")))?;
    Ok(serde_json::json!({ "deleted": true, "id": a.id }))
}

/// Resolve a session id to its on-disk path. Validates the id is
/// non-empty and contains only `[a-zA-Z0-9-]` so a maliciously
/// shaped id can't path-traverse out of the sessions directory.
pub(crate) fn session_path(id: &str) -> Option<std::path::PathBuf> {
    if id.is_empty() {
        return None;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(std::path::PathBuf::from(format!("{SESSION_DIR}/{id}.json")))
}

#[cfg(test)]
mod tests {
    use super::{assemble_rounds, delta_rounds};
    use crate::RoundRecord;

    fn round(n: u32) -> RoundRecord {
        RoundRecord {
            round: n,
            text: format!("r{n}"),
            tool_calls: Vec::new(),
        }
    }

    fn rounds_of(rs: &[RoundRecord]) -> Vec<u32> {
        rs.iter().map(|r| r.round).collect()
    }

    #[test]
    fn delta_rounds_root_keeps_all() {
        let full = vec![round(1), round(2), round(3)];
        assert_eq!(rounds_of(&delta_rounds(&full, None)), vec![1, 2, 3]);
    }

    #[test]
    fn delta_rounds_fork_keeps_only_new() {
        let full = vec![round(1), round(2), round(3), round(4)];
        assert_eq!(rounds_of(&delta_rounds(&full, Some(2))), vec![3, 4]);
    }

    #[test]
    fn assemble_rounds_concatenates_inherited_prefix_and_node() {
        // Parent has rounds 1..3; forking at bp=2 inherits only 1,2 (parent's
        // round 3 is past the fork point), then appends the node's own rounds.
        let parent = vec![round(1), round(2), round(3)];
        let node = vec![round(3), round(4)];
        assert_eq!(rounds_of(&assemble_rounds(&parent, 2, node)), vec![1, 2, 3, 4]);
    }

    #[test]
    fn assemble_then_delta_is_a_round_trip() {
        // A node forked at bp=2 stores its delta [3, 4]; assembling against the
        // parent prefix recovers the full [1, 2, 3, 4], and taking the delta of
        // that recovers the node's [3, 4].
        let parent_full = vec![round(1), round(2), round(3)];
        let node_delta = vec![round(3), round(4)];
        let assembled = assemble_rounds(&parent_full, 2, node_delta);
        assert_eq!(rounds_of(&assembled), vec![1, 2, 3, 4]);
        assert_eq!(rounds_of(&delta_rounds(&assembled, Some(2))), vec![3, 4]);
    }
}
