//! Bundled agent impls. Currently just [`EchoAgent`] for scaffolding
//! + tests; real archetypes (Writer / Coder / Researcher) land in
//! their own modules once the LLM-driven planner surface settles.

use async_trait::async_trait;

use crate::{Agent, AgentError, Plan, Step};

/// Trivial agent that returns a one-step plan containing the goal as
/// the step description and no tool call. Useful for smoke tests and
/// for integration scaffolding that needs a predictable shape.
pub struct EchoAgent {
    id: String,
}

impl EchoAgent {
    /// Construct with the default id `"com.nexus.agent.echo"`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: "com.nexus.agent.echo".into(),
        }
    }

    /// Construct with a custom id. Useful in tests that want two
    /// distinguishable agents.
    #[must_use]
    pub fn with_id(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Default for EchoAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for EchoAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn plan(&self, goal: &str) -> Result<Plan, AgentError> {
        let goal = goal.trim();
        if goal.is_empty() {
            return Err(AgentError::PlanningFailed("empty goal".into()));
        }
        let steps = vec![Step {
            id: "echo-1".into(),
            description: goal.to_string(),
            tool_call: None,
        }];
        Ok(Plan::new(goal, steps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_agent_returns_one_step_plan_with_goal() {
        let agent = EchoAgent::new();
        let plan = agent.plan("write a haiku").await.unwrap();
        assert_eq!(plan.goal, "write a haiku");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].description, "write a haiku");
        assert!(plan.steps[0].tool_call.is_none());
    }

    #[tokio::test]
    async fn echo_agent_rejects_empty_goal() {
        let agent = EchoAgent::new();
        assert!(matches!(
            agent.plan("   ").await.unwrap_err(),
            AgentError::PlanningFailed(_)
        ));
    }

    #[tokio::test]
    async fn echo_agent_honours_custom_id() {
        let agent = EchoAgent::with_id("test.agent.alpha");
        assert_eq!(agent.id(), "test.agent.alpha");
    }
}
