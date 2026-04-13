//! Cross-database relation resolution and rollup aggregation.

use rusqlite::Connection;

use crate::error::{DatabaseError, Result};
use crate::formula::eval::FormulaValue;
use crate::types::RollupAggregation;

/// Resolve a relation: load the referenced records from a target base
/// by their record IDs.
///
/// # Errors
///
/// Returns `DatabaseError::RelationError` on `SQLite` query failure.
pub fn resolve_relation(
    conn: &Connection,
    target_base_id: i64,
    record_ids: &[String],
) -> Result<Vec<nexus_storage::bases::BaseRecord>> {
    if record_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = record_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 2)).collect();
    let sql = format!(
        "SELECT data_json FROM bases_records WHERE base_id = ?1 AND record_id IN ({})",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        DatabaseError::RelationError(format!("prepare failed: {e}"))
    })?;

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(target_base_id)];
    for id in record_ids {
        params.push(Box::new(id.clone()));
    }

    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(AsRef::as_ref)),
            |row| {
                let json_str: String = row.get(0)?;
                Ok(json_str)
            },
        )
        .map_err(|e| DatabaseError::RelationError(format!("query failed: {e}")))?;

    let mut records = Vec::new();
    for row in rows {
        let json_str = row.map_err(|e| {
            DatabaseError::RelationError(format!("row read failed: {e}"))
        })?;
        let record: nexus_storage::bases::BaseRecord =
            serde_json::from_str(&json_str).map_err(|e| {
                DatabaseError::RelationError(format!("deserialize failed: {e}"))
            })?;
        records.push(record);
    }

    Ok(records)
}

/// Compute a rollup aggregation over related records.
///
/// 1. Extracts the relation field value from `source_record` (list of target IDs)
/// 2. Resolves the relation to load target records
/// 3. Extracts `target_property` from each target record
/// 4. Applies the aggregation function
///
/// # Errors
///
/// Returns `DatabaseError::RelationError` on resolution or aggregation failure.
pub fn compute_rollup(
    conn: &Connection,
    source_record: &nexus_storage::bases::BaseRecord,
    relation_field: &str,
    target_base_id: i64,
    target_property: &str,
    aggregation: RollupAggregation,
) -> Result<FormulaValue> {
    // Extract relation value (array of record IDs).
    let relation_value = source_record.fields.get(relation_field);
    let record_ids: Vec<String> = match relation_value {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => return Ok(FormulaValue::Null),
    };

    // Resolve the related records.
    let related = resolve_relation(conn, target_base_id, &record_ids)?;

    // Extract the target property values.
    let values: Vec<Option<&serde_json::Value>> = related
        .iter()
        .map(|r| r.fields.get(target_property))
        .collect();

    // Apply aggregation.
    #[allow(clippy::cast_precision_loss)]
    match aggregation {
        RollupAggregation::Count => {
            Ok(FormulaValue::Number(values.len() as f64))
        }
        RollupAggregation::CountValues | RollupAggregation::CountNotEmpty => {
            let count = values.iter().filter(|v| !is_empty_value(**v)).count();
            Ok(FormulaValue::Number(count as f64))
        }
        RollupAggregation::CountEmpty => {
            let count = values.iter().filter(|v| is_empty_value(**v)).count();
            Ok(FormulaValue::Number(count as f64))
        }
        RollupAggregation::CountUnique => {
            let mut unique = std::collections::HashSet::new();
            for val in values.iter().flatten() {
                unique.insert(val.to_string());
            }
            Ok(FormulaValue::Number(unique.len() as f64))
        }
        RollupAggregation::Sum => {
            let sum: f64 = values.iter().filter_map(|v| v.and_then(serde_json::Value::as_f64)).sum();
            Ok(FormulaValue::Number(sum))
        }
        RollupAggregation::Average => {
            let nums: Vec<f64> = values.iter().filter_map(|v| v.and_then(serde_json::Value::as_f64)).collect();
            if nums.is_empty() {
                return Ok(FormulaValue::Null);
            }
            Ok(FormulaValue::Number(
                nums.iter().sum::<f64>() / nums.len() as f64,
            ))
        }
        RollupAggregation::Min => {
            let min = values
                .iter()
                .filter_map(|v| v.and_then(serde_json::Value::as_f64))
                .reduce(f64::min);
            match min {
                Some(n) => Ok(FormulaValue::Number(n)),
                None => Ok(FormulaValue::Null),
            }
        }
        RollupAggregation::Max => {
            let max = values
                .iter()
                .filter_map(|v| v.and_then(serde_json::Value::as_f64))
                .reduce(f64::max);
            match max {
                Some(n) => Ok(FormulaValue::Number(n)),
                None => Ok(FormulaValue::Null),
            }
        }
        RollupAggregation::PercentEmpty => {
            if values.is_empty() {
                return Ok(FormulaValue::Number(0.0));
            }
            let empty = values.iter().filter(|v| is_empty_value(**v)).count();
            Ok(FormulaValue::Number(
                (empty as f64 / values.len() as f64) * 100.0,
            ))
        }
        RollupAggregation::PercentNotEmpty => {
            if values.is_empty() {
                return Ok(FormulaValue::Number(0.0));
            }
            let not_empty = values.iter().filter(|v| !is_empty_value(**v)).count();
            Ok(FormulaValue::Number(
                (not_empty as f64 / values.len() as f64) * 100.0,
            ))
        }
    }
}

