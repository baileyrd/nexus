//! Workflow step executor (PRD-16 §7 minimum-viable slice).
//!
//! Walks a [`Workflow`]'s `[[steps]]` in order, dispatching each via
//! an injected [`ActionDispatcher`]. This is the deterministic path
//! — parallel steps and variable interpolation beyond `${trigger.*}`
//! are planned follow-ups.
//!
//! Retry policy is *per step*: each step's `max_retries` /
//! `retry_backoff` / `retry_initial_delay_ms` / `retry_max_delay_ms` /
//! `retry_jitter` shadow the workflow-level `[error_handling]` block,
//! which in turn falls back to built-in defaults (`max_retries = 0`,
//! exponential backoff, 100 ms base, 30 s cap, full jitter on). When
//! parallel scheduling lands (BL-028 #4) each branch will retry
//! independently with this same per-step config.
//!
//! The executor is library-only; no kernel or IPC dependency. A core
//! plugin wraps it with a [`KernelActionDispatcher`] equivalent that
//! routes `ipc` actions through `PluginContext::ipc_call`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::interpolate::{interpolate_step, VariableMap};
use crate::{Step, Workflow};

/// Dispatcher trait — the executor calls one of these per step to
/// carry out the action. `step.step_type` selects the semantics;
/// the dispatcher owns the matching logic (an `ipc` step forwards
/// through `ipc_call`, a `noop` step does nothing, etc.).
#[async_trait]
pub trait ActionDispatcher: Send + Sync {
    /// Execute a single step. Returns the step's response value
    /// (opaque JSON) on success, an error message on failure. The
    /// executor decides how to aggregate; this trait just runs the
    /// one step.
    async fn run(&self, step: &Step) -> Result<serde_json::Value, String>;
}

/// Per-step outcome in a [`WorkflowRun`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOutcome {
    /// Step name when present (`[[steps]].name`), else a synthetic
    /// `step-N` id for tracking.
    pub step_id: String,
    /// Action type — mirrors `step.step_type`.
    pub step_type: String,
    /// Dispatcher response when the step ran to completion.
    pub response: Option<serde_json::Value>,
    /// Terminal status.
    pub status: StepOutcomeStatus,
    /// Error message when `status == Failed`. `None` otherwise.
    pub error: Option<String>,
    /// Number of dispatch attempts made for this step. `1` means the
    /// first attempt succeeded (or the only attempt failed with no
    /// retries configured). `N` means the step finished — successfully
    /// or otherwise — on its `N`-th try.
    #[serde(default = "default_attempts")]
    pub attempts: u32,
}

fn default_attempts() -> u32 {
    1
}

/// Terminal status for one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcomeStatus {
    /// Dispatcher returned Ok.
    Ok,
    /// Dispatcher returned Err and the step's `on_error` allowed
    /// continuation (`"continue"` / `"log_warn"`).
    Failed,
    /// An earlier step failed and this step's `on_error` policy was
    /// `stop` (default), so the executor stopped before running it.
    Skipped,
}

/// Result of a full workflow run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// `workflow.name` from the source document.
    pub workflow_name: String,
    /// Ordered outcomes, one per `[[steps]]` entry.
    pub steps: Vec<StepOutcome>,
    /// `true` when no step failed. A run with `on_error = "continue"`
    /// failures still reports `false` here — the boolean tracks
    /// correctness, not completion. A condition-skipped run also
    /// reports `true` — the gate closed cleanly, which is a success.
    pub success: bool,
    /// `true` when the workflow's `[condition]` evaluated to false
    /// and the executor short-circuited before running any step.
    /// `steps` is empty in that case.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub condition_skipped: bool,
}

/// Errors from [`run_workflow`].
#[derive(Debug, Error)]
pub enum WorkflowExecutionError {
    /// The workflow has no steps. Not necessarily an error for the
    /// caller, but the executor surfaces it so UIs can short-circuit.
    #[error("workflow '{0}' has no steps")]
    EmptyPlan(String),
}

/// Execute a workflow's steps in order.
///
/// For each step:
/// 1. Dispatcher runs it.
/// 2. On `Ok`, record `Ok` with the response.
/// 3. On `Err`, record `Failed` with the error. If `step.on_error`
///    is `Some("continue")` or `Some("log_warn")` the loop continues
///    to the next step; otherwise it stops and remaining steps are
///    emitted as `Skipped` placeholders.
///
/// # Errors
///
/// [`WorkflowExecutionError::EmptyPlan`] when the workflow has zero
/// steps. Dispatcher failures are *not* bubbled as errors — they're
/// captured in the [`WorkflowRun`] per step so the UI has a stable
/// shape to render.
pub async fn run_workflow<D: ActionDispatcher>(
    workflow: &Workflow,
    dispatcher: &D,
) -> Result<WorkflowRun, WorkflowExecutionError> {
    run_workflow_with_variables(workflow, dispatcher, &VariableMap::new()).await
}

