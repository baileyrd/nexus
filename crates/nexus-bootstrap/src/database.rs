//! Typed IPC-client helpers for the `com.nexus.database` core plugin.
//!
//! CLI and TUI callers reach the pure-logic database helpers (CSV
//! import/export, formula evaluation) through these wrappers so they do not
//! need a direct `nexus-database` dependency (invariant #3). Each helper:
//!
//! 1. Serializes arguments to JSON.
//! 2. `await`s the async [`IpcInvoker::ipc_call`] on the provided invoker.
//! 3. Deserializes the response into a typed DTO.
//!
//! BL-147 — helpers take `&dyn IpcInvoker` rather than `&Runtime`, so the
//! same surface works against both local and remote (`ssh://`) forges. Each
//! helper is `async`; sync callers wrap with `rt.block_on(...)`.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::invoker::IpcInvoker;

/// Re-export of the CSV import response DTO defined alongside the plugin.
pub use nexus_database::core_plugin::{CsvExportResponse, CsvImportResponse, FormulaEvalResponse};

const DATABASE_PLUGIN: &str = "com.nexus.database";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

async fn call<T: serde::de::DeserializeOwned>(
    invoker: &(dyn IpcInvoker + Send + Sync),
    command: &str,
    args: serde_json::Value,
) -> Result<T> {
    let value = invoker
        .ipc_call(DATABASE_PLUGIN, command, args, IPC_TIMEOUT)
        .await
        .with_context(|| format!("database ipc call '{command}' failed"))?;
    serde_json::from_value(value)
        .with_context(|| format!("database ipc response '{command}' decode failed"))
}

/// Parse `csv_bytes` into `BaseRecord`s.
///
/// When `column_mapping` is `None`, the plugin derives one — matching header
/// names to `field_names` when `has_header = true`, or using positional
/// indices `0..N` when `has_header = false`.
pub async fn csv_import(
    invoker: &(dyn IpcInvoker + Send + Sync),
    csv_bytes: &[u8],
    field_names: &[String],
    has_header: bool,
    column_mapping: Option<&[(usize, String)]>,
) -> Result<CsvImportResponse> {
    call(
        invoker,
        "csv_import",
        serde_json::json!({
            "csv_bytes": csv_bytes,
            "field_names": field_names,
            "has_header": has_header,
            "column_mapping": column_mapping,
        }),
    )
    .await
}

/// Serialize `records` to CSV bytes using `field_names` for column ordering.
pub async fn csv_export(
    invoker: &(dyn IpcInvoker + Send + Sync),
    records: &[nexus_types::bases::BaseRecord],
    field_names: &[String],
) -> Result<CsvExportResponse> {
    call(
        invoker,
        "csv_export",
        serde_json::json!({
            "records": records,
            "field_names": field_names,
        }),
    )
    .await
}

/// Evaluate a formula `expression` against a record's `fields`.
pub async fn formula_eval(
    invoker: &(dyn IpcInvoker + Send + Sync),
    expression: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<FormulaEvalResponse> {
    call(
        invoker,
        "formula_eval",
        serde_json::json!({
            "expression": expression,
            "fields": fields,
        }),
    )
    .await
}