fn is_empty_value(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        nexus_storage::schema::configure_pragmas(&conn).unwrap();
        nexus_storage::schema::migrate(&conn).unwrap();

        // Create target base.
        conn.execute(
            "INSERT INTO bases (path, name, schema_json, created_at, modified_at) VALUES ('target.bases', 'Target', '{}', 0, 0)",
            [],
        ).unwrap();
        let base_id = conn.last_insert_rowid();

        // Insert target records.
        for (id, score) in &[("t1", 10), ("t2", 20), ("t3", 30)] {
            let data = serde_json::json!({"id": id, "score": score, "status": "done"});
            conn.execute(
                "INSERT INTO bases_records (base_id, record_id, data_json, created_at, modified_at) VALUES (?1, ?2, ?3, 0, 0)",
                rusqlite::params![base_id, id, data.to_string()],
            ).unwrap();
        }

        conn
    }

    #[test]
    fn resolve_empty_ids() {
        let conn = setup_db();
        let result = resolve_relation(&conn, 1, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn resolve_existing_records() {
        let conn = setup_db();
        let ids = vec!["t1".to_string(), "t3".to_string()];
        let result = resolve_relation(&conn, 1, &ids).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn rollup_sum() {
        let conn = setup_db();
        let mut fields = serde_json::Map::new();
        fields.insert("related".to_string(), serde_json::json!(["t1", "t2", "t3"]));
        let record = nexus_storage::bases::BaseRecord {
            id: "source".to_string(),
            fields,
        };
        let result = compute_rollup(&conn, &record, "related", 1, "score", RollupAggregation::Sum).unwrap();
        assert_eq!(result, FormulaValue::Number(60.0));
    }

    #[test]
    fn rollup_average() {
        let conn = setup_db();
        let mut fields = serde_json::Map::new();
        fields.insert("related".to_string(), serde_json::json!(["t1", "t2", "t3"]));
        let record = nexus_storage::bases::BaseRecord {
            id: "source".to_string(),
            fields,
        };
        let result = compute_rollup(&conn, &record, "related", 1, "score", RollupAggregation::Average).unwrap();
        assert_eq!(result, FormulaValue::Number(20.0));
    }

    #[test]
    fn rollup_count() {
        let conn = setup_db();
        let mut fields = serde_json::Map::new();
        fields.insert("related".to_string(), serde_json::json!(["t1", "t2"]));
        let record = nexus_storage::bases::BaseRecord {
            id: "source".to_string(),
            fields,
        };
        let result = compute_rollup(&conn, &record, "related", 1, "score", RollupAggregation::Count).unwrap();
        assert_eq!(result, FormulaValue::Number(2.0));
    }

    #[test]
    fn rollup_min_max() {
        let conn = setup_db();
        let mut fields = serde_json::Map::new();
        fields.insert("related".to_string(), serde_json::json!(["t1", "t2", "t3"]));
        let record = nexus_storage::bases::BaseRecord {
            id: "source".to_string(),
            fields,
        };
        let min = compute_rollup(&conn, &record, "related", 1, "score", RollupAggregation::Min).unwrap();
        let max = compute_rollup(&conn, &record, "related", 1, "score", RollupAggregation::Max).unwrap();
        assert_eq!(min, FormulaValue::Number(10.0));
        assert_eq!(max, FormulaValue::Number(30.0));
    }

    #[test]
    fn rollup_missing_relation_field() {
        let conn = setup_db();
        let record = nexus_storage::bases::BaseRecord {
            id: "source".to_string(),
            fields: serde_json::Map::new(),
        };
        let result = compute_rollup(&conn, &record, "missing", 1, "score", RollupAggregation::Sum).unwrap();
        assert!(matches!(result, FormulaValue::Null));
    }
}
