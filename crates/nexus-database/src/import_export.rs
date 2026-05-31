//! CSV import and export for database records.

use std::io::{Read, Write};

use crate::error::{DatabaseError, Result};

/// Result of a CSV import operation.
#[derive(Debug)]
pub struct ImportResult {
    /// Number of records successfully imported.
    pub imported: usize,
    /// Number of records skipped due to errors.
    pub skipped: usize,
    /// Per-row errors: (`row_number`, `error_message`).
    pub errors: Vec<(usize, String)>,
}

/// Column mapping for CSV import.
///
/// Maps CSV column indices (0-based) to database field names.
#[derive(Debug, Clone)]
pub struct ColumnMapping {
    /// `(csv_column_index, field_name)` pairs.
    pub mappings: Vec<(usize, String)>,
}

impl ColumnMapping {
    /// Create a mapping from CSV header names that match field names exactly.
    #[must_use]
    pub fn from_headers(headers: &csv::StringRecord, field_names: &[String]) -> Self {
        let mut mappings = Vec::new();
        for (i, header) in headers.iter().enumerate() {
            if field_names.contains(&header.to_string()) {
                mappings.push((i, header.to_string()));
            }
        }
        Self { mappings }
    }
}

/// Import records from a CSV reader into a base.
///
/// Reads the CSV, maps columns to fields using `mapping`, generates UUIDs
/// for record IDs, and returns an [`ImportResult`] with counts and errors.
///
/// Records are returned as a vector — the caller is responsible for
/// persisting them (via `save_base` + `insert_base`).
///
/// # Errors
///
/// Returns `DatabaseError::ImportExportError` on CSV parse failure.
pub fn import_csv<R: Read>(
    reader: R,
    mapping: &ColumnMapping,
    has_header: bool,
) -> Result<(Vec<nexus_types::bases::BaseRecord>, ImportResult)> {
    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(has_header)
        .from_reader(reader);

    let mut records = Vec::new();
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
    };

    for (row_idx, row_result) in csv_reader.records().enumerate() {
        let row_num = row_idx + if has_header { 2 } else { 1 };

        let row = match row_result {
            Ok(r) => r,
            Err(e) => {
                result
                    .errors
                    .push((row_num, format!("CSV parse error: {e}")));
                result.skipped += 1;
                continue;
            }
        };

        let mut fields = serde_json::Map::new();
        let record_id = uuid::Uuid::new_v4().to_string();
        fields.insert(
            "id".to_string(),
            serde_json::Value::String(record_id.clone()),
        );

        for (col_idx, field_name) in &mapping.mappings {
            if let Some(value) = row.get(*col_idx) {
                let json_value = csv_value_to_json(value);
                fields.insert(field_name.clone(), json_value);
            }
        }

        records.push(nexus_types::bases::BaseRecord {
            id: record_id,
            deleted_at: None,
            fields,
        });
        result.imported += 1;
    }

    Ok((records, result))
}

/// Export base records to a CSV writer.
///
/// Writes a header row with the specified field names, then one row per
/// record. Returns the number of records written.
///
/// # Errors
///
/// Returns `DatabaseError::ImportExportError` on I/O failure.
pub fn export_csv<W: Write>(
    writer: W,
    records: &[nexus_types::bases::BaseRecord],
    field_names: &[String],
) -> Result<usize> {
    let mut csv_writer = csv::Writer::from_writer(writer);

    // Write header.
    csv_writer
        .write_record(field_names)
        .map_err(|e| DatabaseError::ImportExportError(format!("header write failed: {e}")))?;

    // Write records.
    for record in records {
        let row: Vec<String> = field_names
            .iter()
            .map(|name| {
                record
                    .fields
                    .get(name)
                    .map(json_value_to_csv)
                    .unwrap_or_default()
            })
            .collect();
        csv_writer
            .write_record(&row)
            .map_err(|e| DatabaseError::ImportExportError(format!("row write failed: {e}")))?;
    }

    csv_writer
        .flush()
        .map_err(|e| DatabaseError::ImportExportError(format!("flush failed: {e}")))?;

    Ok(records.len())
}

