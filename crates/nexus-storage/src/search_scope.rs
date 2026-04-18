//! Scoped search query parsing and post-filtering.
//!
//! Extracts `tag:`, `path:`, `prop:`, and `type:` prefixes from search
//! queries. `tag:` / `path:` / `prop:` post-filter Tantivy hits via
//! `SQLite` lookups; `type:` filters directly on the `block_type`
//! stored on each search result.
//!
//! The `prop:` scope supports both the legacy string-substring form
//! (`prop:KEY:VALUE`) and typed comparison operators that dispatch to
//! the typed columns (`value_num`, `value_date`, `value_bool`)
//! populated by migration 003:
//!
//! - `prop:priority>3`, `prop:priority<=5`  → numeric comparison
//! - `prop:due<2026-01-01`                  → date comparison (YYYY-MM-DD)
//! - `prop:draft=true`                      → boolean equality
//! - `prop:status:done`                     → string substring (legacy)

use rusqlite::Connection;

use crate::search::SearchResult;
use crate::StorageError;

/// Comparison operator for numeric / date typed property filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// `>` — greater than.
    Gt,
    /// `<` — less than.
    Lt,
    /// `>=` — greater than or equal.
    Gte,
    /// `<=` — less than or equal.
    Lte,
    /// `=` — equal.
    Eq,
}

impl CmpOp {
    fn sql(self) -> &'static str {
        match self {
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Gte => ">=",
            Self::Lte => "<=",
            Self::Eq => "=",
        }
    }
}

/// The typed operation portion of a `prop:` filter.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyOp {
    /// Legacy substring match against the raw JSON `value` column
    /// (the `prop:KEY:VALUE` form).
    Contains(String),
    /// Numeric comparison against `value_num`.
    NumCmp(CmpOp, f64),
    /// Date comparison against `value_date` (unix seconds, UTC midnight).
    DateCmp(CmpOp, i64),
    /// Boolean equality against `value_bool`.
    BoolEq(bool),
}

/// A scope filter extracted from a search query.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeFilter {
    /// Filter to files with a specific tag.
    Tag(String),
    /// Filter to files whose path starts with a prefix.
    Path(String),
    /// Filter to files with a property matching the typed operator.
    Property {
        /// The property key.
        key: String,
        /// The typed operation against that key.
        op: PropertyOp,
    },
    /// Filter to search hits whose block has the given block_type
    /// (`heading`, `paragraph`, `list_item`, etc.). Matched directly
    /// against the result's `block_type` — no DB lookup needed.
    Type(String),
}

/// Parse a query string, extracting scope prefixes and returning the
/// remaining plain-text query.
///
/// Supported prefixes:
/// - `tag:NAME` → [`ScopeFilter::Tag`]
/// - `path:PREFIX` → [`ScopeFilter::Path`]
/// - `prop:KEY{:|=|>|<|>=|<=}VALUE` → [`ScopeFilter::Property`]
/// - `type:BLOCK_TYPE` → [`ScopeFilter::Type`]
///
/// Tokens that don't match a prefix (or malformed `prop:` without a value)
/// are kept as plain-text query terms.
#[must_use]
pub fn parse_scoped_query(input: &str) -> (String, Vec<ScopeFilter>) {
    let mut filters = Vec::new();
    let mut text_parts = Vec::new();

    for token in input.split_whitespace() {
        if let Some(tag) = token.strip_prefix("tag:") {
            if !tag.is_empty() {
                filters.push(ScopeFilter::Tag(tag.to_string()));
                continue;
            }
        }
        if let Some(path) = token.strip_prefix("path:") {
            if !path.is_empty() {
                filters.push(ScopeFilter::Path(path.to_string()));
                continue;
            }
        }
        if let Some(rest) = token.strip_prefix("prop:") {
            if let Some((key, op)) = parse_prop_body(rest) {
                filters.push(ScopeFilter::Property { key, op });
                continue;
            }
            // Malformed prop: — treat as plain text
        }
        if let Some(ty) = token.strip_prefix("type:") {
            if !ty.is_empty() {
                filters.push(ScopeFilter::Type(ty.to_string()));
                continue;
            }
        }
        text_parts.push(token);
    }

    (text_parts.join(" "), filters)
}

