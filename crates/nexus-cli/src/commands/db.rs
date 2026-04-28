//! Database command handlers — `nexus db import-csv|export-csv|eval-formula`.
//!
//! All three dispatch through `com.nexus.database` via `ipc_call`; no direct
//! `nexus-database` linkage. This is the lower-level IPC-only twin of
//! `nexus bases`, which works at the filesystem layer (`.bases` directories).
//!
//! `apply_view` (handler id 4) is intentionally not surfaced — it requires a
//! base file context and is exercised by the shell renderer rather than
//! being a useful CLI today. `list` / `show <base>` would require a new
//! `list_bases` handler that does not yet exist; both are deferred.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const DATABASE_PLUGIN: &str = "com.nexus.database";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// `nexus db <subcommand>` — wraps `com.nexus.database` IPC handlers.
#[derive(Subcommand, Debug)]
pub enum DbCommand {
    /// Import records from a CSV file (`com.nexus.database::csv_import`).
    ImportCsv {
        /// Path to the CSV file to import.
        file: PathBuf,
        /// Ordered list of field names matching the schema, comma-separated.
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
        /// Treat the first row as a header. When set, header names are
        /// matched against `--fields`; otherwise positional 0..N mapping.
        #[arg(long)]
        has_header: bool,
        /// Explicit column mapping as JSON: `[[col_index, "field"], ...]`.
        /// Inline JSON or `@<path>` to read from a file.
        #[arg(long)]
        column_mapping: Option<String>,
    },
    /// Export records to CSV bytes (`com.nexus.database::csv_export`).
    ExportCsv {
        /// JSON array of `BaseRecord`s, or `@<path>` to read from a file.
        /// Defaults to `[]` if omitted (header-only export).
        #[arg(long)]
        records: Option<String>,
        /// Column order for the CSV header and each row, comma-separated.
        #[arg(long, value_delimiter = ',')]
        fields: Option<Vec<String>>,
        /// Output file path. Writes CSV bytes to stdout if omitted.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Evaluate a formula against a record's fields
    /// (`com.nexus.database::formula_eval`).
    EvalFormula {
        /// Formula expression (Notion-compatible syntax, e.g.
        /// `prop("score") + 5`).
        expr: String,
        /// JSON object mapping field name → value, or `@<path>` to read
        /// from a file.
        #[arg(long)]
        fields: String,
    },
}

/// Dispatch entry point used by `main.rs`.
pub fn run(app: &mut App, cmd: DbCommand) -> Result<()> {
    match cmd {
        DbCommand::ImportCsv {
            file,
            fields,
            has_header,
            column_mapping,
        } => import_csv(app, &file, &fields, has_header, column_mapping.as_deref()),
        DbCommand::ExportCsv {
            records,
            fields,
            output,
        } => export_csv(
            app,
            records.as_deref(),
            fields.as_deref(),
            output.as_deref(),
        ),
        DbCommand::EvalFormula { expr, fields } => eval_formula(app, &expr, &fields),
    }
}

// ── Subcommand handlers ─────────────────────────────────────────────────────

fn import_csv(
    app: &mut App,
    file: &std::path::Path,
    fields: &[String],
    has_header: bool,
    column_mapping: Option<&str>,
) -> Result<()> {
    let csv_bytes = std::fs::read(file)
        .with_context(|| format!("failed to read CSV file '{}'", file.display()))?;
    let mapping_text = match column_mapping {
        Some(s) => Some(
            resolve_inline_or_file(s)
                .with_context(|| format!("failed to read --column-mapping '{s}'"))?,
        ),
        None => None,
    };
    let args = build_csv_import_args(&csv_bytes, fields, has_header, mapping_text.as_deref())
        .context("failed to build csv_import args")?;
    let response = call(app, "csv_import", args)?;
    let imported = response
        .get("imported")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    println!("Imported {imported} record(s).");
    if let Some(skipped) = response.get("skipped").and_then(Value::as_u64) {
        if skipped > 0 {
            println!("Skipped {skipped} record(s) due to parse errors.");
        }
    }
    Ok(())
}

fn export_csv(
    app: &mut App,
    records: Option<&str>,
    fields: Option<&[String]>,
    output: Option<&std::path::Path>,
) -> Result<()> {
    let records_text = match records {
        Some(s) => Some(
            resolve_inline_or_file(s).with_context(|| format!("failed to read --records '{s}'"))?,
        ),
        None => None,
    };
    let args = build_csv_export_args(records_text.as_deref(), fields)
        .context("failed to build csv_export args")?;
    let response = call(app, "csv_export", args)?;
    let csv_bytes =
        extract_csv_bytes(&response).context("csv_export response missing csv_bytes")?;
    if let Some(path) = output {
        std::fs::write(path, &csv_bytes)
            .with_context(|| format!("failed to write CSV to '{}'", path.display()))?;
    } else {
        use std::io::Write;
        let mut stdout = std::io::stdout();
        stdout
            .write_all(&csv_bytes)
            .context("failed to write CSV bytes to stdout")?;
    }
    Ok(())
}

fn eval_formula(app: &mut App, expr: &str, fields: &str) -> Result<()> {
    let fields_text = resolve_inline_or_file(fields)
        .with_context(|| format!("failed to read --fields '{fields}'"))?;
    let args =
        build_eval_formula_args(expr, &fields_text).context("failed to build formula_eval args")?;
    let response = call(app, "formula_eval", args)?;
    let pretty = serde_json::to_string_pretty(&response)
        .context("failed to format formula_eval response")?;
    println!("{pretty}");
    Ok(())
}

// ── Pure-function arg builders ──────────────────────────────────────────────

/// Build `csv_import` IPC args. Pure: no IPC, no file IO.
///
/// `column_mapping`, when supplied, must be a JSON array of
/// `[col_index, "field_name"]` pairs.
fn build_csv_import_args(
    csv_bytes: &[u8],
    fields: &[String],
    has_header: bool,
    column_mapping: Option<&str>,
) -> Result<Value, serde_json::Error> {
    let mapping: Option<Value> = match column_mapping {
        Some(s) => Some(serde_json::from_str(s)?),
        None => None,
    };
    Ok(serde_json::json!({
        "csv_bytes": csv_bytes,
        "field_names": fields,
        "has_header": has_header,
        "column_mapping": mapping,
    }))
}

/// Build `csv_export` IPC args. Pure: no IPC, no file IO.
///
/// `records_json`, when supplied, must be a JSON array of `BaseRecord`s.
/// Defaults to `[]` when `None`. `fields` defaults to `[]`.
fn build_csv_export_args(
    records_json: Option<&str>,
    fields: Option<&[String]>,
) -> Result<Value, serde_json::Error> {
    let records: Value = match records_json {
        Some(s) => serde_json::from_str(s)?,
        None => Value::Array(Vec::new()),
    };
    let field_names: Vec<&str> = fields
        .map(|f| f.iter().map(String::as_str).collect())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "records": records,
        "field_names": field_names,
    }))
}

