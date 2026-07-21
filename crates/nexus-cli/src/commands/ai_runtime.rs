//! `nexus ai runtime list|get|cancel|pool-stats|triggers` — observe/control
//! `com.nexus.ai.runtime` background tasks (C79 #432).
//!
//! ADR 0028 designed this surface but it never shipped to any frontend;
//! the observe/control IPC handlers already exist (agent delegate
//! fan-out and async workflow steps create the tasks this observes), so
//! this is pure frontend wiring — every call routes through
//! `com.nexus.ai.runtime` via `ipc_call`.

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_LONG as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;
use crate::output::{print_list, print_success, print_value, OutputFormat};

const AI_RUNTIME_PLUGIN: &str = plugin_ids::AI_RUNTIME;

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(AI_RUNTIME_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("ai-runtime ipc call '{command}' failed"))
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string()
}

/// List background tasks, optionally filtered by status / capped by limit.
pub fn list(app: &mut App, status: Option<&str>, limit: Option<u32>) -> Result<()> {
    let mut args = serde_json::json!({});
    if let Some(status) = status {
        args["status"] = Value::String(status.to_string());
    }
    if let Some(limit) = limit {
        args["limit"] = Value::from(limit);
    }

    let response = call(app, "list", args)?;
    let format = app.format();
    let runs = response
        .get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if format == OutputFormat::Text && runs.is_empty() {
        println!("No background tasks.");
        return Ok(());
    }

    let headers = &["Task ID", "Kind", "Priority", "Status", "Caller", "Submitted"];
    let rows: Vec<Vec<String>> = runs
        .iter()
        .map(|r| {
            vec![
                str_field(r, "task_id"),
                str_field(r, "kind"),
                str_field(r, "priority"),
                str_field(r, "status"),
                str_field(r, "caller_plugin_id"),
                str_field(r, "submitted_at"),
            ]
        })
        .collect();
    print_list(format, headers, &rows);
    Ok(())
}

/// Show full detail for one task (events, timestamps, status).
pub fn get(app: &mut App, task_id: &str) -> Result<()> {
    let response = call(app, "get", serde_json::json!({ "task_id": task_id }))?;
    print_value(app.format(), &response);
    Ok(())
}

/// Request cancellation of a queued or running task.
pub fn cancel(app: &mut App, task_id: &str, reason: Option<&str>) -> Result<()> {
    let mut args = serde_json::json!({ "task_id": task_id });
    if let Some(reason) = reason {
        args["reason"] = Value::String(reason.to_string());
    }
    call(app, "cancel", args)?;
    let format = app.format();
    print_success(
        format,
        &format!("Cancellation requested for task '{task_id}'."),
        &serde_json::json!({ "task_id": task_id, "cancelled": true }),
    );
    Ok(())
}

/// Show current worker-pool utilization.
pub fn pool_stats(app: &mut App) -> Result<()> {
    let response = call(app, "pool_stats", Value::Null)?;
    print_value(app.format(), &response);
    Ok(())
}

/// List registered ambient triggers.
pub fn triggers(app: &mut App) -> Result<()> {
    let response = call(app, "list_triggers", Value::Null)?;
    let format = app.format();
    let triggers = response
        .get("triggers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if format == OutputFormat::Text && triggers.is_empty() {
        println!("No registered triggers.");
        return Ok(());
    }

    let headers = &["ID", "Name", "Enabled", "Goal Template"];
    let rows: Vec<Vec<String>> = triggers
        .iter()
        .map(|t| {
            let enabled = t
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            vec![
                str_field(t, "id"),
                str_field(t, "name"),
                enabled.to_string(),
                str_field(t, "goal_template"),
            ]
        })
        .collect();
    print_list(format, headers, &rows);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_field_falls_back_to_placeholder_for_missing_key() {
        let v = serde_json::json!({ "present": "value" });
        assert_eq!(str_field(&v, "present"), "value");
        assert_eq!(str_field(&v, "missing"), "?");
    }

    #[test]
    fn str_field_falls_back_to_placeholder_for_non_string_value() {
        let v = serde_json::json!({ "count": 5 });
        assert_eq!(str_field(&v, "count"), "?");
    }
}
