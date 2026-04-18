//! Workflow command handlers — `nexus workflow list|show|reload|validate`.
//!
//! All dispatch through `com.nexus.workflow` via `ipc_call`; no
//! direct `nexus-workflow` linkage.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const WORKFLOW_PLUGIN: &str = "com.nexus.workflow";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
/// Run timeout — workflows chain many plugin calls so give them more headroom.
const RUN_TIMEOUT: Duration = Duration::from_secs(600);

/// `nexus workflow list` — every loaded workflow.
pub fn list(app: &mut App) -> Result<()> {
    let response = call(app, "list", Value::Object(serde_json::Map::new()))?;
    print_summary_table(&response);
    Ok(())
}

/// `nexus workflow show <name>` — full metadata for one workflow.
pub fn show(app: &mut App, name: &str) -> Result<()> {
    let response = call(app, "get", serde_json::json!({ "name": name }))?;
    print_full(&response);
    Ok(())
}

/// `nexus workflow run <name>` — execute a loaded workflow end-to-end.
pub fn run(app: &mut App, name: &str) -> Result<()> {
    let (runtime, rt) = app.runtime()?;
    let response = rt
        .block_on(
            runtime
                .context
                .ipc_call(
                    WORKFLOW_PLUGIN,
                    "run",
                    serde_json::json!({ "name": name }),
                    RUN_TIMEOUT,
                ),
        )
        .with_context(|| format!("workflow run '{name}' failed"))?;
    print_run(&response);
    Ok(())
}

fn print_run(run: &Value) {
    let name = run
        .get("workflow_name")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let success = run
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let steps = run
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Workflow : {name}");
    println!(
        "State    : {}",
        if success {
            "success"
        } else {
            "partial / failed"
        }
    );
    for step in &steps {
        let sid = step.get("step_id").and_then(Value::as_str).unwrap_or("?");
        let stype = step.get("step_type").and_then(Value::as_str).unwrap_or("?");
        let status = step.get("status").and_then(Value::as_str).unwrap_or("?");
        let badge = match status {
            "ok" => "✓",
            "failed" => "✗",
            "skipped" => "·",
            _ => "?",
        };
        let err = step.get("error").and_then(Value::as_str).unwrap_or("");
        if err.is_empty() {
            println!("  {badge} [{status}] {sid} ({stype})");
        } else {
            println!("  {badge} [{status}] {sid} ({stype}) — {err}");
        }
    }
}

/// `nexus workflow reload` — re-scan the `.workflows/` directory.
pub fn reload(app: &mut App) -> Result<()> {
    let response = call(app, "reload", Value::Object(serde_json::Map::new()))?;
    let n = response.get("loaded").and_then(Value::as_u64).unwrap_or(0);
    println!("Reloaded {n} workflow(s).");
    Ok(())
}

/// `nexus workflow validate <file>` — parse + validate a TOML file.
pub fn validate(app: &mut App, path: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading workflow from {path}"))?;
    match call(app, "validate", serde_json::json!({ "text": text })) {
        Ok(value) => {
            let name = value
                .get("workflow")
                .and_then(|w| w.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let trigger = value
                .get("trigger")
                .and_then(|t| t.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let steps = value
                .get("steps")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            println!("OK  name={name}  trigger={trigger}  steps={steps}");
            Ok(())
        }
        Err(err) => {
            eprintln!("INVALID  {err}");
            std::process::exit(1);
        }
    }
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_summary_table(response: &Value) {
    let workflows = match response.as_array() {
        Some(arr) => arr,
        None => {
            eprintln!("unexpected response shape: {response}");
            return;
        }
    };
    if workflows.is_empty() {
        println!("(no workflows)");
        return;
    }
    let name_w = workflows
        .iter()
        .filter_map(|w| w.get("workflow").and_then(|m| m.get("name")).and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    let trig_w = workflows
        .iter()
        .filter_map(|w| w.get("trigger").and_then(|m| m.get("type")).and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(7)
        .max(7);
    println!(
        "{:<name_w$}  {:<trig_w$}  {}",
        "NAME",
        "TRIGGER",
        "STEPS",
        name_w = name_w,
        trig_w = trig_w,
    );
    for w in workflows {
        let name = w
            .get("workflow")
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        let trig = w
            .get("trigger")
            .and_then(|m| m.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        let steps = w
            .get("steps")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        println!(
            "{:<name_w$}  {:<trig_w$}  {}",
            name,
            trig,
            steps,
            name_w = name_w,
            trig_w = trig_w,
        );
    }
}

fn print_full(wf: &Value) {
    let name = wf
        .get("workflow")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let desc = wf
        .get("workflow")
        .and_then(|m| m.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let trig = wf
        .get("trigger")
        .and_then(|m| m.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Workflow : {name}");
    if !desc.is_empty() {
        println!("           {desc}");
    }
    println!("Trigger  : {trig}");
    if let Some(steps) = wf.get("steps").and_then(Value::as_array) {
        println!("Steps    : {}", steps.len());
        for (i, s) in steps.iter().enumerate() {
            let sname = s.get("name").and_then(Value::as_str).unwrap_or("");
            let stype = s.get("type").and_then(Value::as_str).unwrap_or("?");
            println!("  {}. [{stype}] {sname}", i + 1);
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(
        runtime
            .context
            .ipc_call(WORKFLOW_PLUGIN, command, args, IPC_TIMEOUT),
    )
    .with_context(|| format!("workflow ipc call '{command}' failed"))
}
