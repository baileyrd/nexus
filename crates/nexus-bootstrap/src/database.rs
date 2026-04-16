//! Typed IPC-client helpers for the `com.nexus.database` core plugin.
//!
//! CLI and TUI callers reach the pure-logic database helpers (CSV
//! import/export, formula evaluation) through these wrappers so they do not
//! need a direct `nexus-database` dependency (invariant #3). Each helper:
//!
//! 1. Serializes arguments to JSON.
//! 2. `block_on`s the async `ipc_call` on the provided Tokio runtime.
//! 3. Deserializes the response into a typed DTO.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use tokio::runtime::Runtime as TokioRuntime;

use crate::Runtime;

/// Re-export of the CSV import response DTO defined alongside the plugin.
pub use nexus_database::core_plugin::{CsvExportResponse, CsvImportResponse, FormulaEvalResponse};

const DATABASE_PLUGIN: &str = "com.nexus.database";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

fn call<T: serde::de::DeserializeOwned>(
    runtime: &Runtime,
    rt: &TokioRuntime,
    command: &str,
    args: serde_json::Value,
) -> Result<T> {
    let value = rt
        .block_on(
            runtime
                .context
                .ipc_call(DATABASE_PLUGIN, command, args, IPC_TIMEOUT),
        )
        .with_context(|| format!("database ipc call '{command}' failed"))?;
    serde_json::from_value(value)
        .with_context(|| format!("database ipc response '{command}' decode failed"))
}

/// Parse `csv_bytes` into `BaseRecord`s.
///
/// When `column_mapping` is `None`, the plugin derives one — matching header
/// names to `field_names` when `has_header = true`, or using positional
/// indices `0..N` when `has_header = false`.
pub fn csv_import(
    runtime: &Runtime,
    rt: &TokioRuntime,
    csv_bytes: &[u8],
    field_names: &[String],
    has_header: bool,
    column_mapping: Option<&[(usize, String)]>,
) -> Result<CsvImportResponse> {
    call(
        runtime,
        rt,
        "csv_import",
        serde_json::json!({
            "csv_bytes": csv_bytes,
            "field_names": field_names,
            "has_header": has_header,
            "column_mapping": column_mapping,
        }),
    )
}

/// Serialize `records` to CSV bytes using `field_names` for column ordering.
pub fn csv_export(
    runtime: &Runtime,
    rt: &TokioRuntime,
    records: &[nexus_types::bases::BaseRecord],
    field_names: &[String],
) -> Result<CsvExportResponse> {
    call(
        runtime,
        rt,
        "csv_export",
        serde_json::json!({
            "records": records,
            "field_names": field_names,
        }),
    )
}

/// Evaluate a formula `expression` against a record's `fields`.
pub fn formula_eval(
    runtime: &Runtime,
    rt: &TokioRuntime,
    expression: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<FormulaEvalResponse> {
    call(
        runtime,
        rt,
        "formula_eval",
        serde_json::json!({
            "expression": expression,
            "fields": fields,
        }),
    )
}
