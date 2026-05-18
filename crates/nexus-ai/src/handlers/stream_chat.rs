//! `stream_chat` IPC handler — direct chat with per-token bus events,
//! including the `mode=complete` ghost-completion path.

use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::activity_log::ActivityRecorder;
use crate::config::AiConfig;
use crate::handlers::shared::{
    build_ai_provider, compose_chat_system, exec_err, filter_to_read_only, ipc_messages_to_chat,
    last_user_prompt, record_activity_error, resolve_surface, run_complete, run_tool_dispatch_loop,
    EngineEnvelope, ToolDispatchOutcome,
};
use crate::ipc::{AiStreamChatArgs, AiStreamChatMode, AiToolPolicy};
use crate::tools::ToolRegistry;
use nexus_types::activity::{ActivityEntry, ActivityOutcome};

#[allow(clippy::too_many_lines, reason = "BL-037 records on every exit path; flow stays linear")]
pub(crate) async fn handle_stream_chat(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    tools: Option<Arc<ToolRegistry>>,
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    // Decode through the typed `AiStreamChatArgs`. The wire shape matches
    // the historical ad-hoc `{ messages, system, session_id }` shape
    // exactly (same field names, same `lowercase` role tags), so existing
    // chat callers keep working without modification — only the new
    // optional fields (`mode`, `tools`, `max_tokens`, `stop`, `trim`,
    // `surface`) change behaviour for BL-010 / BL-011 / BL-034 / BL-037
    // callers.
    let parsed: AiStreamChatArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("stream_chat: args decode: {e}")))?;
    let messages = ipc_messages_to_chat(&parsed.messages);
    let system = parsed.system.clone();
    let session_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let started_at = std::time::Instant::now();
    let prompt_text = last_user_prompt(&parsed.messages);
    let mode = parsed.mode.unwrap_or_default();
    let surface = resolve_surface(parsed.surface.as_deref(), mode);
    let provider_label = ai_cfg.as_ref().map(|c| c.provider.clone());
    let model_label = ai_cfg.as_ref().and_then(|c| c.model.clone());

    let Some(ai_cfg) = ai_cfg else {
        let err = "stream_chat: no AI chat provider configured";
        record_activity_error(
            activity.as_ref(),
            &session_id,
            surface,
            provider_label.clone(),
            model_label.clone(),
            prompt_text.clone(),
            started_at,
            err,
        )
        .await;
        return Err(exec_err(err.to_string()));
    };

    // mode=complete forces tools=none regardless of the caller's value
    // — the contract is "single round-trip, no side effects".
    let tool_policy = match mode {
        AiStreamChatMode::Complete => AiToolPolicy::None,
        AiStreamChatMode::Chat => parsed.tools.unwrap_or_default(),
    };

    let envelope = EngineEnvelope::new(Arc::clone(&ctx), session_id.clone());
    envelope.publish_start();

    let outcome = match mode {
        AiStreamChatMode::Chat => {
            let registry = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            let registry_for_loop: Arc<ToolRegistry> = match tool_policy {
                AiToolPolicy::Auto => registry,
                AiToolPolicy::None => Arc::new(ToolRegistry::new()),
                AiToolPolicy::AutoWithMcp => {
                    crate::tools::discover_mcp_tools(Arc::clone(&ctx), registry).await
                }
                AiToolPolicy::AutoReadOnly => Arc::new(filter_to_read_only(&registry)),
            };
            let on_chunk = envelope.chunk_sink();
            let ai = match build_ai_provider(&ai_cfg) {
                Ok(p) => p,
                Err(e) => {
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &e,
                    )
                    .await;
                    return Err(exec_err(e));
                }
            };
            let effective_system = compose_chat_system(system.as_deref());
            match run_tool_dispatch_loop(
                ai.as_ref(),
                registry_for_loop.as_ref(),
                messages,
                Some(effective_system.as_str()),
                &on_chunk,
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    let msg = format!("stream_chat: {e}");
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &msg,
                    )
                    .await;
                    return Err(exec_err(msg));
                }
            }
        }
        AiStreamChatMode::Complete => {
            let ai = match build_ai_provider(&ai_cfg) {
                Ok(p) => p,
                Err(e) => {
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &e,
                    )
                    .await;
                    return Err(exec_err(e));
                }
            };
            let on_chunk = envelope.chunk_sink();
            let text = match run_complete(
                ai.as_ref(),
                &messages,
                system.as_deref(),
                &parsed,
                &on_chunk,
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    let msg = format!("stream_chat: {e}");
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &msg,
                    )
                    .await;
                    return Err(exec_err(msg));
                }
            };
            ToolDispatchOutcome {
                text,
                tool_calls: Vec::new(),
                files: Vec::new(),
            }
        }
    };

    envelope.publish_done(&outcome.text);

    if let Some(rec) = activity {
        let entry = ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            surface,
            origin: "ai".into(),
            provider: provider_label,
            model: model_label,
            prompt: prompt_text,
            files: outcome.files.clone(),
            tool_calls: outcome.tool_calls.clone(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: u64::try_from(started_at.elapsed().as_millis()).ok(),
        };
        rec.append(entry).await;
    }

    Ok(serde_json::json!({"session_id": session_id, "text": outcome.text}))
}
