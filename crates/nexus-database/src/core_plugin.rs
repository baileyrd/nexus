//! Core plugin: exposes `nexus-database`'s pure-logic helpers as IPC.
//!
//! Registers as `com.nexus.database`. The SQL-backed query engine for bases
//! lives in `nexus-storage` (the sole `rusqlite` owner); this plugin exposes
//! only the pure in-memory operations that don't touch `SQLite`:
//!
//! | Command        | Handler id | Description                                   |
//! |----------------|------------|-----------------------------------------------|
//! | `csv_import`   | 1          | Parse CSV bytes into `BaseRecord`s            |
//! | `csv_export`   | 2          | Serialize `BaseRecord`s to CSV bytes          |
//! | `formula_eval` | 3          | Evaluate a formula against a record's fields  |
//!
//! Invokers (CLI / TUI) reach these via
//! `ipc_call("com.nexus.database", …)` instead of linking `nexus-database`
//! directly (invariant #3 in `docs/ARCHITECTURE.md` §7).

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

use crate::formula;
use crate::import_export::{export_csv, import_csv, ColumnMapping};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.database";

// ── IPC handler ids ──────────────────────────────────────────────────────────
//
// Stable within the plugin. Append only — never reuse a retired id.

/// Handler id for `csv_import`. See [`CsvImportArgs`] / [`CsvImportResponse`].
pub const HANDLER_CSV_IMPORT: u32 = 1;
/// Handler id for `csv_export`. See [`CsvExportArgs`] / [`CsvExportResponse`].
pub const HANDLER_CSV_EXPORT: u32 = 2;
/// Handler id for `formula_eval`. See [`FormulaEvalArgs`] / [`FormulaEvalResponse`].
pub const HANDLER_FORMULA_EVAL: u32 = 3;
/// Handler id for `apply_view`. See [`ApplyViewArgs`] and
/// [`crate::views::AppliedView`] for the response shape.
pub const HANDLER_APPLY_VIEW: u32 = 4;

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Arguments for `csv_import`.
#[derive(Debug, Clone, Deserialize)]
pub struct CsvImportArgs {
    /// Raw CSV file bytes.
    pub csv_bytes: Vec<u8>,
    /// Ordered list of field names as they appear in the schema. Used when
    /// building a default column mapping (header match, or positional).
    pub field_names: Vec<String>,
    /// Whether the first row is a header. When `true` and `column_mapping`
    /// is `None`, the handler matches header names against `field_names`.
    pub has_header: bool,
    /// Explicit `(csv_column_index, field_name)` mapping. When `None`, the
    /// handler derives one from the headers (or positional 0..N when
    /// `has_header = false`).
    #[serde(default)]
    pub column_mapping: Option<Vec<(usize, String)>>,
}

/// Response from `csv_import`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvImportResponse {
    /// Records parsed out of the CSV.
    pub records: Vec<nexus_types::bases::BaseRecord>,
    /// Number of records successfully imported.
    pub imported: usize,
    /// Number of records skipped due to parse errors.
    pub skipped: usize,
    /// Per-row errors: `(row_number, message)`.
    pub errors: Vec<(usize, String)>,
}

/// Arguments for `csv_export`.
#[derive(Debug, Clone, Deserialize)]
pub struct CsvExportArgs {
    /// Records to serialize.
    pub records: Vec<nexus_types::bases::BaseRecord>,
    /// Column order for the CSV header and each row.
    pub field_names: Vec<String>,
}

/// Response from `csv_export`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvExportResponse {
    /// Serialized CSV bytes (header row + one row per record).
    pub csv_bytes: Vec<u8>,
    /// Number of records written (excludes header).
    pub count: usize,
}