/// Execute a workflow's steps with a pre-built variable map.
///
/// Each step's `extra` fields are passed through
/// [`interpolate_step`](crate::interpolate::interpolate_step) before
/// the dispatcher sees them, so `${trigger.path}` / `${inputs.dir}` /
/// etc. placeholders resolve against `variables`. Unknown placeholders
/// pass through verbatim (see the module docs on
/// [`crate::interpolate`]).
///
/// Callers that don't need variable interpolation should use
/// [`run_workflow`].
///
/// # Errors
/// Same as [`run_workflow`] — [`WorkflowExecutionError::EmptyPlan`]
/// when the workflow has zero steps.
pub async fn run_workflow_with_variables<D: ActionDispatcher>(
    workflow: &Workflow,
    dispatcher: &D,
    variables: &VariableMap,
) -> Result<WorkflowRun, WorkflowExecutionError> {
    if workflow.steps.is_empty() {
        return Err(WorkflowExecutionError::EmptyPlan(
            workflow.workflow.name.clone(),
        ));
    }
    let mut outcomes: Vec<StepOutcome> = Vec::with_capacity(workflow.steps.len());
    let mut abort = false;
    for (i, step) in workflow.steps.iter().enumerate() {
        let step_id = step
            .name
            .clone()
            .unwrap_or_else(|| format!("step-{i}"));
        if abort {
            outcomes.push(StepOutcome {
                step_id,
                step_type: step.step_type.clone(),
                response: None,
                status: StepOutcomeStatus::Skipped,
                error: None,
                attempts: 0,
            });
            continue;
        }
        let mut resolved = step.clone();
        if !variables.is_empty() {
            interpolate_step(&mut resolved, variables);
        }
        let step = &resolved;

        let result = dispatch_with_retry(workflow, step, dispatcher).await;

        match result {
            Ok((response, attempts)) => outcomes.push(StepOutcome {
                step_id,
                step_type: step.step_type.clone(),
                response: Some(response),
                status: StepOutcomeStatus::Ok,
                error: None,
                attempts,
            }),
            Err((reason, attempts)) => {
                let policy = step.on_error.as_deref().unwrap_or("stop");
                let continue_on_error = matches!(policy, "continue" | "log_warn");
                outcomes.push(StepOutcome {
                    step_id,
                    step_type: step.step_type.clone(),
                    response: None,
                    status: StepOutcomeStatus::Failed,
                    error: Some(reason),
                    attempts,
                });
                if !continue_on_error {
                    abort = true;
                }
            }
        }
    }
    let success = outcomes
        .iter()
        .all(|o| o.status == StepOutcomeStatus::Ok);
    Ok(WorkflowRun {
        workflow_name: workflow.workflow.name.clone(),
        steps: outcomes,
        success,
        condition_skipped: false,
    })
}

/// Resolve the retry config for `step` against the workflow-level
/// `[error_handling]` block and run the dispatcher with backoff.
///
/// Returns `Ok((response, attempts))` if the step ultimately succeeded
/// or `Err((reason, attempts))` if all retries were exhausted.
async fn dispatch_with_retry<D: ActionDispatcher>(
    workflow: &Workflow,
    step: &Step,
    dispatcher: &D,
) -> Result<(serde_json::Value, u32), (String, u32)> {
    let max_retries = step
        .max_retries
        .or_else(|| {
            workflow
                .error_handling
                .as_ref()
                .and_then(|eh| eh.max_retries)
        })
        .unwrap_or(0);
    let backoff_kind = step
        .retry_backoff
        .as_deref()
        .or_else(|| {
            workflow
                .error_handling
                .as_ref()
                .and_then(|eh| eh.retry_backoff.as_deref())
        })
        .unwrap_or("exponential");
    let base_ms = step.retry_initial_delay_ms.unwrap_or(100);
    let cap_ms = step.retry_max_delay_ms.unwrap_or(30_000);
    let jitter = step.retry_jitter.unwrap_or(true);

    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match dispatcher.run(step).await {
            Ok(v) => return Ok((v, attempt)),
            Err(e) if attempt > max_retries => return Err((e, attempt)),
            Err(_) => {
                let raw = match backoff_kind {
                    "constant" => base_ms,
                    "linear" => base_ms.saturating_mul(u64::from(attempt)),
                    // exponential (default): base * 2^(attempt-1),
                    // shift capped to avoid UB on big attempt counts.
                    _ => {
                        let shift = (attempt - 1).min(20);
                        base_ms.saturating_mul(1u64 << shift)
                    }
                }
                .min(cap_ms);
                let delay = if jitter { fastrand::u64(0..=raw) } else { raw };
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }
    }
}

