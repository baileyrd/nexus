//! Cross-base relation resolution + rollup aggregation (PRD-10 §7 — DG-41).
//!
//! The `BaseRelation` type from `nexus-types` describes a relation
//! between two bases (source field → target base + target field) but
//! the runtime that *resolves* relations (and computes rollups over
//! them) didn't exist before DG-41.
//!
//! This module is pure (no kernel ctx, no disk I/O) — same posture as
//! `views::apply_view`. The IPC handler at
//! `com.nexus.database::resolve_relation` / `::compute_rollup` loads
//! the source + target records via the existing storage handlers and
//! then calls into here.
//!
//! ## Resolution semantics
//!
//! A relation's `source_field` carries a scalar (string id) or an
//! array of strings — both are accepted, mirroring how Bases stores
//! many-to-one vs. many-to-many.
//!
//! - For each source value, find target records whose
//!   `target_field` equals it.
//! - Order of returned records matches the source-side value order
//!   when arrays are used; for scalar source values, results are
//!   in target-side insertion order.

use crate::types::RollupAggregation;
use nexus_types::bases::{BaseRecord, BaseRelation};

/// Errors returned by relation resolution / rollup.
#[derive(Debug, thiserror::Error)]
pub enum RelationError {
    /// Source record didn't have the relation's `source_field`.
    #[error("source record {0} missing source_field `{1}`")]
    MissingSourceField(String, String),
    /// `source_field` value couldn't be coerced to a string or list
    /// of strings.
    #[error("source_field `{0}` on record `{1}` has incompatible type for relation resolution")]
    UnsupportedSourceShape(String, String),
}

/// Resolve a relation: return target records that match the source
/// record's `source_field`.
///
/// Scalar source values match a single target record (or none); list
/// source values match in the order they appear. Soft-deleted
/// (deleted_at != None) target records are filtered out so the UI
/// never sees stale references.
///
/// # Errors
/// See [`RelationError`].
pub fn resolve_relation<'a>(
    source: &BaseRecord,
    relation: &BaseRelation,
    target_records: &'a [BaseRecord],
) -> Result<Vec<&'a BaseRecord>, RelationError> {
    let raw = source.fields.get(&relation.source_field).ok_or_else(|| {
        RelationError::MissingSourceField(source.id.clone(), relation.source_field.clone())
    })?;

    let lookups = source_lookup_keys(raw, source, &relation.source_field)?;
    if lookups.is_empty() {
        return Ok(Vec::new());
    }

    let live_targets: Vec<&BaseRecord> = target_records
        .iter()
        .filter(|r| r.deleted_at.is_none())
        .collect();

    // For each lookup value (in order), find the first matching
    // target record. Per PRD-10 §7 a relation may be one-to-many,
    // so the same lookup value can match multiple targets; collect
    // them all in target-side insertion order.
    let mut out: Vec<&BaseRecord> = Vec::new();
    for key in &lookups {
        for target in &live_targets {
            if target_field_matches(target, &relation.target_field, key) {
                out.push(target);
            }
        }
    }
    // Deduplicate by id while preserving order — a target record
    // referenced by both array positions in the source shouldn't
    // appear twice.
    let mut seen = std::collections::HashSet::new();
    out.retain(|r| seen.insert(r.id.clone()));
    Ok(out)
}

fn source_lookup_keys(
    raw: &serde_json::Value,
    source: &BaseRecord,
    field_name: &str,
) -> Result<Vec<String>, RelationError> {
    match raw {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(s) => Ok(vec![s.clone()]),
        serde_json::Value::Number(n) => Ok(vec![n.to_string()]),
        serde_json::Value::Array(items) => {
            let mut keys = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    serde_json::Value::String(s) => keys.push(s.clone()),
                    serde_json::Value::Number(n) => keys.push(n.to_string()),
                    serde_json::Value::Null => {}
                    _ => {
                        return Err(RelationError::UnsupportedSourceShape(
                            field_name.to_string(),
                            source.id.clone(),
                        ));
                    }
                }
            }
            Ok(keys)
        }
        _ => Err(RelationError::UnsupportedSourceShape(
            field_name.to_string(),
            source.id.clone(),
        )),
    }
}