/// Build `formula_eval` IPC args. Pure: no IPC, no file IO.
///
/// `fields_json` must be a JSON object mapping field name → value.
fn build_eval_formula_args(expr: &str, fields_json: &str) -> Result<Value, serde_json::Error> {
    let fields: Value = serde_json::from_str(fields_json)?;
    Ok(serde_json::json!({
        "expression": expr,
        "fields": fields,
    }))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Resolve a CLI argument that can be either an inline literal or
/// `@<path>` for a file. With no `@` prefix, returns `s` verbatim.
fn resolve_inline_or_file(s: &str) -> std::io::Result<String> {
    if let Some(path) = s.strip_prefix('@') {
        std::fs::read_to_string(path)
    } else {
        Ok(s.to_string())
    }
}

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(
        runtime
            .context
            .ipc_call(DATABASE_PLUGIN, command, args, IPC_TIMEOUT),
    )
    .with_context(|| format!("database ipc call '{command}' failed"))
}

/// Extract `csv_bytes` from a `csv_export` response. The handler
/// serializes `Vec<u8>` as a JSON array of integers.
fn extract_csv_bytes(response: &Value) -> Option<Vec<u8>> {
    let arr = response.get("csv_bytes")?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let n = v.as_u64()?;
        if n > u8::MAX as u64 {
            return None;
        }
        out.push(n as u8);
    }
    Some(out)
}

// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn csv_import_args_serialize_to_expected_shape() {
        let v = build_csv_import_args(b"hi", &["a".to_string()], true, None).unwrap();
        assert!(
            v["csv_bytes"].is_array(),
            "csv_bytes should be a JSON array"
        );
        assert_eq!(v["csv_bytes"][0], 104); // 'h'
        assert_eq!(v["csv_bytes"][1], 105); // 'i'
        assert_eq!(v["field_names"][0], "a");
        assert_eq!(v["has_header"], true);
        assert!(v["column_mapping"].is_null());
    }

    #[test]
    fn csv_import_args_with_column_mapping_round_trips() {
        let mapping = r#"[[0, "name"], [1, "score"]]"#;
        let v = build_csv_import_args(b"x", &["name".to_string()], false, Some(mapping)).unwrap();
        assert_eq!(v["column_mapping"][0][0], 0);
        assert_eq!(v["column_mapping"][0][1], "name");
        assert_eq!(v["column_mapping"][1][1], "score");
    }

    #[test]
    fn csv_import_args_bad_mapping_json_returns_err() {
        let err = build_csv_import_args(b"x", &[], false, Some("not-json"));
        assert!(err.is_err(), "invalid mapping JSON should bubble up");
    }

    #[test]
    fn csv_export_args_defaults_records_and_fields() {
        let v = build_csv_export_args(None, None).unwrap();
        assert!(v["records"].is_array());
        assert_eq!(v["records"].as_array().unwrap().len(), 0);
        assert!(v["field_names"].is_array());
        assert_eq!(v["field_names"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn csv_export_args_round_trips_records_and_fields() {
        let records = r#"[{"id":"r1","fields":{"name":"A"}}]"#;
        let fields = vec!["name".to_string()];
        let v = build_csv_export_args(Some(records), Some(&fields)).unwrap();
        assert_eq!(v["records"][0]["id"], "r1");
        assert_eq!(v["field_names"][0], "name");
    }

    #[test]
    fn eval_formula_args_serialize_to_expected_shape() {
        let v = build_eval_formula_args("expr", r#"{"k":1}"#).unwrap();
        assert_eq!(v["expression"], "expr");
        assert_eq!(v["fields"]["k"], 1);
    }

    #[test]
    fn eval_formula_args_bad_json_returns_err() {
        let err = build_eval_formula_args("expr", "not-json");
        assert!(err.is_err(), "invalid fields JSON should bubble up");
    }

    #[test]
    fn resolve_inline_or_file_handles_at_prefix() {
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        tmp.write_all(b"hello").expect("write tempfile");
        let path = tmp.path().to_string_lossy().to_string();
        let arg = format!("@{path}");
        let resolved = resolve_inline_or_file(&arg).expect("resolve @<file>");
        assert_eq!(resolved, "hello");
    }

    #[test]
    fn resolve_inline_or_file_returns_literal_when_no_prefix() {
        let resolved = resolve_inline_or_file("inline").expect("resolve literal");
        assert_eq!(resolved, "inline");
    }

    #[test]
    fn extract_csv_bytes_decodes_byte_array() {
        let resp = serde_json::json!({ "csv_bytes": [104, 105], "count": 0 });
        let bytes = extract_csv_bytes(&resp).expect("decode bytes");
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn extract_csv_bytes_rejects_oversized_value() {
        let resp = serde_json::json!({ "csv_bytes": [256] });
        assert!(extract_csv_bytes(&resp).is_none());
    }
}
