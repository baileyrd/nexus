//! Query engine: compile structured filters and sorts to SQL, execute against
//! the `bases_records` table, return paginated results.
//!
//! Filtering uses `json_extract()` on the `data_json` column to access
//! individual field values stored inside each record's JSON blob.

use std::fmt::Write;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use nexus_database::{DatabaseError, Result};

// ── Query types ─────────────────────────────────────────────────────────────

/// A structured query against a single base.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// The base (database) to query.
    pub base_id: i64,
    /// Filter conditions (`AND`ed together).
    pub filters: Vec<Filter>,
    /// Sort order (applied in sequence).
    pub sorts: Vec<Sort>,
    /// Maximum number of records to return.
    pub limit: Option<u32>,
    /// Number of records to skip (for pagination).
    pub offset: Option<u32>,
}

/// A single filter condition on a field.
#[derive(Debug, Clone)]
pub struct Filter {
    /// Field name to filter on.
    pub field: String,
    /// Comparison operator.
    pub operator: FilterOp,
    /// Value to compare against.
    pub value: serde_json::Value,
}

/// Filter comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Exact equality.
    Is,
    /// Not equal.
    IsNot,
    /// Substring match (case-insensitive).
    Contains,
    /// Does not contain substring.
    DoesNotContain,
    /// String starts with prefix.
    StartsWith,
    /// String ends with suffix.
    EndsWith,
    /// Numeric greater than.
    GreaterThan,
    /// Numeric less than.
    LessThan,
    /// Numeric greater than or equal.
    GreaterThanOrEqual,
    /// Numeric less than or equal.
    LessThanOrEqual,
    /// Value is null or empty string.
    IsEmpty,
    /// Value is not null and not empty string.
    IsNotEmpty,
    /// Date is strictly before.
    DateIsBefore,
    /// Date is strictly after.
    DateIsAfter,
    /// Date is on or before.
    DateIsOnOrBefore,
    /// Date is on or after.
    DateIsOnOrAfter,
}

/// Sort specification for a single field.
#[derive(Debug, Clone)]
pub struct Sort {
    /// Field name to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending (A→Z, 0→9, oldest→newest).
    #[default]
    Ascending,
    /// Descending (Z→A, 9→0, newest→oldest).
    Descending,
}

/// Result of a query execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// The matching records (after LIMIT/OFFSET).
    pub records: Vec<nexus_types::bases::BaseRecord>,
    /// Total number of matching records (before LIMIT/OFFSET).
    pub total_count: u64,
    /// Whether there are more records beyond the current page.
    pub has_more: bool,
}

// ── SQL compilation ─────────────────────────────────────────────────────────

