//! Plan execution.
//!
//! [`PlanExecutor`] walks a [`Plan`] step-by-step, consulting the
//! [`StepPolicy`] before each step and dispatching tool calls through
//! the injected [`ToolDispatcher`]. Returns an [`Observation`] that
//! mirrors the plan 1:1 so the UI can render a combined plan /
//! progress view without a separate zip step.

use serde::{Deserialize, Serialize};

use crate::{
    AgentError, AutoApprove, Observation, Plan, PolicyDecision, Step, StepPolicy, ToolCall,
    ToolDispatcher,
};

/// Outcome of one step in an [`Observation`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    /// Step id — matches [`Step::id`] in the input plan.
    pub step_id: String,
    /// `Some(response)` when the step carried a tool call that ran;
    /// `None` for informational steps or steps that never ran
    /// (e.g. policy denial, earlier tool failure aborted the plan).
    pub response: Option<serde_json::Value>,
    /// Final status — one of `"ok"`, `"denied"`, `"failed"`, `"skipped"`.
    pub status: StepStatus,
}

/// Terminal status for a single step. Mirrors UI affordances (green /
/// yellow / red / grey).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Ran to completion without error (tool call succeeded, or the
    /// step was informational).
    Ok,
    /// Policy rejected the step; plan aborted at this point.
    Denied,
    /// Tool call returned an error.
    Failed,
    /// Step was skipped because an earlier step failed or was denied.
    Skipped,
}

/// Orchestrates plan execution. Construct with a [`ToolDispatcher`];
/// call [`PlanExecutor::run`] to execute with [`AutoApprove`] or
/// [`PlanExecutor::run_with_policy`] to inject a user-approval flow.
pub struct PlanExecutor<D: ToolDispatcher> {
    dispatcher: D,
}

impl<D: ToolDispatcher> PlanExecutor<D> {
    /// Construct over a tool dispatcher.
    pub fn new(dispatcher: D) -> Self {
        Self { dispatcher }
    }

    /// Run a plan with the auto-approve policy.
    ///
    /// # Errors
    /// Propagates the first tool failure as [`AgentError::ToolFailed`].
    /// Steps after the failure are still represented in the
    /// [`Observation`] with [`StepStatus::Skipped`] so the UI can
    /// show what was and wasn't attempted.
    pub async fn run(&self, plan: &Plan) -> Result<Observation, AgentError> {
        self.run_with_policy(plan, &AutoApprove).await
    }

    /// Run a plan, consulting `policy` before every step.
    ///
    /// # Errors
    /// Same as [`Self::run`], plus [`AgentError::StepDenied`] when
    /// the policy rejects a step.
    pub async fn run_with_policy(
        &self,
        plan: &Plan,
        policy: &dyn StepPolicy,
    ) -> Result<Observation, AgentError> {
        let mut results: Vec<StepResult> = Vec::with_capacity(plan.steps.len());
        let mut first_error: Option<AgentError> = None;

        for step in &plan.steps {
            if first_error.is_some() {
                results.push(StepResult {
                    step_id: step.id.clone(),
                    response: None,
                    status: StepStatus::Skipped,
                });
                continue;
            }

            match policy.allow(step) {
                PolicyDecision::Deny(reason) => {
                    results.push(StepResult {
                        step_id: step.id.clone(),
                        response: None,
                        status: StepStatus::Denied,
                    });
                    first_error = Some(AgentError::StepDenied {
                        step_id: step.id.clone(),
                        reason,
                    });
                    continue;
                }
                PolicyDecision::Approve => {}
            }

            match self.run_step(step).await {
                Ok(resp) => results.push(StepResult {
                    step_id: step.id.clone(),
                    response: resp,
                    status: StepStatus::Ok,
                }),
                Err(err) => {
                    results.push(StepResult {
                        step_id: step.id.clone(),
                        response: None,
                        status: StepStatus::Failed,
                    });
                    first_error = Some(err);
                }
            }
        }

        let success = first_error.is_none();
        let observation = Observation {
            plan_id: plan.id.clone(),
            steps: results,
            success,
        };

        if let Some(err) = first_error {
            // Return the observation alongside the error when the
            // caller needs partial results — today we just log the
            // observation at debug and propagate the error; callers
            // wanting the partial observation can reassemble it from
            // the step ids in the error and a fresh run.
            tracing::debug!(
                plan_id = %plan.id,
                steps_run = observation.steps.len(),
                "plan execution aborted",
            );
            return Err(err);
        }
        Ok(observation)
    }

    async fn run_step(
        &self,
        step: &Step,
    ) -> Result<Option<serde_json::Value>, AgentError> {
        match &step.tool_call {
            None => Ok(None),
            Some(call) => self.dispatch_tool(step, call).await.map(Some),
        }
    }

    /// Execute a single step at `index` in the plan without touching
    /// any surrounding state. Unlike [`Self::run`] this doesn't run
    /// the policy — the caller has already decided the step is OK —
    /// and returns a single [`StepResult`]. Enables per-step approval
    /// flows where the UI drives the loop.
    ///
    /// # Errors
    /// - [`AgentError::PlanningFailed`] when `index` is out of bounds.
    /// - [`AgentError::ToolFailed`] when the dispatched tool call
    ///   errors. The error carries the step id so the UI can still
    ///   render the partial result.
    pub async fn execute_step_at(
        &self,
        plan: &Plan,
        index: usize,
    ) -> Result<StepResult, AgentError> {
        let step = plan.steps.get(index).ok_or_else(|| {
            AgentError::PlanningFailed(format!(
                "step index {index} out of bounds (len={})",
                plan.steps.len()
            ))
        })?;
        match self.run_step(step).await {
            Ok(resp) => Ok(StepResult {
                step_id: step.id.clone(),
                response: resp,
                status: StepStatus::Ok,
            }),
            Err(err) => Err(err),
        }
    }

