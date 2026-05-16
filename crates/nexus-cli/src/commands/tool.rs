//! `nexus tool list [--capability ID]…` — PRD-15 §4 (DG-32).
//!
//! Routes to `com.nexus.agent::list_tools`. The CLI never links
//! `nexus-agent`; the agent core plugin owns the registry.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::app::App;

const AGENT_PLUGIN: &str = "com.nexus.agent";
const IPC_TIMEOUT: Duration = Duration::from_secs(10);

/// `nexus tool list [--capability …]` — print the agent tool catalogue.
///
/// # Errors
/// Surfaces IPC dispatch errors and bad-args responses verbatim.
pub fn list(app: &mut App, capabilities: &[String]) -> Result<()> {
    let args = if capabilities.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        let arr: Vec<Value> = capabilities
            .iter()
            .map(|c| Value::String(c.clone()))
            .collect();
        let mut map = serde_json::Map::new();
        map.insert("capabilities".to_string(), Value::Array(arr));
        Value::Object(map)
    };

    let response = call(app, "list_tools", args)?;

    let tools = response
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("list_tools reply was not an array: {response}"))?;
    if tools.is_empty() {
        println!("(no tools)");
        return Ok(());
    }

    struct Row {
        name: String,
        description: String,
        approval: bool,
        duration_ms: u64,
        capabilities: Vec<String>,
    }
    let mut rows: Vec<Row> = Vec::with_capacity(tools.len());
    for entry in tools {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let approval = entry
            .get("requires_approval")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let duration_ms = entry
            .get("estimated_duration_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let capabilities: Vec<String> = entry
            .get("required_capabilities")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v: &Value| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        rows.push(Row {
            name,
            description,
            approval,
            duration_ms,
            capabilities,
        });
    }

    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .max("NAME".len());
    println!(
        "{:width$}  APPR  ~ms    CAPS                          DESCRIPTION",
        "NAME",
        width = name_w
    );
    for row in rows {
        let appr = if row.approval { "yes " } else { "no  " };
        let caps_join = if row.capabilities.is_empty() {
            "-".to_string()
        } else {
            row.capabilities.join(",")
        };
        println!(
            "{name:width$}  {appr}  {dur:<5}  {caps:<28}  {desc}",
            name = row.name,
            dur = row.duration_ms,
            caps = caps_join,
            desc = row.description,
            width = name_w
        );
    }
    Ok(())
}

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(AGENT_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("tool ipc call '{command}' failed"))
}