/// Convert a CSV cell value to a `serde_json::Value`.
///
/// Attempts to parse as number or boolean first, falls back to string.
fn csv_value_to_json(value: &str) -> serde_json::Value {
    if value.is_empty() {
        return serde_json::Value::Null;
    }
    if value.eq_ignore_ascii_case("true") {
        return serde_json::Value::Bool(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return serde_json::Value::Bool(false);
    }
    if let Ok(n) = value.parse::<f64>() {
        return serde_json::json!(n);
    }
    serde_json::Value::String(value.to_string())
}

/// Convert a JSON value to a CSV-friendly string.
fn json_value_to_csv(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(json_value_to_csv).collect();
            parts.join("; ")
        }
        serde_json::Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_basic_csv() {
        let csv_data = "name,status,priority\nBuy milk,todo,1\nWrite tests,done,3\n";
        let mapping = ColumnMapping {
            mappings: vec![
                (0, "name".to_string()),
                (1, "status".to_string()),
                (2, "priority".to_string()),
            ],
        };
        let (records, result) = import_csv(csv_data.as_bytes(), &mapping, true).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].fields.get("name").unwrap(), "Buy milk");
        assert_eq!(records[1].fields.get("status").unwrap(), "done");
    }

    #[test]
    fn import_auto_detects_types() {
        let csv_data = "val\n42\ntrue\nhello\n";
        let mapping = ColumnMapping {
            mappings: vec![(0, "val".to_string())],
        };
        let (records, _) = import_csv(csv_data.as_bytes(), &mapping, true).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[0].fields.get("val").unwrap(),
            &serde_json::json!(42.0)
        );
        assert_eq!(
            records[1].fields.get("val").unwrap(),
            &serde_json::json!(true)
        );
        assert_eq!(records[2].fields.get("val").unwrap(), "hello");
    }

    #[test]
    fn export_basic_csv() {
        let records = vec![
            nexus_types::bases::BaseRecord {
                id: "r1".to_string(),
                deleted_at: None,
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("name".to_string(), serde_json::json!("Alice"));
                    m.insert("score".to_string(), serde_json::json!(95));
                    m
                },
            },
            nexus_types::bases::BaseRecord {
                id: "r2".to_string(),
                deleted_at: None,
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("name".to_string(), serde_json::json!("Bob"));
                    m.insert("score".to_string(), serde_json::json!(87));
                    m
                },
            },
        ];

        let fields = vec!["name".to_string(), "score".to_string()];
        let mut buf = Vec::new();
        let count = export_csv(&mut buf, &records, &fields).unwrap();
        assert_eq!(count, 2);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("name,score"));
        assert!(output.contains("Alice,95"));
        assert!(output.contains("Bob,87"));
    }

    #[test]
    fn roundtrip_csv() {
        let records = vec![nexus_types::bases::BaseRecord {
            id: "r1".to_string(),
            deleted_at: None,
            fields: {
                let mut m = serde_json::Map::new();
                m.insert("title".to_string(), serde_json::json!("Test"));
                m.insert("count".to_string(), serde_json::json!(42));
                m
            },
        }];

        let fields = vec!["title".to_string(), "count".to_string()];
        let mut buf = Vec::new();
        export_csv(&mut buf, &records, &fields).unwrap();

        let mapping = ColumnMapping {
            mappings: vec![(0, "title".to_string()), (1, "count".to_string())],
        };
        let (imported, result) = import_csv(buf.as_slice(), &mapping, true).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(imported[0].fields.get("title").unwrap(), "Test");
        assert_eq!(
            imported[0].fields.get("count").unwrap(),
            &serde_json::json!(42.0)
        );
    }

    #[test]
    fn column_mapping_from_headers() {
        let headers = csv::StringRecord::from(vec!["name", "age", "email"]);
        let field_names = vec!["name".to_string(), "email".to_string()];
        let mapping = ColumnMapping::from_headers(&headers, &field_names);
        assert_eq!(mapping.mappings.len(), 2);
        assert_eq!(mapping.mappings[0], (0, "name".to_string()));
        assert_eq!(mapping.mappings[1], (2, "email".to_string()));
    }
}
