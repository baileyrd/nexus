//! Skill command handlers — `nexus skill list|show|context|triggered|reload`.
//!
//! All five dispatch through `com.nexus.skills` via `ipc_call`; no
//! direct `nexus-skills` linkage.

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_SHORT as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;

const SKILLS_PLUGIN: &str = plugin_ids::SKILLS;

/// `nexus skill list` — every loaded skill.
pub fn list(app: &mut App) -> Result<()> {
    let response = call(app, "list", serde_json::json!({}))?;
    print_summary_table(&response);
    Ok(())
}

/// `nexus skill show <id>` — full frontmatter + body for one skill.
pub fn show(app: &mut App, id: &str) -> Result<()> {
    let response = call(app, "get", serde_json::json!({ "id": id }))?;
    print_full(&response);
    Ok(())
}

/// `nexus skill context <ctx>` — skills auto-activating in `ctx`.
pub fn context(app: &mut App, ctx: &str) -> Result<()> {
    let response = call(app, "list_by_context", serde_json::json!({ "context": ctx }))?;
    print_summary_table(&response);
    Ok(())
}

/// `nexus skill triggered <text>` — skills whose triggers match.
pub fn triggered(app: &mut App, text: &str) -> Result<()> {
    let response = call(app, "triggered_by", serde_json::json!({ "text": text }))?;
    print_summary_table(&response);
    Ok(())
}

/// `nexus skill render <id> [--param k=v ...]` — render the skill
/// body with parameter substitution applied.
pub fn render(app: &mut App, id: &str, params: &[String]) -> Result<()> {
    let mut values = serde_json::Map::new();
    for raw in params {
        let (k, v) = raw
            .split_once('=')
            .with_context(|| format!("invalid --param '{raw}': expected KEY=VALUE"))?;
        values.insert(k.trim().to_string(), Value::String(v.to_string()));
    }
    let response = call(
        app,
        "render",
        serde_json::json!({ "id": id, "values": values }),
    )?;
    if let Some(body) = response.get("body").and_then(Value::as_str) {
        print!("{body}");
    } else {
        eprintln!("unexpected response shape: {response}");
    }
    Ok(())
}

/// `nexus skill reload` — re-scan the skills directory.
pub fn reload(app: &mut App) -> Result<()> {
    let response = call(app, "reload", serde_json::json!({}))?;
    let n = response
        .get("loaded")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    println!("Reloaded {n} skill(s).");
    Ok(())
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_summary_table(response: &Value) {
    let skills = match response.as_array() {
        Some(arr) => arr,
        None => {
            eprintln!("unexpected response shape: {response}");
            return;
        }
    };
    if skills.is_empty() {
        println!("(no skills)");
        return;
    }
    let id_w = skills
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(2)
        .max(2);
    let name_w = skills
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<id_w$}  {:<name_w$}  TAGS",
        "ID",
        "NAME",
        id_w = id_w,
        name_w = name_w,
    );
    for s in skills {
        let id = s.get("id").and_then(Value::as_str).unwrap_or("?");
        let name = s.get("name").and_then(Value::as_str).unwrap_or("?");
        let tags = s
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        println!(
            "{:<id_w$}  {:<name_w$}  {}",
            id,
            name,
            tags,
            id_w = id_w,
            name_w = name_w,
        );
    }
}

fn print_full(skill: &Value) {
    let id = skill.get("id").and_then(Value::as_str).unwrap_or("?");
    let name = skill.get("name").and_then(Value::as_str).unwrap_or("?");
    let version = skill
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let author = skill.get("author").and_then(Value::as_str).unwrap_or("?");
    let description = skill
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    println!("{name} ({id}) v{version}");
    println!("by {author}");
    if !description.is_empty() {
        println!();
        println!("{description}");
    }
    if let Some(tags) = skill.get("tags").and_then(Value::as_array) {
        let joined = tags
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !joined.is_empty() {
            println!();
            println!("Tags : {joined}");
        }
    }
    if let Some(contexts) = skill
        .get("applicable_contexts")
        .and_then(Value::as_array)
    {
        let joined = contexts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !joined.is_empty() {
            println!("Contexts : {joined}");
        }
    }
    if let Some(body) = skill.get("body").and_then(Value::as_str) {
        if !body.is_empty() {
            println!();
            println!("--- Body ---");
            println!("{body}");
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(SKILLS_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("skills ipc call '{command}' failed"))
}
