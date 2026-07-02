//! `stream_ask` IPC handler — RAG retrieve + streaming chat with
//! per-token bus events.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nexus_kernel::{Events as _, KernelPluginContext};
use nexus_plugins::PluginError;

use crate::activity_log::ActivityRecorder;
use crate::config::AiConfig;
use crate::handlers::shared::{
    build_ai_provider, build_embedding_provider, exec_err, record_activity_error,
};
use nexus_types::activity::{ActivityEntry, ActivityOutcome, ActivitySurface};

// Same shape as handle_stream_chat — every error path records an
// `ActivityEntry` so the timeline reflects retrieval failures, embed
// failures, etc. alongside successes.
#[allow(
    clippy::too_many_lines,
    reason = "BL-037 records on every exit path; flow stays linear"
)]
pub(crate) async fn handle_stream_ask(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let started_at = std::time::Instant::now();
    let messages: Vec<crate::provider::ChatMessage> = args
        .get("messages")
        .ok_or_else(|| exec_err("stream_ask: missing 'messages'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("stream_ask: messages decode: {e}")))
        })?;
    let session_id = args
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);
    let question = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::provider::Role::User))
        .map(|m| m.content.clone())
        .ok_or_else(|| exec_err("stream_ask: no user message in 'messages'".to_string()))?;

    let provider_label = ai_cfg.as_ref().map(|c| c.provider.clone());
    let model_label = ai_cfg.as_ref().and_then(|c| c.model.clone());

    let record_err = |err: String| {
        let rec = activity.clone();
        let session_id = session_id.clone();
        let provider_label = provider_label.clone();
        let model_label = model_label.clone();
        let prompt = question.clone();
        async move {
            record_activity_error(
                rec.as_ref(),
                &session_id,
                ActivitySurface::Ask,
                provider_label,
                model_label,
                prompt,
                started_at,
                &err,
            )
            .await;
        }
    };

    let Some(ai_cfg) = ai_cfg else {
        record_err("stream_ask: no AI chat provider configured".into()).await;
        return Err(exec_err(
            "stream_ask: no AI chat provider configured".to_string(),
        ));
    };
    let Some(embed_cfg) = embed_cfg else {
        record_err("stream_ask: no embedding provider configured".into()).await;
        return Err(exec_err(
            "stream_ask: no embedding provider configured".to_string(),
        ));
    };
    let ai = match build_ai_provider(&ai_cfg) {
        Ok(p) => p,
        Err(e) => {
            record_err(e.clone()).await;
            return Err(exec_err(e));
        }
    };
    // C26 (#379) — register + install the cooperative cancel flag; the
    // guard sweeps the registry entry on every exit path.
    let cancel_flag = crate::cancel::register(&session_id);
    let _cancel_guard = crate::cancel::CancelGuard(session_id.clone());
    ai.install_cancel_flag(std::sync::Arc::clone(&cancel_flag));
    let embedder = match build_embedding_provider(&embed_cfg) {
        Ok(p) => p,
        Err(e) => {
            record_err(e.clone()).await;
            return Err(exec_err(e));
        }
    };

    let sources = match crate::rag::retrieve(&ctx, embedder.as_ref(), &question, limit).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("stream_ask: retrieve: {e}");
            record_err(msg.clone()).await;
            return Err(exec_err(msg));
        }
    };
    let system = crate::rag::build_rag_prompt(&sources);

    let _ = ctx.publish(
        "com.nexus.ai.stream_start",
        serde_json::json!({
            "session_id": &session_id,
            "sources": &sources,
        }),
    );

    let ctx_chunk = Arc::clone(&ctx);
    let sid_chunk = session_id.clone();
    let chunk_idx = Arc::new(AtomicUsize::new(0));
    let on_chunk = {
        let chunk_idx = Arc::clone(&chunk_idx);
        move |chunk: String| {
            let idx = chunk_idx.fetch_add(1, Ordering::Relaxed);
            let _ = ctx_chunk.publish(
                "com.nexus.ai.stream_chunk",
                serde_json::json!({
                    "session_id": &sid_chunk,
                    "chunk": chunk,
                    "index": idx,
                }),
            );
        }
    };

    let text = match ai
        .chat_stream_with(&messages, Some(&system), &on_chunk)
        .await
    {
        Ok(t) => t,
        Err(crate::error::AiError::Cancelled) => {
            // C26 (#379) — user Stop: the chunks already emitted stand
            // as the partial answer; signal termination so the chat
            // surface stops its spinner.
            let _ = ctx.publish(
                "com.nexus.ai.stream_done",
                serde_json::json!({
                    "session_id": &session_id,
                    "text": "",
                    "cancelled": true,
                }),
            );
            return Ok(serde_json::json!({
                "session_id": session_id, "cancelled": true
            }));
        }
        Err(e) => {
            let msg = format!("stream_ask: {e}");
            record_err(msg.clone()).await;
            return Err(exec_err(msg));
        }
    };

    // BL-038: enrich sources with line ranges + 1-based numbering so the
    // shell can render `[N]` markers in the answer as clickable chips.
    let citations = crate::rag::build_citations(&ctx, &sources, &text).await;

    // C27 (#380) — provider-reported usage rides along when available.
    let mut done_payload = serde_json::json!({
        "session_id": &session_id,
        "text": &text,
        "sources": &sources,
        "citations": &citations,
    });
    if let Some(u) = ai.take_usage() {
        done_payload["usage"] = serde_json::json!({
            "input_tokens": u.input_tokens,
            "output_tokens": u.output_tokens,
        });
    }
    let _ = ctx.publish("com.nexus.ai.stream_done", done_payload);

    if let Some(rec) = activity {
        let mut files: Vec<String> = Vec::new();
        for s in &sources {
            if !files.contains(&s.file_path) {
                files.push(s.file_path.clone());
            }
        }
        let entry = ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            surface: ActivitySurface::Ask,
            origin: "ai".into(),
            provider: provider_label,
            model: model_label,
            prompt: question.clone(),
            files,
            tool_calls: Vec::new(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: u64::try_from(started_at.elapsed().as_millis()).ok(),
        };
        rec.append(entry).await;
    }

    Ok(serde_json::json!({
        "session_id": session_id,
        "text": text,
        "sources": sources,
        "citations": citations,
    }))
}
