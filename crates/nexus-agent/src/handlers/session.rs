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
    resolve_archetype_for_run, run_session_optionally_gated, AiChatBridge, BusBridgePolicy,
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

pub(crate) async fn handle_session_run(
    ctx: Arc<KernelPluginContext>,
    pending_approvals: Arc<PendingApprovals>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionRunArgs = parse_args(args, "session_run")?;

    let driver = AiChatBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    let dispatcher = KernelToolBridge {
        ctx: Arc::clone(&ctx),
        timeout: DEFAULT_TOOL_TIMEOUT,
    };

    let resolved = match &parsed.archetype {
        Some(name) => Some(resolve_archetype_for_run(&ctx, Some(name)).await),
        None => None,
    };

    let mut system = match (&parsed.system, resolved.as_ref()) {
        (Some(s), _) => s.clone(),
        (None, Some(r)) => r.system_prompt.clone(),
        (None, None) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };

    if parsed.system.is_none() {
        if let Some(slug) = parsed.archetype.as_deref() {
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
            archetype = parsed.archetype.as_deref().unwrap_or(""),
            "DG-36 follow-up: applying manifest tool allow/deny policy",
        );
    }

    let session_config = parsed.session_config.clone().unwrap_or_default();

    // BL-134 Phase 2b-ii — prefer the caller-supplied session id when
    // present (typically the `nexus-ai-runtime` worker). Falling back
    // to a fresh UUID preserves the legacy behaviour for every other
    // call site that doesn't opt in.
    let allocated_session_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let session = if parsed.auto_approve {
        run_session_optionally_gated(
            &driver,
            &dispatcher,
            crate::session::AutoApproveAll,
            manifest_policy.clone(),
            &parsed.goal,
            &system,
            parsed.archetype.clone(),
            allocated_session_id,
            session_config,
        )
        .await
    } else {
        let timeout = parsed
            .approval_timeout_secs
            .unwrap_or(DEFAULT_APPROVAL_TIMEOUT_SECS)
            .clamp(1, MAX_APPROVAL_TIMEOUT_SECS);
        let policy = BusBridgePolicy {
            session_id: allocated_session_id.clone(),
            ctx: Arc::clone(&ctx),
            pending: Arc::clone(&pending_approvals),
            timeout: Duration::from_secs(timeout),
            strict_approval: parsed.strict_approval,
        };
        let policy_session_id = policy.session_id.clone();
        let session = run_session_optionally_gated(
            &driver,
            &dispatcher,
            policy,
            manifest_policy.clone(),
            &parsed.goal,
            &system,
            parsed.archetype.clone(),
            policy_session_id,
            session_config,
        )
        .await;
        drop_pending(&pending_approvals, &session.id);
        session
    };

    record_session_memory(&ctx, &session).await;

    let path = session_path(&session.id)
        .ok_or_else(|| exec_err("session_run: refusing to write empty id".into()))?;
    let bytes = serde_json::to_vec_pretty(&session)
        .map_err(|e| exec_err(format!("session_run: encode session: {e}")))?;
    let path_str = path
        .to_str()
        .ok_or_else(|| exec_err("session_run: session path not UTF-8".into()))?;
    tracing::info!(
        session_id = %session.id,
        path = %path_str,
        bytes = bytes.len(),
        "session_run: persisting transcript"
    );
    ctx.ipc_call(
        "com.nexus.storage",
        "write_vault_file",
        serde_json::json!({ "path": path_str, "bytes": bytes }),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| {
        tracing::warn!(session_id = %session.id, error = %e, "session_run: persist failed");
        exec_err(format!("session_run: persist: {e}"))
    })?;
    tracing::info!(session_id = %session.id, "session_run: persisted ok");

    // BL-133 follow-up — emit `com.nexus.agent.session_completed` so
    // the auto-notify subscriber can dispatch a `notifications::send`
    // when the run exceeded the configured threshold. Best-effort:
    // failure here must not break the IPC reply.
    let duration_ms =
        crate::auto_notify::duration_ms_between(&session.started_at, &session.ended_at)
            .unwrap_or(0);
    let outcome_str = match serde_json::to_value(&session.outcome) {
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
        tracing::debug!(error = %e, "session_run: publish session_completed failed");
    }

    serde_json::to_value(&session).map_err(|e| exec_err(format!("session_run: encode reply: {e}")))
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
    let path = session_path(&a.id)
        .ok_or_else(|| exec_err(format!("session_get: invalid id '{}'", a.id)))?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("session_get: {e}")))?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| exec_err(format!("session_get: invalid JSON on disk: {e}")))
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
