//! Agent command handlers — `nexus agent plan|run|run-plan`.
//!
//! All three drive through `com.nexus.agent` via `ipc_call`; the CLI
//! never links `nexus-agent` directly. Matches the shape of
//! `commands::ai` so agents and AI share a consistent surface.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const AGENT_PLUGIN: &str = "com.nexus.agent";

/// Planning is usually a single chat round-trip (~30 s tops against
/// remote providers). Run can string many tool calls together so the
/// two share the generous upper bound the AI CLI already uses.
const IPC_TIMEOUT: Duration = Duration::from_secs(600);

/// `nexus agent plan <goal>` — produce a plan without executing it.
pub fn plan(app: &mut App, goal: &str) -> Result<()> {
    let response = call(app, "plan", serde_json::json!({ "goal": goal }))?;
    print_plan(&response);
    Ok(())
}

/// `nexus agent run <goal>` — plan + execute, print per-step outcomes.
pub fn run(app: &mut App, goal: &str) -> Result<()> {
    let response = call(app, "run", serde_json::json!({ "goal": goal }))?;
    print_observation(&response);
    Ok(())
}

/// `nexus agent run-plan <file.json>` — execute a preset plan loaded
/// from disk. Useful for replaying a plan produced by `plan` earlier.
pub fn run_plan(app: &mut App, plan_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(plan_path)
        .with_context(|| format!("reading plan from {plan_path}"))?;
    let plan: Value = serde_json::from_str(&raw)
        .with_context(|| format!("plan file is not valid JSON: {plan_path}"))?;
    let response = call(app, "run_plan", serde_json::json!({ "plan": plan }))?;
    print_observation(&response);
    Ok(())
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_plan(plan: &Value) {
    let id = plan.get("id").and_then(Value::as_str).unwrap_or("<no-id>");
    let goal = plan
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("<no-goal>");
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Plan  : {id}");
    println!("Goal  : {goal}");
    println!("Steps : {}", steps.len());
    for (i, step) in steps.iter().enumerate() {
        let desc = step
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("<no-description>");
        let sid = step.get("id").and_then(Value::as_str).unwrap_or("");
        print!("  {}. [{sid}] {desc}", i + 1);
        if let Some(call) = step.get("tool_call").filter(|v| !v.is_null()) {
            let target = call
                .get("target_plugin_id")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let cmd = call.get("command_id").and_then(Value::as_str).unwrap_or("?");
            print!(" → {target}.{cmd}");
        }
        println!();
    }
}

fn print_observation(obs: &Value) {
    let plan_id = obs
        .get("plan_id")
        .and_then(Value::as_str)
        .unwrap_or("<no-id>");
    let success = obs
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let steps = obs
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Plan  : {plan_id}");
    println!("State : {}", if success { "success" } else { "partial / failed" });
    for step in &steps {
        let sid = step.get("step_id").and_then(Value::as_str).unwrap_or("?");
        let status = step
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        print!("  [{status}] {sid}");
        if let Some(resp) = step.get("response").filter(|v| !v.is_null()) {
            let preview = preview_json(resp, 160);
            print!(" — {preview}");
        }
        println!();
    }
}

fn preview_json(v: &Value, max: usize) -> String {
    let full = v.to_string();
    if full.len() <= max {
        full
    } else {
        let mut end = max;
        while !full.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &full[..end])
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(
        runtime
            .context
            .ipc_call(AGENT_PLUGIN, command, args, IPC_TIMEOUT),
    )
    .with_context(|| format!("agent ipc call '{command}' failed"))
}