/// Build an empty run representing a workflow whose `[condition]`
/// evaluated to `false`. Used by the core plugin when it gates a run
/// before dispatching.
#[must_use]
pub fn condition_skipped_run(workflow: &Workflow) -> WorkflowRun {
    WorkflowRun {
        workflow_name: workflow.workflow.name.clone(),
        steps: Vec::new(),
        success: true,
        condition_skipped: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_workflow_text;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct RecordingDispatcher {
        calls: Arc<AtomicUsize>,
        fail_at: Option<usize>,
    }

    #[async_trait]
    impl ActionDispatcher for RecordingDispatcher {
        async fn run(&self, _step: &Step) -> Result<serde_json::Value, String> {
            let idx = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_at == Some(idx) {
                return Err(format!("boom at {idx}"));
            }
            Ok(serde_json::json!({ "index": idx }))
        }
    }

    const THREE_STEPS: &str = r#"
[workflow]
name = "Three"

[trigger]
type = "manual"

[[steps]]
type = "noop"

[[steps]]
type = "noop"

[[steps]]
type = "noop"
"#;

    #[tokio::test]
    async fn every_step_runs_when_dispatcher_succeeds() {
        let wf = parse_workflow_text(THREE_STEPS).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let d = RecordingDispatcher {
            calls: Arc::clone(&calls),
            fail_at: None,
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(run.success);
        assert_eq!(run.steps.len(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn default_on_error_stops_and_skips_remaining() {
        let wf = parse_workflow_text(THREE_STEPS).unwrap();
        let d = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_at: Some(1),
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(!run.success);
        assert_eq!(run.steps[0].status, StepOutcomeStatus::Ok);
        assert_eq!(run.steps[1].status, StepOutcomeStatus::Failed);
        assert_eq!(run.steps[2].status, StepOutcomeStatus::Skipped);
    }

    #[tokio::test]
    async fn on_error_continue_runs_remaining_steps() {
        const WF_CONT: &str = r#"
[workflow]
name = "C"

[trigger]
type = "manual"

[[steps]]
type = "noop"

[[steps]]
type = "noop"
on_error = "continue"

[[steps]]
type = "noop"
"#;
        let wf = parse_workflow_text(WF_CONT).unwrap();
        let d = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_at: Some(1),
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(!run.success);
        assert_eq!(run.steps[2].status, StepOutcomeStatus::Ok);
    }

    #[tokio::test]
    async fn empty_plan_errors() {
        const WF_EMPTY: &str = r#"
[workflow]
name = "E"

[trigger]
type = "manual"
"#;
        let wf = parse_workflow_text(WF_EMPTY).unwrap();
        let d = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_at: None,
        };
        let err = run_workflow(&wf, &d).await.unwrap_err();
        assert!(matches!(err, WorkflowExecutionError::EmptyPlan(_)));
    }

    #[tokio::test]
    async fn variables_are_interpolated_into_step_extras() {
        use crate::Step;
        use std::sync::Mutex;

        struct CapturingDispatcher {
            seen: Mutex<Vec<Step>>,
        }
        #[async_trait]
        impl ActionDispatcher for CapturingDispatcher {
            async fn run(&self, step: &Step) -> Result<serde_json::Value, String> {
                self.seen.lock().unwrap().push(step.clone());
                Ok(serde_json::json!({}))
            }
        }

        const WF: &str = r#"
[workflow]
name = "V"

[trigger]
type = "manual"

[[steps]]
type = "ipc"
target = "com.nexus.storage"
command = "read_file"
[steps.args]
path = "${trigger.path}"
"#;
        let wf = parse_workflow_text(WF).unwrap();
        let d = CapturingDispatcher {
            seen: Mutex::new(Vec::new()),
        };
        let mut vars = VariableMap::new();
        vars.insert(
            "trigger.path".into(),
            toml::Value::String("notes/a.md".into()),
        );
        run_workflow_with_variables(&wf, &d, &vars).await.unwrap();

        let seen = d.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let path = seen[0]
            .extra
            .get("args")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("path"))
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(path, "notes/a.md");
    }

    #[tokio::test]
    async fn empty_variables_does_not_touch_steps() {
        // Regression: if vars is empty we skip the clone+walk entirely.
        // Dispatcher sees the step unchanged.
        let wf = parse_workflow_text(THREE_STEPS).unwrap();
        let d = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_at: None,
        };
        let run = run_workflow_with_variables(&wf, &d, &VariableMap::new())
            .await
            .unwrap();
        assert!(run.success);
        assert_eq!(run.steps.len(), 3);
    }

    /// Dispatcher that fails its first `fail_count` calls, then succeeds.
    struct FlakyDispatcher {
        calls: Arc<AtomicUsize>,
        fail_count: usize,
    }

    #[async_trait]
    impl ActionDispatcher for FlakyDispatcher {
        async fn run(&self, _step: &Step) -> Result<serde_json::Value, String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_count {
                Err("flake".into())
            } else {
                Ok(serde_json::json!({ "ok": true }))
            }
        }
    }

    /// Dispatcher that records each sleep delta the executor scheduled
    /// between attempts and always fails.
    struct AlwaysFailDispatcher {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ActionDispatcher for AlwaysFailDispatcher {
        async fn run(&self, _step: &Step) -> Result<serde_json::Value, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err("always".into())
        }
    }

    fn retry_workflow(toml_src: &str) -> Workflow {
        parse_workflow_text(toml_src).unwrap()
    }

    #[tokio::test]
    async fn succeeds_after_two_failures_with_max_retries_3() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
max_retries = 3
retry_initial_delay_ms = 0
retry_jitter = false
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = FlakyDispatcher {
            calls: Arc::clone(&calls),
            fail_count: 2,
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(run.success);
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].status, StepOutcomeStatus::Ok);
        assert_eq!(run.steps[0].attempts, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn exhausts_retries_then_fails() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
max_retries = 2
retry_initial_delay_ms = 0
retry_jitter = false
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = AlwaysFailDispatcher {
            calls: Arc::clone(&calls),
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(!run.success);
        assert_eq!(run.steps[0].status, StepOutcomeStatus::Failed);
        assert_eq!(run.steps[0].attempts, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn max_retries_zero_runs_once() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = AlwaysFailDispatcher {
            calls: Arc::clone(&calls),
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(!run.success);
        assert_eq!(run.steps[0].status, StepOutcomeStatus::Failed);
        assert_eq!(run.steps[0].attempts, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn constant_backoff_uses_base_each_attempt() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
max_retries = 3
retry_backoff = "constant"
retry_initial_delay_ms = 250
retry_jitter = false
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = AlwaysFailDispatcher {
            calls: Arc::clone(&calls),
        };
        let start = tokio::time::Instant::now();
        let run = run_workflow(&wf, &d).await.unwrap();
        let elapsed = start.elapsed();
        // 3 retries -> 3 sleeps of 250ms each = 750ms
        assert_eq!(run.steps[0].attempts, 4);
        assert!(
            elapsed >= std::time::Duration::from_millis(750),
            "expected at least 750ms of paused-time elapsed, got {elapsed:?}"
        );
        // Should not be wildly larger; 3 sleeps shouldn't push past
        // 1500ms in a paused clock.
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "elapsed exceeded constant-backoff total: {elapsed:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn exponential_caps_at_retry_max_delay_ms() {
        // base 1000ms, cap 1500ms. 4 retries -> raw delays would be
        // 1000, 2000, 4000, 8000ms; capped to 1000, 1500, 1500, 1500
        // (= 5500ms total). Without the cap it'd be 15000ms.
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
max_retries = 4
retry_backoff = "exponential"
retry_initial_delay_ms = 1000
retry_max_delay_ms = 1500
retry_jitter = false
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = AlwaysFailDispatcher {
            calls: Arc::clone(&calls),
        };
        let start = tokio::time::Instant::now();
        run_workflow(&wf, &d).await.unwrap();
        let elapsed = start.elapsed();
        // Expected total = 1000 + 1500 + 1500 + 1500 = 5500ms.
        assert!(
            elapsed >= std::time::Duration::from_millis(5500),
            "want >= 5500ms, got {elapsed:?}"
        );
        // Comfortably under the un-capped 15000ms.
        assert!(
            elapsed < std::time::Duration::from_millis(7000),
            "cap not honored, got {elapsed:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn workflow_level_error_handling_supplies_default() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[error_handling]
max_retries = 2

[[steps]]
type = "noop"
retry_initial_delay_ms = 0
retry_jitter = false
"#;
        let wf = retry_workflow(src);
        let calls = Arc::new(AtomicUsize::new(0));
        let d = FlakyDispatcher {
            calls: Arc::clone(&calls),
            fail_count: 2,
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert!(run.success);
        assert_eq!(run.steps[0].attempts, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn step_name_is_used_as_step_id_when_present() {
        const WF_NAMED: &str = r#"
[workflow]
name = "N"

[trigger]
type = "manual"

[[steps]]
name = "StepA"
type = "noop"
"#;
        let wf = parse_workflow_text(WF_NAMED).unwrap();
        let d = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_at: None,
        };
        let run = run_workflow(&wf, &d).await.unwrap();
        assert_eq!(run.steps[0].step_id, "StepA");
    }
}
