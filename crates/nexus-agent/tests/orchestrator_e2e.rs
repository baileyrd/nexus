//! BL-027 — orchestrator pipeline e2e.
//!
//! Wires two canned `ChatDriver`s through one [`AgentOrchestrator`] and
//! exercises the `pipeline` flow: a Researcher returns a one-step plan
//! whose description is `"find data X"`; a Writer returns
//! `"summary of: <prev>"` after the orchestrator substitutes
//! `{{prev}}` with the Researcher's textual summary. We assert the
//! shape of the resulting observations and inspect [`TraceEntry`]
//! goals to confirm the substitution actually happened by the time
//! planning ran.

#![allow(clippy::missing_panics_doc)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use nexus_agent::{AgentOrchestrator, ChatDriver, ToolCall, ToolDispatcher};
use serde_json::Value;

/// Driver that branches on the system prompt — the archetype-specific
/// prompt let us tell Researcher and Writer apart inside one shared
/// driver instance, so the orchestrator can run both archetypes
/// without juggling two driver factories.
#[derive(Clone)]
struct BranchingDriver {
    researcher_reply: Arc<String>,
    writer_reply: Arc<String>,
    /// Captures every `(system, user)` pair the planner sent — lets
    /// the test assert `{{prev}}` was substituted before reaching
    /// `chat`, not just before reaching `delegate`.
    seen: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl ChatDriver for BranchingDriver {
    async fn chat(&self, system: &str, user: &str) -> Result<String, String> {
        self.seen
            .lock()
            .map_err(|e| e.to_string())?
            .push((system.to_string(), user.to_string()));
        if system.to_ascii_lowercase().contains("research") {
            Ok((*self.researcher_reply).clone())
        } else {
            Ok((*self.writer_reply).clone())
        }
    }
}

#[derive(Clone, Default)]
struct CountingDispatcher {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ToolDispatcher for CountingDispatcher {
    async fn dispatch(&self, _call: &ToolCall) -> Result<Value, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({"ok": true}))
    }
}

fn plan_with_description(desc: &str) -> String {
    // The LLM planner accepts `{ "steps": [ { "description": ..., "tool_call": null } ] }`.
    format!(r#"{{"steps":[{{"description":"{desc}","tool_call":null}}]}}"#)
}

#[tokio::test]
async fn pipeline_substitutes_prev_and_returns_two_observations() {
    let driver = BranchingDriver {
        researcher_reply: Arc::new(plan_with_description("find data X")),
        writer_reply: Arc::new(plan_with_description("summary of: {prev}")),
        seen: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let orch = AgentOrchestrator::new(driver.clone(), CountingDispatcher::default());

    let stages = vec![
        ("researcher".to_string(), "find X".to_string()),
        (
            "writer".to_string(),
            "summarize {{prev}}".to_string(),
        ),
    ];
    let observations = orch.pipeline(&stages).await;

    assert_eq!(
        observations.len(),
        2,
        "pipeline should produce one observation per stage"
    );
    assert!(
        observations.iter().all(|o| o.success),
        "both stages should succeed",
    );

    let trace = orch.trace().await;
    assert_eq!(trace.len(), 2, "one trace entry per stage");
    assert_eq!(trace[0].goal, "find X", "researcher goal unchanged");

    // Writer goal: orchestrator must have replaced `{{prev}}` with the
    // researcher's textual summary before the planner saw it.
    let writer_goal = &trace[1].goal;
    assert!(
        writer_goal.starts_with("summarize "),
        "writer goal preserved template prefix: {writer_goal}",
    );
    assert!(
        !writer_goal.contains("{{prev}}"),
        "writer goal must not still contain the unsubstituted token: {writer_goal}",
    );
    assert!(
        writer_goal.len() > "summarize ".len(),
        "substitution should have inserted non-empty content: {writer_goal}",
    );
}
