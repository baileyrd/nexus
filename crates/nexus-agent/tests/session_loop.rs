//! Integration tests for the public session loop (gap-analysis
//! 2026-07-01 §4 / queue item 6 — nexus-agent previously had no
//! `tests/` dir; cross-module behaviour was covered only by in-src
//! unit tests).
//!
//! These exercise the crate's *public* surface end-to-end: a canned
//! [`ChatDriver`], the real [`run_session`] loop, [`AutoApproveAll`],
//! and a recording [`ToolDispatcher`] — no kernel, no network.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use nexus_agent::{
    run_session, AutoApproveAll, ChatDriver, Proposal, ProposedToolCall, SessionOutcome, ToolCall,
    ToolDispatchError, ToolDispatcher,
};

/// Driver that emits one tool call on the first round and a plain
/// final answer on the second.
struct OneToolThenDone {
    round: AtomicUsize,
}

#[async_trait]
impl ChatDriver for OneToolThenDone {
    async fn propose(&self, _system: &str, _user: &str) -> Result<Proposal, String> {
        match self.round.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(Proposal {
                text: "checking the forge".to_string(),
                tool_calls: vec![ProposedToolCall {
                    id: "call-1".to_string(),
                    name: "search".to_string(),
                    tool_call: ToolCall {
                        target_plugin_id: "com.nexus.storage".to_string(),
                        command_id: "search".to_string(),
                        args: serde_json::json!({ "query": "hello" }),
                    },
                }],
                usage: None,
            }),
            _ => Ok(Proposal {
                text: "done: found it".to_string(),
                tool_calls: vec![],
                usage: None,
            }),
        }
    }
}

/// Dispatcher that records every call and answers with a fixed reply.
#[derive(Default)]
struct RecordingDispatcher {
    calls: Mutex<Vec<ToolCall>>,
}

#[async_trait]
impl ToolDispatcher for RecordingDispatcher {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, ToolDispatchError> {
        self.calls
            .lock()
            .expect("recording dispatcher poisoned")
            .push(call.clone());
        Ok(serde_json::json!({ "results": [] }))
    }
}

#[tokio::test]
async fn session_runs_tool_round_then_completes() {
    let driver = OneToolThenDone {
        round: AtomicUsize::new(0),
    };
    let dispatcher = RecordingDispatcher::default();

    let session = run_session(
        &driver,
        &dispatcher,
        &AutoApproveAll,
        "find hello in my notes",
        "you are a test agent",
        None,
    )
    .await;

    assert!(
        matches!(session.outcome, SessionOutcome::Complete),
        "expected completion, got {:?}",
        session.outcome
    );
    // The one proposed tool call reached the dispatcher with its
    // target intact.
    let calls = dispatcher.calls.lock().expect("poisoned");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_plugin_id, "com.nexus.storage");
    assert_eq!(calls[0].command_id, "search");
    // Two rounds recorded: tool round + final round.
    assert_eq!(session.rounds.len(), 2);
}

/// Driver that answers immediately with no tool calls.
struct ImmediateAnswer;

#[async_trait]
impl ChatDriver for ImmediateAnswer {
    async fn propose(&self, _system: &str, _user: &str) -> Result<Proposal, String> {
        Ok(Proposal {
            text: "the answer is 42".to_string(),
            tool_calls: vec![],
            usage: None,
        })
    }
}

#[tokio::test]
async fn tool_free_answer_completes_in_one_round_without_dispatch() {
    let dispatcher = RecordingDispatcher::default();
    let session = run_session(
        &ImmediateAnswer,
        &dispatcher,
        &AutoApproveAll,
        "what is the answer",
        "you are a test agent",
        None,
    )
    .await;

    assert!(matches!(session.outcome, SessionOutcome::Complete));
    assert_eq!(session.rounds.len(), 1);
    assert!(
        dispatcher.calls.lock().expect("poisoned").is_empty(),
        "no tool must be dispatched for a tool-free answer"
    );
}

#[tokio::test]
async fn empty_goal_short_circuits() {
    let dispatcher = RecordingDispatcher::default();
    let session = run_session(
        &ImmediateAnswer,
        &dispatcher,
        &AutoApproveAll,
        "",
        "system",
        None,
    )
    .await;
    assert!(
        !matches!(session.outcome, SessionOutcome::Complete) || session.rounds.is_empty(),
        "an empty goal must not run a normal session loop"
    );
}