/// Compile a filter into a SQL WHERE clause fragment and bind parameters.
fn compile_filter(filter: &Filter) -> (String, Vec<serde_json::Value>) {
    let field_expr = format!("json_extract(data_json, '$.{}')", filter.field);

    match filter.operator {
        FilterOp::Is => (
            format!("{field_expr} = ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::IsNot => (
            format!("{field_expr} != ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::Contains => (
            format!("{field_expr} LIKE '%' || ? || '%'"),
            vec![filter.value.clone()],
        ),
        FilterOp::DoesNotContain => (
            format!("{field_expr} NOT LIKE '%' || ? || '%'"),
            vec![filter.value.clone()],
        ),
        FilterOp::StartsWith => (
            format!("{field_expr} LIKE ? || '%'"),
            vec![filter.value.clone()],
        ),
        FilterOp::EndsWith => (
            format!("{field_expr} LIKE '%' || ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::GreaterThan => (
            format!("CAST({field_expr} AS REAL) > CAST(? AS REAL)"),
            vec![filter.value.clone()],
        ),
        FilterOp::LessThan => (
            format!("CAST({field_expr} AS REAL) < CAST(? AS REAL)"),
            vec![filter.value.clone()],
        ),
        FilterOp::GreaterThanOrEqual => (
            format!("CAST({field_expr} AS REAL) >= CAST(? AS REAL)"),
            vec![filter.value.clone()],
        ),
        FilterOp::LessThanOrEqual => (
            format!("CAST({field_expr} AS REAL) <= CAST(? AS REAL)"),
            vec![filter.value.clone()],
        ),
        FilterOp::IsEmpty => (
            format!("({field_expr} IS NULL OR {field_expr} = '')"),
            vec![],
        ),
        FilterOp::IsNotEmpty => (
            format!("({field_expr} IS NOT NULL AND {field_expr} != '')"),
            vec![],
        ),
        FilterOp::DateIsBefore => (
            format!("{field_expr} < ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::DateIsAfter => (
            format!("{field_expr} > ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::DateIsOnOrBefore => (
            format!("{field_expr} <= ?"),
            vec![filter.value.clone()],
        ),
        FilterOp::DateIsOnOrAfter => (
            format!("{field_expr} >= ?"),
            vec![filter.value.clone()],
        ),
    }
}

/// Compile a sort into an ORDER BY fragment.
fn compile_sort(sort: &Sort) -> String {
    let field_expr = format!("json_extract(data_json, '$.{}')", sort.field);
    let dir = match sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    format!("{field_expr} {dir}")
}

// ── Query execution ─────────────────────────────────────────────────────────

/// Execute a structured query against the `bases_records` table.
///
/// Compiles filters and sorts to SQL using `json_extract()`, executes with
/// bound parameters, and returns a paginated [`QueryResult`].
///
/// # Errors
///
/// Returns `DatabaseError::QueryError` on SQL compilation or execution failure.
pub fn execute(conn: &Connection, query: &Query) -> Result<QueryResult> {
    // Build WHERE clauses.
    let mut where_parts = vec!["base_id = ?".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(query.base_id)];

    for filter in &query.filters {
        let (clause, filter_params) = compile_filter(filter);
        where_parts.push(clause);
        for p in filter_params {
            params.push(json_to_sql_param(&p));
        }
    }

    let where_clause = where_parts.join(" AND ");

    // Build ORDER BY.
    let order_clause = if query.sorts.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = query.sorts.iter().map(compile_sort).collect();
        format!(" ORDER BY {}", parts.join(", "))
    };

    // Count total matching records.
    let count_sql = format!("SELECT COUNT(*) FROM bases_records WHERE {where_clause}");
    let total_count_i64: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params_from_iter(params.iter().map(AsRef::as_ref)),
            |row| row.get(0),
        )
        .map_err(|e| DatabaseError::QueryError(format!("count query failed: {e}")))?;
    let total_count = total_count_i64.unsigned_abs();

    // Build the data query with LIMIT/OFFSET.
    let mut data_sql =
        format!("SELECT data_json FROM bases_records WHERE {where_clause}{order_clause}");

    if let Some(limit) = query.limit {
        write!(data_sql, " LIMIT {limit}").unwrap();
    }
    if let Some(offset) = query.offset {
        write!(data_sql, " OFFSET {offset}").unwrap();
    }

    // Execute.
    let mut stmt = conn
        .prepare(&data_sql)
        .map_err(|e| DatabaseError::QueryError(format!("prepare failed: {e}")))?;

    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(AsRef::as_ref)),
            |row| {
                let json_str: String = row.get(0)?;
                Ok(json_str)
            },
        )
        .map_err(|e| DatabaseError::QueryError(format!("query failed: {e}")))?;

    let mut records = Vec::new();
    for row in rows {
        let json_str =
            row.map_err(|e| DatabaseError::QueryError(format!("row read failed: {e}")))?;
        let record: nexus_types::bases::BaseRecord =
            serde_json::from_str(&json_str).map_err(|e| {
                DatabaseError::QueryError(format!("failed to deserialize record: {e}"))
            })?;
        records.push(record);
    }

    let has_more = match (query.limit, query.offset) {
        (Some(limit), Some(offset)) => (u64::from(offset) + u64::from(limit)) < total_count,
        (Some(limit), None) => u64::from(limit) < total_count,
        _ => false,
    };

    Ok(QueryResult {
        records,
        total_count,
        has_more,
    })
}

/// Convert a `serde_json::Value` to a boxed `rusqlite::types::ToSql`.
fn json_to_sql_param(value: &serde_json::Value) -> Box<dyn rusqlite::types::ToSql> {
    match value {
        serde_json::Value::Null => Box::new(rusqlite::types::Null),
        serde_json::Value::Bool(b) => Box::new(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else {
                Box::new(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Box::new(s.clone()),
        other => Box::new(other.to_string()),
    }
}

// ── CLI filter/sort parsing ─────────────────────────────────────────────────

/// Parse a CLI filter string like `"status = Done"` into a [`Filter`].
///
/// Supported operators: `=`, `!=`, `contains`, `>`, `<`, `>=`, `<=`,
/// `is_empty`, `is_not_empty`, `starts_with`, `ends_with`,
/// `before`, `after`.
///
/// # Errors
///
/// Returns `DatabaseError::QueryError` if the filter string is malformed.
pub fn parse_filter(input: &str) -> Result<Filter> {
    let input = input.trim();

    // Try two-word operators first.
    for (pattern, op) in &[
        ("is_not_empty", FilterOp::IsNotEmpty),
        ("is_empty", FilterOp::IsEmpty),
        ("starts_with", FilterOp::StartsWith),
        ("ends_with", FilterOp::EndsWith),
        ("does_not_contain", FilterOp::DoesNotContain),
    ] {
        if let Some((field, rest)) = split_operator(input, pattern) {
            let value = parse_value(rest);
            return Ok(Filter {
                field: field.to_string(),
                operator: *op,
                value,
            });
        }
    }

    // Try symbolic operators (longest first to match >= before >).
    for (symbol, op) in &[
        ("!=", FilterOp::IsNot),
        (">=", FilterOp::GreaterThanOrEqual),
        ("<=", FilterOp::LessThanOrEqual),
        (">", FilterOp::GreaterThan),
        ("<", FilterOp::LessThan),
        ("=", FilterOp::Is),
    ] {
        if let Some(pos) = input.find(symbol) {
            let field = input[..pos].trim();
            let value_str = input[pos + symbol.len()..].trim();
            if !field.is_empty() {
                return Ok(Filter {
                    field: field.to_string(),
                    operator: *op,
                    value: parse_value(value_str),
                });
            }
        }
    }

    // Try word operators.
    for (word, op) in &[
        ("contains", FilterOp::Contains),
        ("before", FilterOp::DateIsBefore),
        ("after", FilterOp::DateIsAfter),
    ] {
        if let Some((field, rest)) = split_operator(input, word) {
            let value = parse_value(rest);
            return Ok(Filter {
                field: field.to_string(),
                operator: *op,
                value,
            });
        }
    }

    Err(DatabaseError::QueryError(format!(
        "unrecognized filter expression: '{input}'"
    )))
}

/// Split on a word operator (e.g., "status contains foo" → ("status", "foo")).
fn split_operator<'a>(input: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let pattern = format!(" {operator} ");
    if let Some(pos) = input.find(&pattern) {
        let field = input[..pos].trim();
        let rest = input[pos + pattern.len()..].trim();
        if !field.is_empty() {
            return Some((field, rest));
        }
    }
    // Also try operator at end (for is_empty, is_not_empty).
    let suffix = format!(" {operator}");
    if input.ends_with(&suffix) {
        let field = input[..input.len() - suffix.len()].trim();
        if !field.is_empty() {
            return Some((field, ""));
        }
    }
    None
}

/// Parse a value string, stripping optional quotes and detecting numbers/booleans.
fn parse_value(s: &str) -> serde_json::Value {
    let s = s.trim();
    if s.is_empty() {
        return serde_json::Value::Null;
    }
    // Strip quotes.
    let unquoted = if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    };

    // Try boolean.
    if unquoted.eq_ignore_ascii_case("true") {
        return serde_json::Value::Bool(true);
    }
    if unquoted.eq_ignore_ascii_case("false") {
        return serde_json::Value::Bool(false);
    }
    // Try number.
    if let Ok(n) = unquoted.parse::<f64>() {
        return serde_json::json!(n);
    }
    // Default to string.
    serde_json::Value::String(unquoted.to_string())
}

/// Parse a CLI sort string like `"due_date asc"` into a [`Sort`].
///
/// # Errors
///
/// Returns `DatabaseError::QueryError` if the sort string is malformed.
pub fn parse_sort(input: &str) -> Result<Sort> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    match parts.len() {
        1 => Ok(Sort {
            field: parts[0].to_string(),
            direction: SortDirection::Ascending,
        }),
        2 => {
            let direction = match parts[1].to_lowercase().as_str() {
                "asc" | "ascending" => SortDirection::Ascending,
                "desc" | "descending" => SortDirection::Descending,
                other => {
                    return Err(DatabaseError::QueryError(format!(
                        "unknown sort direction: '{other}' (expected 'asc' or 'desc')"
                    )));
                }
            };
            Ok(Sort {
                field: parts[0].to_string(),
                direction,
            })
        }
        _ => Err(DatabaseError::QueryError(format!(
            "invalid sort expression: '{input}' (expected 'field [asc|desc]')"
        ))),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_filter tests ──────────────────────────────────────────────────

    #[test]
    fn parse_filter_equals() {
        let f = parse_filter("status = Done").unwrap();
        assert_eq!(f.field, "status");
        assert_eq!(f.operator, FilterOp::Is);
        assert_eq!(f.value, serde_json::json!("Done"));
    }

    #[test]
    fn parse_filter_not_equals() {
        let f = parse_filter("status != todo").unwrap();
        assert_eq!(f.operator, FilterOp::IsNot);
    }

    #[test]
    fn parse_filter_greater_than() {
        let f = parse_filter("priority > 3").unwrap();
        assert_eq!(f.field, "priority");
        assert_eq!(f.operator, FilterOp::GreaterThan);
        assert_eq!(f.value, serde_json::json!(3.0));
    }

    #[test]
    fn parse_filter_less_than_or_equal() {
        let f = parse_filter("score <= 100").unwrap();
        assert_eq!(f.operator, FilterOp::LessThanOrEqual);
    }

    #[test]
    fn parse_filter_contains() {
        let f = parse_filter("title contains bug").unwrap();
        assert_eq!(f.field, "title");
        assert_eq!(f.operator, FilterOp::Contains);
        assert_eq!(f.value, serde_json::json!("bug"));
    }

    #[test]
    fn parse_filter_is_empty() {
        let f = parse_filter("notes is_empty").unwrap();
        assert_eq!(f.field, "notes");
        assert_eq!(f.operator, FilterOp::IsEmpty);
    }

    #[test]
    fn parse_filter_is_not_empty() {
        let f = parse_filter("assignee is_not_empty").unwrap();
        assert_eq!(f.field, "assignee");
        assert_eq!(f.operator, FilterOp::IsNotEmpty);
    }

    #[test]
    fn parse_filter_before() {
        let f = parse_filter("due_date before 2026-04-15").unwrap();
        assert_eq!(f.operator, FilterOp::DateIsBefore);
    }

    #[test]
    fn parse_filter_quoted_value() {
        let f = parse_filter("status = \"In Progress\"").unwrap();
        assert_eq!(f.value, serde_json::json!("In Progress"));
    }

    #[test]
    fn parse_filter_invalid() {
        assert!(parse_filter("").is_err());
    }

    // ── parse_sort tests ────────────────────────────────────────────────────

    #[test]
    fn parse_sort_asc() {
        let s = parse_sort("due_date asc").unwrap();
        assert_eq!(s.field, "due_date");
        assert_eq!(s.direction, SortDirection::Ascending);
    }

    #[test]
    fn parse_sort_desc() {
        let s = parse_sort("priority desc").unwrap();
        assert_eq!(s.direction, SortDirection::Descending);
    }

    #[test]
    fn parse_sort_default_asc() {
        let s = parse_sort("name").unwrap();
        assert_eq!(s.direction, SortDirection::Ascending);
    }

    #[test]
    fn parse_sort_invalid() {
        assert!(parse_sort("a b c").is_err());
    }

    // ── SQL execution tests ─────────────────────────────────────────────────

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        // Insert a test base.
        conn.execute(
            "INSERT INTO bases (path, name, schema_json, created_at, modified_at)
             VALUES ('test.bases', 'Test', '{}', 0, 0)",
            [],
        )
        .unwrap();

        let base_id = conn.last_insert_rowid();

        // Insert test records.
        let records = vec![
            (
                "r1",
                serde_json::json!({"id": "r1", "title": "Buy milk", "status": "todo", "priority": 1}),
            ),
            (
                "r2",
                serde_json::json!({"id": "r2", "title": "Write tests", "status": "done", "priority": 3}),
            ),
            (
                "r3",
                serde_json::json!({"id": "r3", "title": "Fix bug", "status": "todo", "priority": 5}),
            ),
            (
                "r4",
                serde_json::json!({"id": "r4", "title": "Deploy", "status": "done", "priority": 2}),
            ),
        ];

        for (id, data) in &records {
            conn.execute(
                "INSERT INTO bases_records (base_id, record_id, data_json, created_at, modified_at)
                 VALUES (?1, ?2, ?3, 0, 0)",
                rusqlite::params![base_id, id, data.to_string()],
            )
            .unwrap();
        }

        conn
    }

    #[test]
    fn query_all_records() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 4);
        assert_eq!(result.records.len(), 4);
        assert!(!result.has_more);
    }

    #[test]
    fn query_with_equality_filter() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            filters: vec![Filter {
                field: "status".to_string(),
                operator: FilterOp::Is,
                value: serde_json::json!("todo"),
            }],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 2);
    }

    #[test]
    fn query_with_numeric_filter() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            filters: vec![Filter {
                field: "priority".to_string(),
                operator: FilterOp::GreaterThan,
                value: serde_json::json!(2),
            }],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 2); // priority 3 and 5
    }

    #[test]
    fn query_with_contains_filter() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            filters: vec![Filter {
                field: "title".to_string(),
                operator: FilterOp::Contains,
                value: serde_json::json!("bug"),
            }],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 1);
        assert_eq!(result.records[0].id, "r3");
    }

    #[test]
    fn query_with_sort() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            sorts: vec![Sort {
                field: "priority".to_string(),
                direction: SortDirection::Descending,
            }],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.records[0].id, "r3"); // priority 5
    }

    #[test]
    fn query_with_limit_and_offset() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 4);
        assert_eq!(result.records.len(), 2);
        assert!(result.has_more);
    }

    #[test]
    fn query_with_limit_no_more() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            limit: Some(10),
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.records.len(), 4);
        assert!(!result.has_more);
    }

    #[test]
    fn query_multiple_filters() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            filters: vec![
                Filter {
                    field: "status".to_string(),
                    operator: FilterOp::Is,
                    value: serde_json::json!("todo"),
                },
                Filter {
                    field: "priority".to_string(),
                    operator: FilterOp::GreaterThan,
                    value: serde_json::json!(2),
                },
            ],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 1); // only "Fix bug" (todo, priority 5)
        assert_eq!(result.records[0].id, "r3");
    }

    #[test]
    fn query_empty_result() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 1,
            filters: vec![Filter {
                field: "status".to_string(),
                operator: FilterOp::Is,
                value: serde_json::json!("nonexistent"),
            }],
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 0);
        assert!(result.records.is_empty());
    }

    #[test]
    fn query_nonexistent_base() {
        let conn = setup_test_db();
        let query = Query {
            base_id: 999,
            ..Default::default()
        };
        let result = execute(&conn, &query).unwrap();
        assert_eq!(result.total_count, 0);
    }
}