/// Arguments for `formula_eval`.
#[derive(Debug, Clone, Deserialize)]
pub struct FormulaEvalArgs {
    /// Formula expression (Notion-compatible syntax).
    pub expression: String,
    /// The record's fields, keyed by schema field name.
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Response from `formula_eval`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormulaEvalResponse {
    /// Display-formatted result string (same as [`formula::FormulaValue::to_display_string`]).
    pub display: String,
}

/// Arguments for `apply_view` — runs the full filter / sort / group
/// pipeline from [`crate::views::apply_view`] and returns an
/// [`crate::views::AppliedView`].
#[derive(Debug, Clone, Deserialize)]
pub struct ApplyViewArgs {
    /// Records to apply the view against.
    pub records: Vec<nexus_types::bases::BaseRecord>,
    /// Schema describing the fields. Currently unused by the view
    /// engine but accepted on the wire so future type-aware filters
    /// don't break the handler shape.
    pub schema: nexus_types::bases::BaseSchema,
    /// View definition — name, type, fields, sort, filter, grouping.
    pub view: nexus_types::bases::BaseView,
}

/// Core plugin exposing pure-logic database helpers over IPC.
///
/// Holds no state — every handler is a pure function over its args.
#[derive(Debug, Default)]
pub struct DatabaseCorePlugin;

impl DatabaseCorePlugin {
    /// Construct a new plugin instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CorePlugin for DatabaseCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_CSV_IMPORT => dispatch_csv_import(args),
            HANDLER_CSV_EXPORT => dispatch_csv_export(args),
            HANDLER_FORMULA_EVAL => dispatch_formula_eval(args),
            HANDLER_APPLY_VIEW => dispatch_apply_view(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── Dispatch helpers ─────────────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn parse_args<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(value.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize failed: {e}")))
}

/// Maximum byte size accepted by the `csv_import` IPC handler. The
/// reader is fully in-memory; an unbounded `csv_bytes` reaching this
/// handler from IPC was the documented `DoS` shape from issue #78.
/// `10 MiB` is generous for any realistic CSV (Excel's row cap times
/// a hundred columns of small values still lands under it).
const MAX_CSV_IMPORT_BYTES: usize = 10 * 1024 * 1024;

fn dispatch_csv_import(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: CsvImportArgs = parse_args(args, "csv_import")?;

    if a.csv_bytes.len() > MAX_CSV_IMPORT_BYTES {
        return Err(exec_err(format!(
            "csv_import: input is {} bytes; max is {MAX_CSV_IMPORT_BYTES} bytes",
            a.csv_bytes.len(),
        )));
    }

    let mapping = if let Some(pairs) = a.column_mapping {
        ColumnMapping { mappings: pairs }
    } else if a.has_header {
        let mut peek = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(a.csv_bytes.as_slice());
        let headers = peek
            .headers()
            .map_err(|e| exec_err(format!("csv_import: header read: {e}")))?
            .clone();
        ColumnMapping::from_headers(&headers, &a.field_names)
    } else {
        ColumnMapping {
            mappings: a
                .field_names
                .iter()
                .enumerate()
                .map(|(i, n)| (i, n.clone()))
                .collect(),
        }
    };

    let (records, result) = import_csv(a.csv_bytes.as_slice(), &mapping, a.has_header)
        .map_err(|e| exec_err(format!("csv_import: {e}")))?;

    to_value(
        &CsvImportResponse {
            records,
            imported: result.imported,
            skipped: result.skipped,
            errors: result.errors,
        },
        "csv_import",
    )
}

fn dispatch_csv_export(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: CsvExportArgs = parse_args(args, "csv_export")?;
    let mut buf: Vec<u8> = Vec::new();
    let count = export_csv(&mut buf, &a.records, &a.field_names)
        .map_err(|e| exec_err(format!("csv_export: {e}")))?;
    to_value(
        &CsvExportResponse {
            csv_bytes: buf,
            count,
        },
        "csv_export",
    )
}

fn dispatch_formula_eval(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: FormulaEvalArgs = parse_args(args, "formula_eval")?;
    let value = formula::evaluate(&a.expression, &a.fields)
        .map_err(|e| exec_err(format!("formula_eval: {e}")))?;
    to_value(
        &FormulaEvalResponse {
            display: value.to_display_string(),
        },
        "formula_eval",
    )
}

fn dispatch_apply_view(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    let a: ApplyViewArgs = parse_args(args, "apply_view")?;
    let applied = crate::views::apply_view(&a.records, &a.schema, &a.view);
    to_value(&applied, "apply_view")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_import_roundtrips_through_dispatch() {
        let mut plugin = DatabaseCorePlugin::new();
        let csv = b"name,score\nAlice,95\nBob,87\n";
        let args = serde_json::json!({
            "csv_bytes": csv,
            "field_names": ["name", "score"],
            "has_header": true,
        });
        let value = plugin.dispatch(HANDLER_CSV_IMPORT, &args).unwrap();
        let resp: CsvImportResponse = serde_json::from_value(value).unwrap();
        assert_eq!(resp.imported, 2);
        assert_eq!(resp.skipped, 0);
        assert_eq!(resp.records.len(), 2);
        assert_eq!(resp.records[0].fields.get("name").unwrap(), "Alice");
    }

    #[test]
    fn csv_export_roundtrips_through_dispatch() {
        let mut plugin = DatabaseCorePlugin::new();
        let records = vec![nexus_types::bases::BaseRecord {
            id: "r1".to_string(),
            deleted_at: None,
            fields: {
                let mut m = serde_json::Map::new();
                m.insert("name".to_string(), serde_json::json!("Alice"));
                m.insert("score".to_string(), serde_json::json!(95));
                m
            },
        }];
        let args = serde_json::json!({
            "records": records,
            "field_names": ["name", "score"],
        });
        let value = plugin.dispatch(HANDLER_CSV_EXPORT, &args).unwrap();
        let resp: CsvExportResponse = serde_json::from_value(value).unwrap();
        assert_eq!(resp.count, 1);
        let text = String::from_utf8(resp.csv_bytes).unwrap();
        assert!(text.contains("name,score"));
        assert!(text.contains("Alice,95"));
    }

    #[test]
    fn formula_eval_dispatches_cleanly() {
        let mut plugin = DatabaseCorePlugin::new();
        let args = serde_json::json!({
            "expression": "prop(\"score\") + 5",
            "fields": { "score": 10 },
        });
        let value = plugin.dispatch(HANDLER_FORMULA_EVAL, &args).unwrap();
        let resp: FormulaEvalResponse = serde_json::from_value(value).unwrap();
        assert_eq!(resp.display, "15");
    }

    #[test]
    fn apply_view_dispatches_kanban_grouped_response() {
        let mut plugin = DatabaseCorePlugin::new();
        let args = serde_json::json!({
            "records": [
                { "id": "a", "status": "todo", "priority": 1 },
                { "id": "b", "status": "done", "priority": 2 },
                { "id": "c", "status": "todo", "priority": 3 },
            ],
            "schema": { "version": "1.0", "fields": {} },
            "view": {
                "name": "Board",
                "type": "kanban",
                "fields": ["title"],
                "sort": [{ "field": "priority", "direction": "asc" }],
                "filter": [],
                "groupField": "status",
            },
        });
        let value = plugin.dispatch(HANDLER_APPLY_VIEW, &args).unwrap();
        // Response shape is a full AppliedView with a Grouped layout.
        let layout = value
            .get("layout")
            .expect("layout")
            .clone();
        assert_eq!(layout["kind"], "grouped");
        let groups = layout["groups"].as_array().expect("groups");
        assert_eq!(groups.len(), 2);
        // BTreeMap ordering: done < todo.
        assert_eq!(groups[0]["key"], "done");
        assert_eq!(groups[1]["key"], "todo");
    }

    #[test]
    fn unknown_handler_id_returns_error() {
        let mut plugin = DatabaseCorePlugin::new();
        let err = plugin
            .dispatch(9999, &serde_json::json!({}))
            .expect_err("unknown handler should error");
        match err {
            PluginError::ExecutionFailed { plugin_id, .. } => {
                assert_eq!(plugin_id, PLUGIN_ID);
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }
}
