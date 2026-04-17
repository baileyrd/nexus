//! Tauri command bridge into the database core plugin.
//!
//! Thin adapters over `ctx.ipc_call("com.nexus.database", …)`, mirroring
//! the pattern in [`crate::editor`] / [`crate::terminal`]. Keeps
//! `nexus-database` linkage off `nexus-app`'s public surface — plugin
//! id is the stable contract.

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

use crate::editor::KernelRuntime;

const DATABASE_PLUGIN_ID: &str = "com.nexus.database";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

async fn call_database(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(DATABASE_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Apply a [`nexus_types::bases::BaseView`] to a record set, returning
/// an `AppliedView` with the records filtered, sorted, and — for
/// kanban/calendar views — grouped.
#[tauri::command]
pub async fn db_apply_view(
    records: serde_json::Value,
    schema: serde_json::Value,
    view: serde_json::Value,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_database(
        runtime,
        "apply_view",
        serde_json::json!({
            "records": records,
            "schema": schema,
            "view": view,
        }),
    )
    .await
}