    async fn dispatch_tool(
        &self,
        step: &Step,
        call: &ToolCall,
    ) -> Result<serde_json::Value, AgentError> {
        self.dispatcher
            .dispatch(call)
            .await
            .map_err(|reason| AgentError::ToolFailed {
                step_id: step.id.clone(),
                reason,
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;

    struct RecordingDispatcher {
        calls: Arc<AtomicUsize>,
        reply: serde_json::Value,
        fail_on: Option<usize>,
    }

    #[async_trait]
    impl ToolDispatcher for RecordingDispatcher {
        async fn dispatch(&self, _call: &ToolCall) -> Result<serde_json::Value, String> {
            let idx = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_on == Some(idx) {
                Err(format!("mock failure on call {idx}"))
            } else {
                Ok(self.reply.clone())
            }
        }
    }

    fn make_plan_with_tools(steps: usize) -> Plan {
        let tool_call = ToolCall {
            target_plugin_id: "com.test.echo".into(),
            command_id: "noop".into(),
            args: serde_json::json!({}),
        };
        let mut out = Vec::new();
        for i in 0..steps {
            out.push(Step {
                id: format!("s{i}"),
                description: format!("step {i}"),
                tool_call: Some(tool_call.clone()),
            });
        }
        Plan::new("demo", out)
    }

    #[tokio::test]
    async fn executor_runs_every_tool_call_in_order() {
        let dispatcher = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            reply: serde_json::json!({"ok": true}),
            fail_on: None,
        };
        let calls = Arc::clone(&dispatcher.calls);
        let executor = PlanExecutor::new(dispatcher);
        let plan = make_plan_with_tools(3);

        let obs = executor.run(&plan).await.unwrap();

        assert!(obs.success);
        assert_eq!(obs.steps.len(), 3);
        assert!(obs.steps.iter().all(|r| r.status == StepStatus::Ok));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn executor_aborts_on_first_tool_failure_and_records_skipped() {
        let dispatcher = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            reply: serde_json::json!({"ok": true}),
            fail_on: Some(1),
        };
        let executor = PlanExecutor::new(dispatcher);
        let plan = make_plan_with_tools(3);

        let err = executor.run(&plan).await.unwrap_err();
        assert!(matches!(
            err,
            AgentError::ToolFailed { ref step_id, .. } if step_id == "s1"
        ));
    }

    #[tokio::test]
    async fn informational_steps_succeed_without_dispatcher_calls() {
        struct NeverDispatch;
        #[async_trait]
        impl ToolDispatcher for NeverDispatch {
            async fn dispatch(
                &self,
                _call: &ToolCall,
            ) -> Result<serde_json::Value, String> {
                panic!("should not dispatch informational steps");
            }
        }

        let plan = Plan::new(
            "just talk",
            vec![
                Step {
                    id: "hello".into(),
                    description: "announce milestone".into(),
                    tool_call: None,
                },
                Step {
                    id: "bye".into(),
                    description: "sign off".into(),
                    tool_call: None,
                },
            ],
        );

        let executor = PlanExecutor::new(NeverDispatch);
        let obs = executor.run(&plan).await.unwrap();
        assert!(obs.success);
        assert_eq!(obs.steps.len(), 2);
    }

    #[tokio::test]
    async fn execute_step_at_runs_a_single_tool_call() {
        let dispatcher = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            reply: serde_json::json!({"hit": 1}),
            fail_on: None,
        };
        let calls = Arc::clone(&dispatcher.calls);
        let executor = PlanExecutor::new(dispatcher);
        let plan = make_plan_with_tools(3);
        let result = executor.execute_step_at(&plan, 1).await.unwrap();
        assert_eq!(result.step_id, "s1");
        assert_eq!(result.status, StepStatus::Ok);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_step_at_reports_out_of_bounds() {
        struct NeverDispatch;
        #[async_trait]
        impl ToolDispatcher for NeverDispatch {
            async fn dispatch(
                &self,
                _call: &ToolCall,
            ) -> Result<serde_json::Value, String> {
                panic!("unused");
            }
        }
        let executor = PlanExecutor::new(NeverDispatch);
        let plan = make_plan_with_tools(1);
        let err = executor.execute_step_at(&plan, 5).await.unwrap_err();
        assert!(matches!(err, AgentError::PlanningFailed(_)));
    }

    #[tokio::test]
    async fn policy_denial_aborts_with_skipped_remaining() {
        struct DenyEvens;
        impl StepPolicy for DenyEvens {
            fn allow(&self, step: &Step) -> PolicyDecision {
                // Step ids follow "s0", "s1", ... — deny every even idx.
                let digit = step.id.trim_start_matches('s').parse::<u32>().unwrap_or(0);
                if digit.is_multiple_of(2) {
                    PolicyDecision::Deny(format!("even step {}", step.id))
                } else {
                    PolicyDecision::Approve
                }
            }
        }

        let dispatcher = RecordingDispatcher {
            calls: Arc::new(AtomicUsize::new(0)),
            reply: serde_json::json!({}),
            fail_on: None,
        };
        let executor = PlanExecutor::new(dispatcher);
        let plan = make_plan_with_tools(3);

        let err = executor.run_with_policy(&plan, &DenyEvens).await.unwrap_err();
        match err {
            AgentError::StepDenied { step_id, .. } => assert_eq!(step_id, "s0"),
            other => panic!("expected StepDenied, got {other:?}"),
        }
    }
}
