//! `com.nexus.workflow::next_fire` handler — compute the next
//! scheduled fire time for cron-triggered workflows. Lifted out of
//! `core_plugin.rs` by the BL-137 oversized-file decomposition.
//!
//! Walks the registry, picks `trigger_type == "cron"` (optionally
//! filtering by `name`), parses the `schedule` expression, and asks
//! `next_after(Utc::now())`. Workflows whose expression fails to parse
//! contribute a row with `next_fire_at: null` so the UI can render
//! them with a "schedule unparseable" hint rather than disappearing
//! them.

use std::sync::Mutex;

use nexus_plugins::PluginError;

use crate::core_plugin::NextFireArgs;
use crate::WorkflowRegistry;

use super::shared::{parse_args, poisoned};

pub(crate) fn handle(
    registry: &Mutex<WorkflowRegistry>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: NextFireArgs = parse_args(args, "next_fire")?;
    let now = chrono::Utc::now();
    let reg = registry.lock().map_err(poisoned)?;
    let rows: Vec<serde_json::Value> = reg
        .iter()
        .filter(|(_, wf)| wf.trigger.trigger_type == "cron")
        .filter(|(name, _)| a.name.as_deref().is_none_or(|n| n == *name))
        .map(|(name, wf)| {
            let expr = wf
                .trigger
                .extra
                .get("schedule")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let next = if expr.is_empty() {
                None
            } else {
                match crate::cron::CronSchedule::parse(expr) {
                    Ok(s) => s.next_after(now),
                    Err(_) => None,
                }
            };
            serde_json::json!({
                "name": name,
                "expression": expr,
                "next_fire_at": next.map(|t| t.to_rfc3339()),
            })
        })
        .collect();
    Ok(serde_json::Value::Array(rows))
}