fn target_field_matches(target: &BaseRecord, target_field: &str, key: &str) -> bool {
    // Special case: when target_field is "id", match the record's
    // primary id (BaseRecord has a typed `id`, not a key in `fields`).
    if target_field == "id" {
        return target.id == key;
    }
    let Some(value) = target.fields.get(target_field) else {
        return false;
    };
    match value {
        serde_json::Value::String(s) => s == key,
        serde_json::Value::Number(n) => n.to_string() == key,
        serde_json::Value::Array(items) => items.iter().any(|v| match v {
            serde_json::Value::String(s) => s == key,
            serde_json::Value::Number(n) => n.to_string() == key,
            _ => false,
        }),
        _ => false,
    }
}

/// Compute a rollup: aggregate the `aggregate_field` values across
/// every record returned by [`resolve_relation`].
///
/// Returns a JSON `Value` so the result rides over IPC without
/// re-coupling to the formula engine. Callers project the result
/// into their own renderer.
///
/// # Errors
/// Same as [`resolve_relation`].
pub fn compute_rollup(
    source: &BaseRecord,
    relation: &BaseRelation,
    aggregate_field: &str,
    aggregation: RollupAggregation,
    target_records: &[BaseRecord],
) -> Result<serde_json::Value, RelationError> {
    let related = resolve_relation(source, relation, target_records)?;

    // Count variants are special — they operate on the relation as
    // a whole (or the projected raw values), not on numeric coercion.
    let raw_values: Vec<&serde_json::Value> =
        related.iter().filter_map(|r| r.fields.get(aggregate_field)).collect();

    Ok(match aggregation {
        RollupAggregation::Count => serde_json::json!(related.len()),
        RollupAggregation::CountUnique => {
            let mut seen = std::collections::HashSet::new();
            for v in &raw_values {
                seen.insert(v.to_string());
            }
            serde_json::json!(seen.len())
        }
        RollupAggregation::CountValues | RollupAggregation::CountNotEmpty => {
            let non_empty = raw_values.iter().filter(|v| !v.is_null()).count();
            serde_json::json!(non_empty)
        }
        RollupAggregation::CountEmpty => {
            let empty = raw_values.iter().filter(|v| v.is_null()).count();
            serde_json::json!(empty)
        }
        RollupAggregation::PercentEmpty => percentage(&raw_values, true),
        RollupAggregation::PercentNotEmpty => percentage(&raw_values, false),
        RollupAggregation::Sum => {
            let total: f64 = raw_values
                .iter()
                .filter(|v| !v.is_null())
                .filter_map(|v| v.as_f64())
                .sum();
            serde_json::json!(total)
        }
        RollupAggregation::Average => {
            let nums: Vec<f64> = raw_values
                .iter()
                .filter(|v| !v.is_null())
                .filter_map(|v| v.as_f64())
                .collect();
            if nums.is_empty() {
                serde_json::Value::Null
            } else {
                #[allow(clippy::cast_precision_loss)]
                let avg = nums.iter().sum::<f64>() / nums.len() as f64;
                serde_json::json!(avg)
            }
        }
        RollupAggregation::Min => raw_values
            .iter()
            .filter(|v| !v.is_null())
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, n| match acc {
                None => Some(n),
                Some(cur) => Some(cur.min(n)),
            })
            .map_or(serde_json::Value::Null, |n| serde_json::json!(n)),
        RollupAggregation::Max => raw_values
            .iter()
            .filter(|v| !v.is_null())
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, n| match acc {
                None => Some(n),
                Some(cur) => Some(cur.max(n)),
            })
            .map_or(serde_json::Value::Null, |n| serde_json::json!(n)),
    })
}

fn percentage(values: &[&serde_json::Value], target_empty: bool) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::Value::Null;
    }
    let count = values
        .iter()
        .filter(|v| v.is_null() == target_empty)
        .count();
    #[allow(clippy::cast_precision_loss)]
    let pct = (count as f64 / values.len() as f64) * 100.0;
    serde_json::json!(pct)
}

