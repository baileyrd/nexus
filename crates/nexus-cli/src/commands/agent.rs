//! Agent command handlers — `nexus agent plan|run`.
//!
//! Both drive through `com.nexus.agent` via `ipc_call`; the CLI
//! never links `nexus-agent` directly. Matches the shape of
//! `commands::ai` so agents and AI share a consistent surface.
//!
//! Per ADR 0025 Phase 1, `run` now drives `session_run`
//! (auto-approve mode) and renders the resulting transcript.
//! `run-plan` was removed — there's no session-model equivalent
//! for replaying a static plan.

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

/// `nexus agent plan <goal> [--archetype ..]` — produce a plan without executing.
pub fn plan(app: &mut App, goal: &str, archetype: Option<&str>) -> Result<()> {
    let response = call(app, "plan", goal_args(goal, archetype))?;
    print_plan(&response);
    Ok(())
}

/// `nexus agent run <goal> [--archetype ..]` — drive a session
/// end-to-end with auto-approve, then render the transcript.
pub fn run(app: &mut App, goal: &str, archetype: Option<&str>) -> Result<()> {
    let mut args = goal_args(goal, archetype);
    if let Some(map) = args.as_object_mut() {
        map.insert("auto_approve".into(), Value::Bool(true));
    }
    let response = call(app, "session_run", args)?;
    print_session(&response);
    Ok(())
}

/// `nexus agent list-custom` — list `.agent.toml` manifests under
/// `<forge>/.forge/agents/`. PRD-15 §9 (DG-36).
///
/// # Errors
/// Surfaces IPC dispatch errors verbatim.
pub fn list_custom(app: &mut App) -> Result<()> {
    let response = call(app, "list_custom", Value::Object(serde_json::Map::new()))?;
    let manifests = response
        .get("manifests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let errors = response
        .get("errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if manifests.is_empty() && errors.is_empty() {
        println!("(no custom agents under .forge/agents/)");
        return Ok(());
    }

    if !manifests.is_empty() {
        let slug_w = manifests
            .iter()
            .filter_map(|m| m.get("slug").and_then(Value::as_str))
            .map(str::len)
            .max()
            .unwrap_or(4)
            .max("SLUG".len());
        println!("{:width$}  NAME                 ARCHETYPE", "SLUG", width = slug_w);
        for m in &manifests {
            let slug = m.get("slug").and_then(Value::as_str).unwrap_or("?");
            let name = m
                .get("agent")
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("(unnamed)");
            let arche = m
                .get("agent")
                .and_then(|a| a.get("archetype"))
                .and_then(Value::as_str)
                .unwrap_or("-");
            println!("{slug:width$}  {name:<20} {arche}", width = slug_w);
        }
    }
    if !errors.is_empty() {
        if !manifests.is_empty() {
            println!();
        }
        println!("errors ({}):", errors.len());
        for e in errors {
            let path = e.get("path").and_then(Value::as_str).unwrap_or("?");
            let msg = e.get("error").and_then(Value::as_str).unwrap_or("?");
            println!("  {path}: {msg}");
        }
    }
    Ok(())
}

fn goal_args(goal: &str, archetype: Option<&str>) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("goal".into(), Value::String(goal.into()));
    if let Some(a) = archetype {
        map.insert("archetype".into(), Value::String(a.into()));
    }
    Value::Object(map)
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

fn print_session(session: &Value) {
    let id = session.get("id").and_then(Value::as_str).unwrap_or("<no-id>");
    let goal = session
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("<no-goal>");
    let outcome = session
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let rounds = session
        .get("rounds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Session : {id}");
    println!("Goal    : {goal}");
    println!("Outcome : {outcome}");
    println!("Rounds  : {}", rounds.len());
    for round in &rounds {
        let n = round.get("round").and_then(Value::as_u64).unwrap_or(0);
        let text = round.get("text").and_then(Value::as_str).unwrap_or("");
        let calls = round
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        println!("  round {n}");
        if !text.is_empty() {
            for line in text.lines() {
                println!("    {line}");
            }
        }
        for tc in &calls {
            let name = tc.get("name").and_then(Value::as_str).unwrap_or("?");
            let approved = tc.get("approved").and_then(Value::as_bool).unwrap_or(false);
            let error = tc.get("error").and_then(Value::as_str).unwrap_or("");
            let marker = if !error.is_empty() {
                "✗"
            } else if approved {
                "✓"
            } else {
                "·"
            };
            print!("    {marker} {name}");
            if !error.is_empty() {
                print!(" — {error}");
            } else if let Some(resp) = tc.get("response").filter(|v| !v.is_null()) {
                let preview = preview_json(resp, 160);
                print!(" — {preview}");
            }
            println!();
        }
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
