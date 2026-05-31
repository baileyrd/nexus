//! BL-064 AI suggestion handler (`suggest`). Pure free `async fn` —
//! `dispatch_async` captures it via `Box::pin` so it never holds a
//! borrow on `TerminalCorePlugin` across awaits.
//!
//! Split out of `core_plugin.rs` by SD-03 terminal chunk 5.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;

use crate::ai::AiSuggestionEngine;
use crate::core_plugin::{SuggestArgs, SuggestResponse, PLUGIN_ID};
use crate::server::InMemoryTerminalServer;
use crate::session::SessionId;

use super::shared::{exec_err, poisoned, to_value};

/// Hard ceiling on the per-call wait for the `com.nexus.ai::stream_chat`
/// response. The chip is a UI hint; if the model is slow we'd rather
/// surface the static rule's reason than spin a spinner forever.
const SUGGEST_LLM_TIMEOUT: Duration = Duration::from_secs(10);

/// Default tail length scanned by `suggest` when the caller doesn't
/// supply `line_count`. Large enough to catch a build-error block;
/// small enough that the in-memory rule engine evaluates in
/// microseconds.
const SUGGEST_DEFAULT_LINE_COUNT: usize = 50;

/// BL-064 — async handler for `com.nexus.terminal::suggest`. Walks
/// the recent N lines, finds the first matching rule, and (when a
/// kernel context is wired) routes the matched line + rule through
/// `com.nexus.ai::stream_chat` for an enriched explanation.
///
/// The function is intentionally a free `async fn`: the dispatcher
/// captures it via `Box::pin(...)` in `dispatch_async`, so it never
/// borrows `&mut self` past the synchronous setup.
pub(crate) async fn handle_suggest(
    ctx: Option<&Arc<KernelPluginContext>>,
    server: &Arc<Mutex<InMemoryTerminalServer>>,
    engine: &Arc<AiSuggestionEngine>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SuggestArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("suggest: invalid args: {e}")))?;
    let limit = parsed
        .line_count
        .map(|n| usize::try_from(n).unwrap_or(SUGGEST_DEFAULT_LINE_COUNT))
        .unwrap_or(SUGGEST_DEFAULT_LINE_COUNT)
        .max(1);

    // Read the tail of the line buffer under a brief server lock —
    // we drop the lock before the (possibly slow) IPC call so the
    // drainer / IPC dispatcher aren't blocked on the LLM provider.
    let session_id = SessionId::from_string(parsed.session_id.clone());
    let lines: Vec<crate::lines::Line> = {
        let guard = server.lock().map_err(poisoned)?;
        let manager = guard.manager();
        let total = manager
            .line_count(&session_id)
            .ok_or_else(|| exec_err(format!("suggest: unknown session '{}'", parsed.session_id)))?;
        let start = total.saturating_sub(limit);
        manager
            .lines_snapshot(&session_id, Some(start), Some(total - start))
            .unwrap_or_default()
    };

    // First match wins. Iterating lines newest-first means a fresh
    // breach near the prompt outranks an older error scrolling out
    // of the tail window.
    let suggestion = lines
        .iter()
        .rev()
        .find_map(|line| engine.observe(&line.text_only).into_iter().next());
    let Some(suggestion) = suggestion else {
        return Ok(serde_json::Value::Null);
    };

    let static_response = SuggestResponse {
        text: suggestion.text.clone(),
        reason: suggestion.reason.clone(),
        severity: severity_tag(suggestion.severity).to_string(),
        source_rule: suggestion.source_rule.to_string(),
        llm_used: false,
    };

    // No kernel context → no IPC → return the static rule response.
    let Some(ctx) = ctx else {
        return to_value(&static_response, "suggest");
    };

    // Build the enrichment prompt. Keep it tight — we want a 2-3
    // sentence explanation, not a chat transcript. The matched line
    // gives the model the concrete output that triggered the rule;
    // the rule's reason gives it a structural framing.
    let matched_line: String = lines
        .iter()
        .rev()
        .find(|l| !engine.observe(&l.text_only).is_empty())
        .map(|l| l.text_only.clone())
        .unwrap_or_default();
    let user_prompt = format!(
        "Terminal pattern matched: {rule}\n\
         Matched line: `{line}`\n\
         Suggested command: `{cmd}`\n\n\
         In 2-3 sentences, explain what's likely wrong and why the suggested \
         command helps. Be concrete; don't restate the prompt.",
        rule = suggestion.source_rule,
        line = matched_line.replace('`', "'"),
        cmd = suggestion.text,
    );

    let ai_args = serde_json::json!({
        "messages": [
            {"role": "user", "content": user_prompt}
        ],
        "system": "You are a developer assistant explaining terminal output. Reply with prose, no markdown lists.",
        "mode": "complete",
        "tools": "none",
        "max_tokens": 200,
    });

    // Call into `com.nexus.ai::stream_chat` with a hard 10 s budget.
    // tokio::time::timeout wraps the whole IPC future so a hanging
    // provider doesn't strand the suggestion chip indefinitely. The
    // inner `ipc_call` already accepts a per-call timeout, but a
    // misbehaving provider can still leak the future past the
    // deadline; the outer timeout is the load-bearing guarantee.
    let llm_call = ctx.ipc_call("com.nexus.ai", "stream_chat", ai_args, SUGGEST_LLM_TIMEOUT);
    let enriched_text: Option<String> = match tokio::time::timeout(SUGGEST_LLM_TIMEOUT, llm_call)
        .await
    {
        Ok(Ok(response)) => {
            let text = response
                .get("text")
                .and_then(|v: &serde_json::Value| v.as_str())
                .map(str::to_string);
            text.filter(|s: &String| !s.trim().is_empty())
        }
        Ok(Err(err)) => {
            tracing::debug!(plugin = PLUGIN_ID, %err, "suggest: AI call failed; falling back to static rule");
            None
        }
        Err(_) => {
            tracing::debug!(
                plugin = PLUGIN_ID,
                "suggest: AI call timed out after 10s; falling back to static rule"
            );
            None
        }
    };

    let response = match enriched_text {
        Some(reason) => SuggestResponse {
            reason,
            llm_used: true,
            ..static_response
        },
        None => static_response,
    };
    to_value(&response, "suggest")
}

/// Map the [`crate::SuggestionSeverity`] enum to its serde tag — the
/// IPC response carries the lowercase string verbatim so an off-the-
/// shelf TS client can switch on it.
fn severity_tag(s: crate::ai::SuggestionSeverity) -> &'static str {
    match s {
        crate::ai::SuggestionSeverity::Info => "info",
        crate::ai::SuggestionSeverity::Warning => "warning",
        crate::ai::SuggestionSeverity::Error => "error",
    }
}
