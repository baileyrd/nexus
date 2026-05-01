//! Multi-agent orchestration (PRD-15 §10, BL-027).
//!
//! [`AgentOrchestrator`] hands subtasks between archetypes
//! (Researcher → Writer → Coder, …) sharing scratch state and
//! emitting a trace log. Each delegated job builds a fresh archetype
//! [`LlmAgent`] via [`build_archetype`], runs [`Agent::plan`], and
//! executes the resulting plan through a [`PlanExecutor`] backed by
//! the caller-supplied [`ToolDispatcher`]. Results are returned as
//! [`Observation`]s — exactly what the existing single-agent flow
//! produces — and a stage's textual summary is stashed in scratch
//! under `"prev"` for `{{prev}}`-style substitution in the next
//! stage's goal template.
//!
//! The orchestrator is library-only: the `com.nexus.agent` core
//! plugin in [`crate::core_plugin`] wires four IPC handlers
//! (`delegate`, `parallel`, `pipeline`, `trace_get`) over the same
//! `KernelPluginContext` adapters used by the existing `plan` /
//! `run` handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{
    build_archetype, Agent, AgentError, ChatDriver, Observation, PlanExecutor, ToolDispatcher,
};

/// One trace entry per delegated stage.
///
/// Captures enough metadata for an "agent timeline" UI: which
/// archetype ran, the goal it received (after substitution), wall-clock
/// timestamps, terminal status, and a short summary derived from the
/// observation. The trace log is append-only and lives behind a
/// `Mutex` so concurrent `parallel` jobs all land in the same vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct TraceEntry {
    /// Zero-based ordinal in the orchestrator's append order. For
    /// `parallel` jobs this reflects completion order, not input
    /// order; pair with the response vector when ordering matters.
    pub stage_idx: u32,
    /// Archetype id resolved by [`build_archetype`] (e.g.
    /// `com.nexus.agent.writer`).
    pub archetype: String,
    /// Goal text actually sent to the planner — already includes any
    /// `{{prev}}` substitution applied by [`AgentOrchestrator::pipeline`].
    pub goal: String,
    /// Unix epoch milliseconds when the stage started.
    pub started_at_ms: u64,
    /// Unix epoch milliseconds when the stage ended (success or fail).
    pub ended_at_ms: u64,
    /// Terminal status — one of `"ok"`, `"failed"`, `"plan_failed"`.
    pub status: String,
    /// Short human-readable summary derived from the resulting
    /// observation. For pipelines this is also the value stashed
    /// under the `"prev"` scratch key.
    pub summary: String,
}

/// Orchestrator handing subtasks between archetypes with shared
/// scratch state (PRD-15 §10).
///
/// `D: ChatDriver + Clone` so each delegated stage can spin up its
/// own [`crate::LlmAgent`] without consuming the orchestrator's
/// driver. `P: ToolDispatcher + Clone` so the executor can be rebuilt
/// per stage too.
pub struct AgentOrchestrator<D, P>
where
    D: ChatDriver + Clone + 'static,
    P: ToolDispatcher + Clone,
{
    driver: D,
    dispatcher: P,
    scratch: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    trace: Arc<Mutex<Vec<TraceEntry>>>,
}

