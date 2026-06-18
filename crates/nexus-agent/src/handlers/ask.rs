//! `com.nexus.agent::ask` / `::ask_respond` — interactive prompts (Phase 5.2).
//!
//! Mirrors the Phase 2b approval bus-bridge: `ask` registers a `oneshot` in
//! [`PendingAsks`], publishes a `com.nexus.agent.ask_requested` event, and
//! awaits a reply; a frontend renders the questions and calls `ask_respond`
//! with the answers, which fulfils the channel. Without a frontend the wait
//! times out and `ask` returns `timed_out: true`, so headless / auto-approve
//! sessions never hang.

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Events as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

use super::shared::{exec_err, insert_ask_bounded, parse_args, PendingAsks, DEFAULT_ASK_TIMEOUT_SECS};

/// Bus topic carrying an interactive prompt to frontends.
pub(crate) const EVENT_ASK_REQUESTED: &str = "com.nexus.agent.ask_requested";

/// Args for `com.nexus.agent::ask`.
#[derive(Debug, Deserialize)]
pub(crate) struct AskArgs {
    questions: Vec<AskQuestion>,
}

/// One question in an [`AskArgs`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AskQuestion {
    /// Caller-chosen id echoed back in the answer.
    id: String,
    /// The question text shown to the user.
    prompt: String,
    /// Selectable options. Empty means free-form input.
    #[serde(default)]
    options: Vec<String>,
    /// Whether multiple options may be selected.
    #[serde(default)]
    multi: bool,
}

/// Args for `com.nexus.agent::ask_respond` — the frontend's reply.
#[derive(Debug, Deserialize)]
pub(crate) struct AskRespondArgs {
    ask_id: String,
    /// Answers payload, passed through verbatim to the waiting `ask` call.
    answers: serde_json::Value,
}

/// `ask` — publish the questions and await the user's answers.
pub(crate) async fn handle_ask(
    ctx: Arc<KernelPluginContext>,
    pending: Arc<PendingAsks>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AskArgs = parse_args(args, "ask")?;
    if parsed.questions.is_empty() {
        return Err(exec_err("ask: at least one question is required".into()));
    }

    let ask_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
    match pending.lock() {
        Ok(mut map) => {
            insert_ask_bounded(&mut map, ask_id.clone(), tx);
        }
        Err(e) => return Err(exec_err(format!("ask: pending-asks lock poisoned: {e}"))),
    }

    let payload = serde_json::json!({ "ask_id": ask_id, "questions": parsed.questions });
    if let Err(e) = ctx.publish(EVENT_ASK_REQUESTED, payload) {
        if let Ok(mut map) = pending.lock() {
            map.remove(&ask_id);
        }
        return Err(exec_err(format!("ask: publish {EVENT_ASK_REQUESTED}: {e}")));
    }

    match tokio::time::timeout(Duration::from_secs(DEFAULT_ASK_TIMEOUT_SECS), rx).await {
        Ok(Ok(answers)) => {
            Ok(serde_json::json!({ "ask_id": ask_id, "answers": answers, "timed_out": false }))
        }
        // Timed out, or the responder dropped the channel: clean up and report
        // a timeout rather than erroring, so the model can proceed without a
        // user (the common headless / auto-approve case).
        _ => {
            if let Ok(mut map) = pending.lock() {
                map.remove(&ask_id);
            }
            Ok(serde_json::json!({ "ask_id": ask_id, "answers": [], "timed_out": true }))
        }
    }
}

/// `ask_respond` — deliver a frontend's answers to the waiting `ask` call.
pub(crate) async fn handle_ask_respond(
    pending: Arc<PendingAsks>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AskRespondArgs = parse_args(args, "ask_respond")?;
    let entry = {
        let mut map = pending
            .lock()
            .map_err(|e| exec_err(format!("ask_respond: pending-asks lock poisoned: {e}")))?;
        map.remove(&parsed.ask_id)
    };
    let Some(entry) = entry else {
        return Err(exec_err(format!(
            "ask_respond: no pending ask '{}'",
            parsed.ask_id
        )));
    };
    if entry.tx.send(parsed.answers).is_err() {
        return Err(exec_err(format!(
            "ask_respond: ask '{}' is no longer awaiting a reply",
            parsed.ask_id
        )));
    }
    Ok(serde_json::json!({ "delivered": true, "ask_id": parsed.ask_id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> Arc<PendingAsks> {
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()))
    }

    #[tokio::test]
    async fn ask_respond_with_no_pending_errors() {
        let p = pending();
        let err = handle_ask_respond(
            Arc::clone(&p),
            &serde_json::json!({ "ask_id": "nope", "answers": [] }),
        )
        .await
        .unwrap_err();
        assert!(format!("{err:?}").contains("no pending ask"));
    }

    #[tokio::test]
    async fn respond_delivers_to_a_waiting_ask() {
        // Register a pending ask by hand, then deliver to it.
        let p = pending();
        let (tx, rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
        {
            let mut map = p.lock().unwrap();
            insert_ask_bounded(&mut map, "a1".to_string(), tx);
        }
        let reply = handle_ask_respond(
            Arc::clone(&p),
            &serde_json::json!({ "ask_id": "a1", "answers": [{ "id": "q1", "selected": ["yes"] }] }),
        )
        .await
        .unwrap();
        assert_eq!(reply["delivered"], true);
        let answers = rx.await.unwrap();
        assert_eq!(answers[0]["selected"][0], "yes");
        // The entry is consumed; a second respond fails.
        assert!(handle_ask_respond(
            Arc::clone(&p),
            &serde_json::json!({ "ask_id": "a1", "answers": [] })
        )
        .await
        .is_err());
    }

    /// The `ask` tool's per-tool dispatch budget must outlast the
    /// handler's own wait, so the handler returns `timed_out` gracefully
    /// before the bridge's `ipc_call` deadline turns the call into a
    /// transport error.
    #[test]
    fn ask_dispatch_timeout_exceeds_handler_wait() {
        assert!(
            crate::tool_registry::ASK_DISPATCH_TIMEOUT_MS > DEFAULT_ASK_TIMEOUT_SECS * 1000,
            "ask dispatch timeout ({}ms) must exceed the handler wait ({}s)",
            crate::tool_registry::ASK_DISPATCH_TIMEOUT_MS,
            DEFAULT_ASK_TIMEOUT_SECS,
        );
    }

    #[test]
    fn ask_args_parse_questions() {
        let parsed: AskArgs = serde_json::from_value(serde_json::json!({
            "questions": [{ "id": "q1", "prompt": "Pick one", "options": ["a", "b"], "multi": false }]
        }))
        .unwrap();
        assert_eq!(parsed.questions.len(), 1);
        assert_eq!(parsed.questions[0].options, ["a", "b"]);
    }
}
