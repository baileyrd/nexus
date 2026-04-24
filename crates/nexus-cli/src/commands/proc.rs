//! Process manager commands — `nexus proc list|show|add|delete|reorder`.
//!
//! Dispatches through `com.nexus.terminal` (handlers 11–15) via
//! `ipc_call`; no direct `nexus-terminal` linkage.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const TERMINAL_PLUGIN: &str = "com.nexus.terminal";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// `nexus proc list` — summary of every saved command.
pub fn list(app: &mut App) -> Result<()> {
    let response = call(app, "saved_list", serde_json::json!({}))?;
    print_list(&response);
    Ok(())
}

/// `nexus proc show <slug>` — full record for one saved command.
///
/// No `get` handler exists on the plugin yet, so we fetch the list
/// and filter client-side. Small N (the UI already assumes the list
/// fits in memory) so the cost is negligible.
pub fn show(app: &mut App, slug: &str) -> Result<()> {
    let response = call(app, "saved_list", serde_json::json!({}))?;
    let arr = response.as_array().cloned().unwrap_or_default();
    match arr.iter().find(|v| v.get("slug").and_then(Value::as_str) == Some(slug)) {
        Some(record) => print_full(record),
        None => {
            eprintln!("no saved command with slug '{slug}'");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `nexus proc add <name> <command>` — create a new saved command
/// with defaults for every other field. Uses a slugified name as the
/// primary key.
pub fn add(
    app: &mut App,
    name: &str,
    command_line: &str,
    shell: Option<&str>,
    working_dir: Option<&str>,
) -> Result<()> {
    let slug = slugify(name);
    let now = chrono::Utc::now().timestamp();
    let record = serde_json::json!({
        "slug": slug,
        "name": name,
        "shell": shell.unwrap_or("/bin/sh"),
        "shell_cmd": command_line,
        "working_dir": working_dir,
        "env_vars": {},
        "env_file": null,
        "icon": "terminal",
        "auto_restart": false,
        "auto_restart_delay_ms": 2000,
        "memory_limit_mb": null,
        "sidebar_order": null,
        "pre_commands": [],
        "created_at": now,
        "updated_at": now,
    });
    call(app, "saved_create", record.clone())?;
    println!("Created saved command '{slug}'.");
    Ok(())
}

/// `nexus proc delete <slug>` — remove a saved command.
pub fn delete(app: &mut App, slug: &str) -> Result<()> {
    call(app, "saved_delete", serde_json::json!({ "slug": slug }))?;
    println!("Deleted saved command '{slug}'.");
    Ok(())
}

/// `nexus proc reorder <slug> [--order N]` — set the sidebar_order
/// column. Pass no order to clear the override (nulls sort last).
pub fn reorder(app: &mut App, slug: &str, order: Option<i32>) -> Result<()> {
    call(
        app,
        "saved_reorder",
        serde_json::json!({ "slug": slug, "sidebar_order": order }),
    )?;
    match order {
        Some(n) => println!("Reordered '{slug}' → {n}."),
        None => println!("Cleared sidebar_order for '{slug}'."),
    }
    Ok(())
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_list(response: &Value) {
    let arr = match response.as_array() {
        Some(a) => a,
        None => {
            eprintln!("unexpected response shape: {response}");
            return;
        }
    };
    if arr.is_empty() {
        println!("(no saved commands)");
        return;
    }
    let slug_w = arr
        .iter()
        .filter_map(|v| v.get("slug").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    let name_w = arr
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<slug_w$}  {:<name_w$}  COMMAND",
        "SLUG",
        "NAME",
        slug_w = slug_w,
        name_w = name_w,
    );
    for v in arr {
        let slug = v.get("slug").and_then(Value::as_str).unwrap_or("?");
        let name = v.get("name").and_then(Value::as_str).unwrap_or("?");
        let cmd = v
            .get("shell_cmd")
            .and_then(Value::as_str)
            .unwrap_or("");
        println!(
            "{:<slug_w$}  {:<name_w$}  {}",
            slug,
            name,
            cmd,
            slug_w = slug_w,
            name_w = name_w,
        );
    }
}

fn print_full(record: &Value) {
    let slug = record.get("slug").and_then(Value::as_str).unwrap_or("?");
    let name = record.get("name").and_then(Value::as_str).unwrap_or("?");
    let shell = record.get("shell").and_then(Value::as_str).unwrap_or("?");
    let cmd = record
        .get("shell_cmd")
        .and_then(Value::as_str)
        .unwrap_or("");
    println!("{name} ({slug})");
    println!("Shell : {shell}");
    println!("Cmd   : {cmd}");
    if let Some(wd) = record.get("working_dir").and_then(Value::as_str) {
        println!("Cwd   : {wd}");
    }
    if let Some(order) = record.get("sidebar_order").and_then(Value::as_i64) {
        println!("Order : {order}");
    }
    if let Some(pre) = record.get("pre_commands").and_then(Value::as_array) {
        if !pre.is_empty() {
            let joined = pre
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" && ");
            println!("Pre   : {joined}");
        }
    }
}

fn slugify(name: &str) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let collapsed: String = base
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "cmd".into()
    } else {
        collapsed
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(
        runtime
            .context
            .ipc_call(TERMINAL_PLUGIN, command, args, IPC_TIMEOUT),
    )
    .with_context(|| format!("terminal ipc call '{command}' failed"))
}
