//! Core plugin wrapping the agent library.
//!
//! Registers as `com.nexus.agent`. Holds a [`KernelPluginContext`]
//! (supplied via [`CorePlugin::wire_context`] at bootstrap) so its
//! handlers can drive two bridges against the live runtime:
//!
//! - [`nexus_bootstrap::agent::AiChatDriver`]-shaped adapter over
//!   `com.nexus.ai::stream_chat` for planning.
//! - [`nexus_bootstrap::agent::KernelToolDispatcher`]-shaped adapter
//!   over `PluginContext::ipc_call` for executing plan steps.
//!
//! Because this module lives in `nexus-agent`, it re-implements the
//! two adapter shapes locally — keeping the library itself
//! kernel-free would otherwise force a circular dep on
//! `nexus-bootstrap`. The bridges here and in bootstrap are
//! intentionally identical in behaviour.
//!
//! # Handlers
//!
//! | Handler id | Command    | Purpose                            |
//! |-----------:|------------|------------------------------------|
//! | 1          | `plan`     | Produce a [`Plan`] from a goal     |
//! | 2          | `run`      | Plan + execute; return Observation |
//! | 3          | `run_plan` | Execute a preset [`Plan`]          |
//!
//! Ids are append-only.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Deserialize;

use crate::{
    Agent, AgentError, ChatDriver, LlmAgent, Plan, PlanExecutor, ToolCall, ToolDispatcher,
};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.agent";

/// `plan` handler id — produce a plan for the given goal.
pub const HANDLER_PLAN: u32 = 1;
/// `run` handler id — plan + execute in one call.
pub const HANDLER_RUN: u32 = 2;
/// `run_plan` handler id — execute a preset plan.
pub const HANDLER_RUN_PLAN: u32 = 3;

/// Default per-tool-call timeout used by the executor when no
/// caller-provided override lands. Matches the bootstrap bridge.
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// Default chat timeout; planner prompts can cost remote-provider
/// latency. Matches the bootstrap bridge.
const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

/// Core plugin instance.
pub struct AgentCorePlugin {
    context: Option<Arc<KernelPluginContext>>,
}

impl AgentCorePlugin {
    /// Construct an unwired plugin. Bootstrap must call
    /// [`CorePlugin::wire_context`] before the first dispatch; any
    /// handler that fires before then returns a clear error.
    #[must_use]
    pub fn new() -> Self {
        Self { context: None }
    }
}

impl Default for AgentCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AgentCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: agent commands are async; caller should use dispatch_async"
            ),
        })
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        let ctx = self.context.clone();
        let args = args.clone();
        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err("agent plugin context not wired (bootstrap incomplete)".into())
            })?;
            match handler_id {
                HANDLER_PLAN => handle_plan(ctx, &args).await,
                HANDLER_RUN => handle_run(ctx, &args).await,
                HANDLER_RUN_PLAN => handle_run_plan(ctx, &args).await,
                other => Err(exec_err(format!("unknown handler id {other}"))),
            }
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
    }
}

// ── Handler impls ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GoalArgs {
    goal: String,
}

#[derive(Deserialize)]
struct PlanArgs {
    plan: Plan,
}

async fn handle_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "plan")?;
    let agent = build_llm_agent(Arc::clone(&ctx));
    let plan = agent.plan(&a.goal).await.map_err(agent_err)?;
    to_value(&plan, "plan")
}

async fn handle_run(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "run")?;
    let agent = build_llm_agent(Arc::clone(&ctx));
    let plan = agent.plan(&a.goal).await.map_err(agent_err)?;
    run_plan_internal(ctx, plan).await
}

async fn handle_run_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: PlanArgs = parse(args, "run_plan")?;
    run_plan_internal(ctx, a.plan).await
}

async fn run_plan_internal(
    ctx: Arc<KernelPluginContext>,
    plan: Plan,
) -> Result<serde_json::Value, PluginError> {
    let executor = PlanExecutor::new(KernelToolBridge {
        ctx,
        timeout: DEFAULT_TOOL_TIMEOUT,
    });
    let observation = executor.run(&plan).await.map_err(agent_err)?;
    to_value(&observation, "run")
}

// ── Local adapters mirroring nexus-bootstrap::agent ────────────────────────

fn build_llm_agent(ctx: Arc<KernelPluginContext>) -> LlmAgent<AiChatBridge> {
    LlmAgent::new(AiChatBridge {
        ctx,
        timeout: DEFAULT_CHAT_TIMEOUT,
    })
}

struct AiChatBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ChatDriver for AiChatBridge {
    async fn chat(&self, system: &str, user_message: &str) -> Result<String, String> {
        #[derive(Deserialize)]
        struct ChatResp {
            #[serde(default)]
            text: String,
        }
        let args = serde_json::json!({
            "messages": [{ "role": "user", "content": user_message }],
            "system": system,
        });
        let raw = self
            .ctx
            .ipc_call("com.nexus.ai", "stream_chat", args, self.timeout)
            .await
            .map_err(|e| e.to_string())?;
        let parsed: ChatResp = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        Ok(parsed.text)
    }
}

struct KernelToolBridge {
    ctx: Arc<KernelPluginContext>,
    timeout: Duration,
}

#[async_trait]
impl ToolDispatcher for KernelToolBridge {
    async fn dispatch(&self, call: &ToolCall) -> Result<serde_json::Value, String> {
        self.ctx
            .ipc_call(
                &call.target_plugin_id,
                &call.command_id,
                call.args.clone(),
                self.timeout,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

// ── Error / serde plumbing ──────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn agent_err(e: AgentError) -> PluginError {
    exec_err(e.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize: {e}")))
}