/// Parse a string into [`RollupAggregation`]. Accepts the same
/// snake_case form the serde tag uses; case-insensitive.
#[must_use]
pub fn parse_aggregation(s: &str) -> Option<RollupAggregation> {
    match s.to_ascii_lowercase().as_str() {
        "count" => Some(RollupAggregation::Count),
        "count_unique" | "countunique" => Some(RollupAggregation::CountUnique),
        "count_values" | "countvalues" => Some(RollupAggregation::CountValues),
        "count_empty" | "countempty" => Some(RollupAggregation::CountEmpty),
        "count_not_empty" | "countnotempty" => Some(RollupAggregation::CountNotEmpty),
        "percent_empty" | "percentempty" => Some(RollupAggregation::PercentEmpty),
        "percent_not_empty" | "percentnotempty" => Some(RollupAggregation::PercentNotEmpty),
        "sum" => Some(RollupAggregation::Sum),
        "average" | "avg" | "mean" => Some(RollupAggregation::Average),
        "min" => Some(RollupAggregation::Min),
        "max" => Some(RollupAggregation::Max),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, fields: serde_json::Value) -> BaseRecord {
        let serde_json::Value::Object(map) = fields else {
            panic!("test record fields must be object");
        };
        BaseRecord {
            id: id.to_string(),
            deleted_at: None,
            fields: map,
        }
    }

    fn relation(source_field: &str, target_field: &str) -> BaseRelation {
        BaseRelation {
            name: "rel".into(),
            relation_type: "one_to_many".into(),
            source_field: source_field.into(),
            target_base: "target.bases".into(),
            target_field: target_field.into(),
        }
    }

    #[test]
    fn resolve_scalar_string_source_matches_one_target() {
        let src = record("p1", json!({ "assignee": "u1" }));
        let targets = vec![
            record("u1", json!({ "name": "Alice" })),
            record("u2", json!({ "name": "Bob" })),
        ];
        let resolved = resolve_relation(&src, &relation("assignee", "id"), &targets).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "u1");
    }

    #[test]
    fn resolve_array_source_matches_in_order() {
        let src = record("p1", json!({ "tags": ["t2", "t1"] }));
        let targets = vec![
            record("t1", json!({ "label": "first" })),
            record("t2", json!({ "label": "second" })),
        ];
        let resolved = resolve_relation(&src, &relation("tags", "id"), &targets).unwrap();
        assert_eq!(resolved.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["t2", "t1"]);
    }

    #[test]
    fn resolve_filters_out_soft_deleted_targets() {
        let src = record("p1", json!({ "owner": "u1" }));
        let mut targets = vec![
            record("u1", json!({})),
            record("u1", json!({})),
        ];
        targets[0].deleted_at = Some(123);
        let resolved = resolve_relation(&src, &relation("owner", "id"), &targets).unwrap();
        // Only the second (live) record at id "u1" matches.
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn resolve_dedupes_duplicate_lookups_preserving_first_match() {
        let src = record("p1", json!({ "tags": ["t1", "t1"] }));
        let targets = vec![record("t1", json!({}))];
        let resolved = resolve_relation(&src, &relation("tags", "id"), &targets).unwrap();
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn resolve_returns_empty_when_source_field_is_null() {
        let src = record("p1", json!({ "owner": null }));
        let targets = vec![record("u1", json!({}))];
        let resolved = resolve_relation(&src, &relation("owner", "id"), &targets).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_errors_when_source_field_missing() {
        let src = record("p1", json!({}));
        let err = resolve_relation(&src, &relation("owner", "id"), &[]).expect_err("should error");
        assert!(matches!(err, RelationError::MissingSourceField(_, _)));
    }

    #[test]
    fn resolve_against_non_id_target_field() {
        let src = record("p1", json!({ "category": "alpha" }));
        let targets = vec![
            record("t1", json!({ "category_name": "alpha" })),
            record("t2", json!({ "category_name": "beta" })),
            record("t3", json!({ "category_name": "alpha" })),
        ];
        let resolved =
            resolve_relation(&src, &relation("category", "category_name"), &targets).unwrap();
        assert_eq!(resolved.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["t1", "t3"]);
    }

    #[test]
    fn rollup_count_returns_related_record_count() {
        let src = record("p1", json!({ "tags": ["t1", "t2", "t3"] }));
        let targets = vec![
            record("t1", json!({})),
            record("t2", json!({})),
            record("t3", json!({})),
        ];
        let v = compute_rollup(
            &src,
            &relation("tags", "id"),
            "anything",
            RollupAggregation::Count,
            &targets,
        )
        .unwrap();
        assert_eq!(v, json!(3));
    }

    #[test]
    fn rollup_sum_adds_numeric_fields() {
        let src = record("p1", json!({ "items": ["i1", "i2"] }));
        let targets = vec![
            record("i1", json!({ "price": 10.0 })),
            record("i2", json!({ "price": 5.5 })),
        ];
        let v = compute_rollup(
            &src,
            &relation("items", "id"),
            "price",
            RollupAggregation::Sum,
            &targets,
        )
        .unwrap();
        assert_eq!(v, json!(15.5));
    }

    #[test]
    fn rollup_average_handles_empty_input() {
        let src = record("p1", json!({ "items": [] }));
        let v = compute_rollup(
            &src,
            &relation("items", "id"),
            "price",
            RollupAggregation::Average,
            &[],
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::Null);
    }

    #[test]
    fn rollup_min_max_numeric_aggregations() {
        let src = record("p1", json!({ "items": ["i1", "i2", "i3"] }));
        let targets = vec![
            record("i1", json!({ "n": 10 })),
            record("i2", json!({ "n": 3 })),
            record("i3", json!({ "n": 7 })),
        ];
        let rel = relation("items", "id");
        assert_eq!(
            compute_rollup(&src, &rel, "n", RollupAggregation::Min, &targets).unwrap(),
            json!(3.0)
        );
        assert_eq!(
            compute_rollup(&src, &rel, "n", RollupAggregation::Max, &targets).unwrap(),
            json!(10.0)
        );
    }

    #[test]
    fn rollup_count_unique_counts_distinct_values() {
        let src = record("p1", json!({ "items": ["i1", "i2", "i3"] }));
        let targets = vec![
            record("i1", json!({ "cat": "a" })),
            record("i2", json!({ "cat": "b" })),
            record("i3", json!({ "cat": "a" })),
        ];
        let v = compute_rollup(
            &src,
            &relation("items", "id"),
            "cat",
            RollupAggregation::CountUnique,
            &targets,
        )
        .unwrap();
        assert_eq!(v, json!(2));
    }

    #[test]
    fn rollup_count_values_counts_non_null() {
        let src = record("p1", json!({ "items": ["i1", "i2", "i3"] }));
        let targets = vec![
            record("i1", json!({ "label": "a" })),
            record("i2", json!({ "label": null })),
            record("i3", json!({})),
        ];
        let v = compute_rollup(
            &src,
            &relation("items", "id"),
            "label",
            RollupAggregation::CountValues,
            &targets,
        )
        .unwrap();
        // i1 has a non-null value; i2 explicitly null; i3 missing the
        // field entirely (excluded by the projection). 1 non-empty.
        assert_eq!(v, json!(1));
    }

    #[test]
    fn rollup_percent_empty_handles_null_projection() {
        let src = record("p1", json!({ "items": ["i1", "i2"] }));
        let targets = vec![
            record("i1", json!({ "label": "a" })),
            record("i2", json!({ "label": null })),
        ];
        let v = compute_rollup(
            &src,
            &relation("items", "id"),
            "label",
            RollupAggregation::PercentEmpty,
            &targets,
        )
        .unwrap();
        // 1 of 2 projected values is null = 50%.
        assert_eq!(v, json!(50.0));
    }

    #[test]
    fn parse_aggregation_accepts_known_names_case_insensitively() {
        for (input, expected) in [
            ("count", RollupAggregation::Count),
            ("count_unique", RollupAggregation::CountUnique),
            ("count_values", RollupAggregation::CountValues),
            ("count_empty", RollupAggregation::CountEmpty),
            ("count_not_empty", RollupAggregation::CountNotEmpty),
            ("percent_empty", RollupAggregation::PercentEmpty),
            ("percent_not_empty", RollupAggregation::PercentNotEmpty),
            ("SUM", RollupAggregation::Sum),
            ("Average", RollupAggregation::Average),
            ("avg", RollupAggregation::Average),
            ("mean", RollupAggregation::Average),
            ("min", RollupAggregation::Min),
            ("max", RollupAggregation::Max),
        ] {
            assert_eq!(parse_aggregation(input), Some(expected), "input: {input}");
        }
        assert!(parse_aggregation("median").is_none());
        assert!(parse_aggregation("concat").is_none()); // not in existing enum
    }
}