/// Parse the body of a `prop:` token (the part after the `prop:` prefix).
///
/// Returns `Some((key, op))` if the token is well-formed, or `None` if
/// it's malformed (missing key, missing value, unparseable typed value).
fn parse_prop_body(rest: &str) -> Option<(String, PropertyOp)> {
    let bytes = rest.as_bytes();
    // Find the first operator character. Key is required to be non-empty,
    // so we start at index 1.
    let mut op_idx = None;
    for (i, &b) in bytes.iter().enumerate() {
        if matches!(b, b':' | b'>' | b'<' | b'=') {
            op_idx = Some(i);
            break;
        }
    }
    let i = op_idx?;
    if i == 0 {
        return None; // empty key
    }
    let key = &rest[..i];

    let (op_len, op_str) = match bytes[i] {
        b':' => (1, ":"),
        b'=' => (1, "="),
        b'>' => {
            if bytes.get(i + 1) == Some(&b'=') {
                (2, ">=")
            } else {
                (1, ">")
            }
        }
        b'<' => {
            if bytes.get(i + 1) == Some(&b'=') {
                (2, "<=")
            } else {
                (1, "<")
            }
        }
        _ => unreachable!(),
    };

    let value = &rest[i + op_len..];
    if value.is_empty() {
        return None;
    }

    let op = match op_str {
        ":" => PropertyOp::Contains(value.to_string()),
        "=" => parse_eq_value(value)?,
        ">" => parse_cmp_value(CmpOp::Gt, value)?,
        "<" => parse_cmp_value(CmpOp::Lt, value)?,
        ">=" => parse_cmp_value(CmpOp::Gte, value)?,
        "<=" => parse_cmp_value(CmpOp::Lte, value)?,
        _ => unreachable!(),
    };

    Some((key.to_string(), op))
}

fn parse_eq_value(value: &str) -> Option<PropertyOp> {
    match value.to_ascii_lowercase().as_str() {
        "true" => return Some(PropertyOp::BoolEq(true)),
        "false" => return Some(PropertyOp::BoolEq(false)),
        _ => {}
    }
    if let Some(ts) = parse_date(value) {
        return Some(PropertyOp::DateCmp(CmpOp::Eq, ts));
    }
    if let Ok(n) = value.parse::<f64>() {
        return Some(PropertyOp::NumCmp(CmpOp::Eq, n));
    }
    None
}

fn parse_cmp_value(cmp: CmpOp, value: &str) -> Option<PropertyOp> {
    if let Some(ts) = parse_date(value) {
        return Some(PropertyOp::DateCmp(cmp, ts));
    }
    if let Ok(n) = value.parse::<f64>() {
        return Some(PropertyOp::NumCmp(cmp, n));
    }
    None
}

/// Parse a `YYYY-MM-DD` string to a Unix timestamp (midnight UTC).
fn parse_date(s: &str) -> Option<i64> {
    if s.len() != 10 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// Filter search results using scope filters and `SQLite` lookups.
///
/// Results must pass ALL filters (AND logic). Original scores are preserved.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
#[allow(clippy::unnecessary_wraps)]
pub fn filter_results(
    conn: &Connection,
    results: Vec<SearchResult>,
    filters: &[ScopeFilter],
) -> Result<Vec<SearchResult>, StorageError> {
    if filters.is_empty() {
        return Ok(results);
    }

    let mut filtered = Vec::new();
    for result in results {
        if passes_all_filters(conn, &result, filters) {
            filtered.push(result);
        }
    }
    Ok(filtered)
}

/// Check if a search result passes all scope filters.
fn passes_all_filters(
    conn: &Connection,
    result: &SearchResult,
    filters: &[ScopeFilter],
) -> bool {
    let file_path = result.file_path.as_str();
    for filter in filters {
        match filter {
            ScopeFilter::Type(block_type) => {
                if result.block_type != *block_type {
                    return false;
                }
            }
            ScopeFilter::Tag(name) => {
                let found: bool = conn
                    .query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM tags t JOIN files f ON f.id = t.file_id
                            WHERE f.path = ?1 AND t.name = ?2 AND f.is_deleted = 0
                        )",
                        rusqlite::params![file_path, name],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if !found {
                    return false;
                }
            }
            ScopeFilter::Path(prefix) => {
                if !file_path.starts_with(prefix.as_str()) {
                    return false;
                }
            }
            ScopeFilter::Property { key, op } => {
                if !property_matches(conn, file_path, key, op) {
                    return false;
                }
            }
        }
    }
    true
}

