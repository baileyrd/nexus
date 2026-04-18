//! Tauri command bridge into the agent core plugin.
//!
//! Thin adapter: serializes args to JSON and calls
//! [`nexus_kernel::PluginContext::ipc_call`] on `com.nexus.agent`.
//! Agent runs can chain many LLM + tool calls together so the timeout
//! is generous.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

const AGENT_PLUGIN_ID: &str = "com.nexus.agent";

/// 10 minutes — enough for a multi-step plan against remote LLM
/// providers while still bounding runaway sessions.
const AGENT_CALL_TIMEOUT: Duration = Duration::from_secs(600);

async fn call_agent(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(AGENT_PLUGIN_ID, command, args, AGENT_CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

fn goal_args(goal: &str, archetype: Option<&str>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("goal".into(), serde_json::Value::String(goal.into()));
    if let Some(a) = archetype.filter(|s| !s.is_empty()) {
        map.insert(
            "archetype".into(),
            serde_json::Value::String(a.into()),
        );
    }
    serde_json::Value::Object(map)
}

/// Produce a plan for a goal without executing it. Returns the Plan
/// JSON as-is.
#[tauri::command]
pub async fn agent_plan(
    goal: String,
    archetype: Option<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_agent(runtime, "plan", goal_args(&goal, archetype.as_deref())).await
}

/// Plan + execute a goal end-to-end. Returns the Observation.
#[tauri::command]
pub async fn agent_run(
    goal: String,
    archetype: Option<String>,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_agent(runtime, "run", goal_args(&goal, archetype.as_deref())).await
}

/// Execute a preset plan (produced by [`agent_plan`]) and return its
/// Observation.
#[tauri::command]
pub async fn agent_run_plan(
    plan: serde_json::Value,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_agent(runtime, "run_plan", serde_json::json!({ "plan": plan })).await
}