impl<D, P> AgentOrchestrator<D, P>
where
    D: ChatDriver + Clone + 'static,
    P: ToolDispatcher + Clone + 'static,
{
    /// Build an orchestrator over a chat driver + tool dispatcher.
    pub fn new(driver: D, dispatcher: P) -> Self {
        Self {
            driver,
            dispatcher,
            scratch: Arc::new(RwLock::new(HashMap::new())),
            trace: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Delegate a single goal to one archetype. Plans via
    /// [`Agent::plan`], executes via [`PlanExecutor::run`], appends a
    /// [`TraceEntry`].
    ///
    /// `_skills` is reserved for future skill layering — today the
    /// orchestrator forwards `None` to [`build_archetype`] for the
    /// extra prompt. Callers that need skill-matched prompts should
    /// resolve them upstream and pass them as part of the goal text;
    /// see BL-027 for the rationale.
    ///
    /// # Errors
    /// Returns an [`AgentError`] when planning fails. Tool-call
    /// failures during execution surface as `Err` but a trace entry
    /// with status `"failed"` is still appended for observability.
    pub async fn delegate(
        &self,
        archetype: &str,
        goal: &str,
        _skills: Option<&[String]>,
    ) -> Result<Observation, AgentError> {
        let started = now_ms();
        let agent = build_archetype(Some(archetype), self.driver.clone(), None);
        let archetype_id = agent.id().to_string();

        let plan = match agent.plan(goal).await {
            Ok(plan) => plan,
            Err(err) => {
                self.append_trace(TraceEntry {
                    stage_idx: self.next_idx().await,
                    archetype: archetype_id,
                    goal: goal.to_string(),
                    started_at_ms: started,
                    ended_at_ms: now_ms(),
                    status: "plan_failed".to_string(),
                    summary: err.to_string(),
                })
                .await;
                return Err(err);
            }
        };

        let executor = PlanExecutor::new(self.dispatcher.clone());
        let result = executor.run(&plan).await;
        let ended = now_ms();
        let idx = self.next_idx().await;
        match result {
            Ok(observation) => {
                let summary = summarise(&observation);
                self.append_trace(TraceEntry {
                    stage_idx: idx,
                    archetype: archetype_id,
                    goal: goal.to_string(),
                    started_at_ms: started,
                    ended_at_ms: ended,
                    status: "ok".to_string(),
                    summary,
                })
                .await;
                Ok(observation)
            }
            Err(err) => {
                self.append_trace(TraceEntry {
                    stage_idx: idx,
                    archetype: archetype_id,
                    goal: goal.to_string(),
                    started_at_ms: started,
                    ended_at_ms: ended,
                    status: "failed".to_string(),
                    summary: err.to_string(),
                })
                .await;
                Err(err)
            }
        }
    }

    /// Fan out a list of `(archetype, goal)` jobs in parallel.
    /// Results are returned in the same order as `jobs`. Individual
    /// job errors are turned into a synthetic failed [`Observation`]
    /// so the caller still gets a positional vector — inspect the
    /// trace to see per-stage status.
    pub async fn parallel(&self, jobs: &[(String, String)]) -> Vec<Observation> {
        let futures = jobs.iter().map(|(arch, goal)| {
            let arch = arch.clone();
            let goal = goal.clone();
            async move {
                match self.delegate(&arch, &goal, None).await {
                    Ok(obs) => obs,
                    Err(err) => failed_observation(&err.to_string()),
                }
            }
        });
        futures::future::join_all(futures).await
    }

    /// Run stages sequentially. After each stage the textual summary
    /// of its [`Observation`] is stored in scratch under the key
    /// `"prev"`, and the next stage's `goal_template` has every
    /// `{{prev}}` token substituted before planning. Stops on the
    /// first stage failure and returns the partial observation list
    /// (not `Err`); inspect [`Self::trace`] for per-stage status.
    pub async fn pipeline(
        &self,
        stages: &[(String, String)],
    ) -> Vec<Observation> {
        let mut out = Vec::with_capacity(stages.len());
        for (archetype, template) in stages {
            let prev_value = self.scratch_get("prev").await;
            let prev_str = prev_value
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let goal = template.replace("{{prev}}", prev_str);
            match self.delegate(archetype, &goal, None).await {
                Ok(obs) => {
                    let summary = summarise(&obs);
                    self.scratch_set("prev", serde_json::Value::String(summary))
                        .await;
                    out.push(obs);
                }
                Err(_) => break,
            }
        }
        out
    }

    /// Snapshot of the trace log, in append order.
    pub async fn trace(&self) -> Vec<TraceEntry> {
        self.trace.lock().await.clone()
    }

    /// Read a key from scratch state. Returns `None` when absent.
    pub async fn scratch_get(&self, key: &str) -> Option<serde_json::Value> {
        self.scratch.read().await.get(key).cloned()
    }

    /// Write a key into scratch state. Overwrites any prior value.
    pub async fn scratch_set(&self, key: &str, value: serde_json::Value) {
        self.scratch.write().await.insert(key.to_string(), value);
    }

    async fn append_trace(&self, entry: TraceEntry) {
        self.trace.lock().await.push(entry);
    }

    async fn next_idx(&self) -> u32 {
        // Length at time of capture is fine — pipeline is sequential
        // and parallel jobs may interleave, but that's documented.
        let len = self.trace.lock().await.len();
        u32::try_from(len).unwrap_or(u32::MAX)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn failed_observation(reason: &str) -> Observation {
    Observation {
        plan_id: format!("orchestrator-failed-{reason}"),
        steps: Vec::new(),
        success: false,
    }
}

/// Best-effort textual summary used both for trace entries and for
/// the `"prev"` scratch handoff in pipelines. Concatenates each
/// step's tool response (if present) as a JSON-stringified blob — the
/// next stage's planner can parse it back if needed, and a human
/// inspecting the trace sees the whole observation.
fn summarise(obs: &Observation) -> String {
    let parts: Vec<String> = obs
        .steps
        .iter()
        .map(|s| match &s.response {
            Some(v) => v.to_string(),
            None => format!("step {} status={:?}", s.step_id, s.status),
        })
        .collect();
    if parts.is_empty() {
        format!("plan {} success={}", obs.plan_id, obs.success)
    } else {
        parts.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::ToolCall;

    #[derive(Clone)]
    struct CannedDriver {
        reply: Arc<String>,
    }
    impl CannedDriver {
        fn new(reply: &str) -> Self {
            Self { reply: Arc::new(reply.to_string()) }
        }
    }
    #[async_trait]
    impl ChatDriver for CannedDriver {
        async fn chat(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok((*self.reply).clone())
        }
    }

    #[derive(Clone, Default)]
    struct CountingDispatcher {
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl ToolDispatcher for CountingDispatcher {
        async fn dispatch(&self, _call: &ToolCall) -> Result<serde_json::Value, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"ok": true}))
        }
    }

    fn one_step_reply(desc: &str) -> String {
        format!(r#"{{"steps":[{{"description":"{desc}","tool_call":null}}]}}"#)
    }

    #[tokio::test]
    async fn delegate_runs_and_appends_trace() {
        let driver = CannedDriver::new(&one_step_reply("research"));
        let orch = AgentOrchestrator::new(driver, CountingDispatcher::default());
        let obs = orch.delegate("researcher", "find stuff", None).await.unwrap();
        assert!(obs.success);
        assert_eq!(obs.steps.len(), 1);
        let trace = orch.trace().await;
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].archetype, crate::RESEARCHER_ID);
        assert_eq!(trace[0].status, "ok");
        assert_eq!(trace[0].stage_idx, 0);
        assert_eq!(trace[0].goal, "find stuff");
    }

    #[tokio::test]
    async fn scratch_state_round_trips() {
        let orch = AgentOrchestrator::new(
            CannedDriver::new(&one_step_reply("noop")),
            CountingDispatcher::default(),
        );
        assert!(orch.scratch_get("k").await.is_none());
        orch.scratch_set("k", serde_json::json!(42)).await;
        assert_eq!(orch.scratch_get("k").await, Some(serde_json::json!(42)));
    }

    #[tokio::test]
    async fn trace_ordering_reflects_append_order() {
        let orch = AgentOrchestrator::new(
            CannedDriver::new(&one_step_reply("step")),
            CountingDispatcher::default(),
        );
        orch.delegate("writer", "a", None).await.unwrap();
        orch.delegate("coder", "b", None).await.unwrap();
        let trace = orch.trace().await;
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].stage_idx, 0);
        assert_eq!(trace[1].stage_idx, 1);
        assert_eq!(trace[0].archetype, crate::WRITER_ID);
        assert_eq!(trace[1].archetype, crate::CODER_ID);
    }
}