fn property_matches(conn: &Connection, file_path: &str, key: &str, op: &PropertyOp) -> bool {
    match op {
        PropertyOp::Contains(value) => {
            let pattern = format!("%{value}%");
            conn.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id
                    WHERE f.path = ?1 AND p.key = ?2 AND p.value LIKE ?3
                    AND f.is_deleted = 0
                )",
                rusqlite::params![file_path, key, pattern],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
        }
        PropertyOp::NumCmp(cmp, n) => {
            let sql = format!(
                "SELECT EXISTS(
                    SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id
                    WHERE f.path = ?1 AND p.key = ?2
                      AND p.value_num IS NOT NULL AND p.value_num {} ?3
                      AND f.is_deleted = 0
                )",
                cmp.sql()
            );
            conn.query_row(
                &sql,
                rusqlite::params![file_path, key, n],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
        }
        PropertyOp::DateCmp(cmp, ts) => {
            let sql = format!(
                "SELECT EXISTS(
                    SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id
                    WHERE f.path = ?1 AND p.key = ?2
                      AND p.value_date IS NOT NULL AND p.value_date {} ?3
                      AND f.is_deleted = 0
                )",
                cmp.sql()
            );
            conn.query_row(
                &sql,
                rusqlite::params![file_path, key, ts],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
        }
        PropertyOp::BoolEq(b) => conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id
                    WHERE f.path = ?1 AND p.key = ?2
                      AND p.value_bool IS NOT NULL AND p.value_bool = ?3
                      AND f.is_deleted = 0
                )",
                rusqlite::params![file_path, key, b],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_scoped_query tests ─────────────────────────────────────────

    #[test]
    fn parse_tag_scope() {
        let (text, filters) = parse_scoped_query("tag:rust programming");
        assert_eq!(text, "programming");
        assert_eq!(filters, vec![ScopeFilter::Tag("rust".to_string())]);
    }

    #[test]
    fn parse_path_scope() {
        let (text, filters) = parse_scoped_query("path:notes/ hello world");
        assert_eq!(text, "hello world");
        assert_eq!(filters, vec![ScopeFilter::Path("notes/".to_string())]);
    }

    #[test]
    fn parse_prop_scope_contains() {
        let (text, filters) = parse_scoped_query("prop:status:done tasks");
        assert_eq!(text, "tasks");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "status".to_string(),
                op: PropertyOp::Contains("done".to_string()),
            }]
        );
    }

    #[test]
    fn parse_prop_scope_num_gt() {
        let (text, filters) = parse_scoped_query("prop:priority>3");
        assert_eq!(text, "");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "priority".to_string(),
                op: PropertyOp::NumCmp(CmpOp::Gt, 3.0),
            }]
        );
    }

    #[test]
    fn parse_prop_scope_num_lte() {
        let (_text, filters) = parse_scoped_query("prop:score<=7.5");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "score".to_string(),
                op: PropertyOp::NumCmp(CmpOp::Lte, 7.5),
            }]
        );
    }

    #[test]
    fn parse_prop_scope_date_lt() {
        let (_text, filters) = parse_scoped_query("prop:due<2026-01-01");
        let expected_ts = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "due".to_string(),
                op: PropertyOp::DateCmp(CmpOp::Lt, expected_ts),
            }]
        );
    }

    #[test]
    fn parse_prop_scope_bool_eq() {
        let (_text, filters) = parse_scoped_query("prop:draft=true");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "draft".to_string(),
                op: PropertyOp::BoolEq(true),
            }]
        );
        let (_text, filters) = parse_scoped_query("prop:draft=FALSE");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "draft".to_string(),
                op: PropertyOp::BoolEq(false),
            }]
        );
    }

    #[test]
    fn parse_prop_scope_eq_numeric() {
        let (_text, filters) = parse_scoped_query("prop:priority=3");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "priority".to_string(),
                op: PropertyOp::NumCmp(CmpOp::Eq, 3.0),
            }]
        );
    }

    #[test]
    fn parse_multiple_scopes() {
        let (text, filters) = parse_scoped_query("tag:rust path:notes/ async programming");
        assert_eq!(text, "async programming");
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0], ScopeFilter::Tag("rust".to_string()));
        assert_eq!(filters[1], ScopeFilter::Path("notes/".to_string()));
    }

    #[test]
    fn parse_no_scopes() {
        let (text, filters) = parse_scoped_query("no scopes here");
        assert_eq!(text, "no scopes here");
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_scope_only_no_text() {
        let (text, filters) = parse_scoped_query("tag:rust");
        assert_eq!(text, "");
        assert_eq!(filters, vec![ScopeFilter::Tag("rust".to_string())]);
    }

    #[test]
    fn parse_malformed_prop_treated_as_text() {
        let (text, filters) = parse_scoped_query("prop:malformed stuff");
        assert_eq!(text, "prop:malformed stuff");
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_prop_unparseable_numeric_treated_as_text() {
        // `>` operator demands numeric or date; "notanumber" is neither.
        let (text, filters) = parse_scoped_query("prop:priority>notanumber");
        assert_eq!(text, "prop:priority>notanumber");
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_empty_prefix_value_treated_as_text() {
        let (text, filters) = parse_scoped_query("tag: hello");
        assert_eq!(text, "tag: hello");
        assert!(filters.is_empty());
    }

    // ── filter_results tests ─────────────────────────────────────────────

    #[test]
    fn filter_results_with_tag() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/a.md', 'markdown', 'h1', 10, 0, 0);",
            [],
        ).unwrap();
        let fid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO tags (name, file_id, source) VALUES ('rust', ?1, 'inline');",
            rusqlite::params![fid],
        ).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/b.md', 'markdown', 'h2', 10, 0, 0);",
            [],
        ).unwrap();

        let results = vec![
            SearchResult { file_path: "notes/a.md".to_string(), block_id: 1, block_type: "paragraph".to_string(), excerpt: String::new(), score: 1.0 },
            SearchResult { file_path: "notes/b.md".to_string(), block_id: 2, block_type: "paragraph".to_string(), excerpt: String::new(), score: 0.5 },
        ];

        let filtered = filter_results(&conn, results, &[ScopeFilter::Tag("rust".to_string())]).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].file_path, "notes/a.md");
    }

    #[test]
    fn filter_results_with_path() {
        let results = vec![
            SearchResult { file_path: "notes/a.md".to_string(), block_id: 1, block_type: "paragraph".to_string(), excerpt: String::new(), score: 1.0 },
            SearchResult { file_path: "docs/b.md".to_string(), block_id: 2, block_type: "paragraph".to_string(), excerpt: String::new(), score: 0.5 },
        ];

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let filtered = filter_results(&conn, results, &[ScopeFilter::Path("notes/".to_string())]).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].file_path, "notes/a.md");
    }

    #[test]
    fn filter_results_no_filters_returns_all() {
        let results = vec![
            SearchResult { file_path: "notes/a.md".to_string(), block_id: 1, block_type: "paragraph".to_string(), excerpt: String::new(), score: 1.0 },
        ];

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let filtered = filter_results(&conn, results, &[]).unwrap();
        assert_eq!(filtered.len(), 1);
    }

    // ── type: scope ──────────────────────────────────────────────────────

    #[test]
    fn parse_type_scope() {
        let (text, filters) = parse_scoped_query("type:heading intro");
        assert_eq!(text, "intro");
        assert_eq!(filters, vec![ScopeFilter::Type("heading".to_string())]);
    }

    #[test]
    fn parse_empty_type_prefix_treated_as_text() {
        let (text, filters) = parse_scoped_query("type: hello");
        assert_eq!(text, "type: hello");
        assert!(filters.is_empty());
    }

    #[test]
    fn filter_results_with_type() {
        let results = vec![
            SearchResult { file_path: "notes/a.md".to_string(), block_id: 1, block_type: "heading".to_string(), excerpt: String::new(), score: 1.0 },
            SearchResult { file_path: "notes/b.md".to_string(), block_id: 2, block_type: "paragraph".to_string(), excerpt: String::new(), score: 0.5 },
        ];

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let filtered = filter_results(&conn, results, &[ScopeFilter::Type("heading".to_string())]).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].block_type, "heading");
    }

    // ── typed prop: filter_results tests ─────────────────────────────────

    /// Build an in-memory DB with a single file whose properties include
    /// numeric, date, and boolean values. Returns (conn, file_path).
    fn setup_typed_props_db() -> (Connection, String) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/typed.md', 'markdown', 'h', 10, 0, 0);",
            [],
        ).unwrap();
        let fid = conn.last_insert_rowid();

        // priority = 5 (number)
        conn.execute(
            "INSERT INTO properties (file_id, key, value, property_type, value_num, value_date, value_bool)
             VALUES (?1, 'priority', '5', 'number', 5.0, NULL, NULL);",
            rusqlite::params![fid],
        ).unwrap();

        // due = 2026-03-01 (date) — stored as unix seconds
        let due_ts = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        conn.execute(
            "INSERT INTO properties (file_id, key, value, property_type, value_num, value_date, value_bool)
             VALUES (?1, 'due', '\"2026-03-01\"', 'string', NULL, ?2, NULL);",
            rusqlite::params![fid, due_ts],
        ).unwrap();

        // draft = true (bool)
        conn.execute(
            "INSERT INTO properties (file_id, key, value, property_type, value_num, value_date, value_bool)
             VALUES (?1, 'draft', 'true', 'boolean', NULL, NULL, 1);",
            rusqlite::params![fid],
        ).unwrap();

        (conn, "notes/typed.md".to_string())
    }

    fn hit(path: &str) -> SearchResult {
        SearchResult {
            file_path: path.to_string(),
            block_id: 1,
            block_type: "paragraph".to_string(),
            excerpt: String::new(),
            score: 1.0,
        }
    }

    #[test]
    fn filter_results_prop_num_gt_matches() {
        let (conn, path) = setup_typed_props_db();
        let filters = vec![ScopeFilter::Property {
            key: "priority".to_string(),
            op: PropertyOp::NumCmp(CmpOp::Gt, 3.0),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn filter_results_prop_num_gt_excludes() {
        let (conn, path) = setup_typed_props_db();
        let filters = vec![ScopeFilter::Property {
            key: "priority".to_string(),
            op: PropertyOp::NumCmp(CmpOp::Gt, 10.0),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn filter_results_prop_date_lt_matches() {
        let (conn, path) = setup_typed_props_db();
        let cutoff = chrono::NaiveDate::from_ymd_opt(2026, 6, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let filters = vec![ScopeFilter::Property {
            key: "due".to_string(),
            op: PropertyOp::DateCmp(CmpOp::Lt, cutoff),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn filter_results_prop_bool_eq_matches() {
        let (conn, path) = setup_typed_props_db();
        let filters = vec![ScopeFilter::Property {
            key: "draft".to_string(),
            op: PropertyOp::BoolEq(true),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert_eq!(out.len(), 1);

        let filters = vec![ScopeFilter::Property {
            key: "draft".to_string(),
            op: PropertyOp::BoolEq(false),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn filter_results_prop_contains_legacy() {
        let (conn, path) = setup_typed_props_db();
        // The raw JSON value for `due` is `"2026-03-01"` (with quotes),
        // so a substring match on `2026` should find it.
        let filters = vec![ScopeFilter::Property {
            key: "due".to_string(),
            op: PropertyOp::Contains("2026".to_string()),
        }];
        let out = filter_results(&conn, vec![hit(&path)], &filters).unwrap();
        assert_eq!(out.len(), 1);
    }
}
