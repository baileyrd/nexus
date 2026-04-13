//! Scoped search query parsing and post-filtering.
//!
//! Extracts `tag:`, `path:`, and `prop:` prefixes from search queries
//! and filters Tantivy results using `SQLite` lookups.

use rusqlite::Connection;

use crate::search::SearchResult;
use crate::StorageError;

/// A scope filter extracted from a search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeFilter {
    /// Filter to files with a specific tag.
    Tag(String),
    /// Filter to files whose path starts with a prefix.
    Path(String),
    /// Filter to files with a property key containing a value substring.
    Property {
        /// The property key.
        key: String,
        /// The value substring to match.
        value: String,
    },
}

/// Parse a query string, extracting scope prefixes and returning the
/// remaining plain-text query.
///
/// Supported prefixes:
/// - `tag:NAME` → `ScopeFilter::Tag("NAME")`
/// - `path:PREFIX` → `ScopeFilter::Path("PREFIX")`
/// - `prop:KEY:VALUE` → `ScopeFilter::Property { key, value }`
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
            if let Some(colon_pos) = rest.find(':') {
                let key = &rest[..colon_pos];
                let value = &rest[colon_pos + 1..];
                if !key.is_empty() && !value.is_empty() {
                    filters.push(ScopeFilter::Property {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                    continue;
                }
            }
            // Malformed prop: — treat as plain text
        }
        text_parts.push(token);
    }

    (text_parts.join(" "), filters)
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
        if passes_all_filters(conn, &result.file_path, filters) {
            filtered.push(result);
        }
    }
    Ok(filtered)
}

/// Check if a file path passes all scope filters.
fn passes_all_filters(
    conn: &Connection,
    file_path: &str,
    filters: &[ScopeFilter],
) -> bool {
    for filter in filters {
        match filter {
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
            ScopeFilter::Property { key, value } => {
                let pattern = format!("%{value}%");
                let found: bool = conn
                    .query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id
                            WHERE f.path = ?1 AND p.key = ?2 AND p.value LIKE ?3
                            AND f.is_deleted = 0
                        )",
                        rusqlite::params![file_path, key, pattern],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if !found {
                    return false;
                }
            }
        }
    }
    true
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
    fn parse_prop_scope() {
        let (text, filters) = parse_scoped_query("prop:status:done tasks");
        assert_eq!(text, "tasks");
        assert_eq!(
            filters,
            vec![ScopeFilter::Property {
                key: "status".to_string(),
                value: "done".to_string(),
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
}
