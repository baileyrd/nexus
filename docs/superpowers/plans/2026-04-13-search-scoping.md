# Search Scoping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `tag:`, `path:`, and `prop:` scope operators to the search query syntax, filtering Tantivy results via SQLite post-filtering.

**Architecture:** A new `search_scope.rs` module parses scope prefixes from the query string and provides a post-filter function. `StorageEngine::search` is updated to parse scopes, run Tantivy on the remaining text, then post-filter with SQLite.

**Tech Stack:** rusqlite (existing), Tantivy (existing). No new dependencies.

---

## File Structure

| File | Role | Change |
|------|------|--------|
| `crates/nexus-storage/src/search_scope.rs` | Query parsing + post-filtering | **NEW** |
| `crates/nexus-storage/src/lib.rs` | Module registration + StorageEngine::search update | Modify |
| `crates/nexus-storage/tests/prd-06-smoke.rs` | Integration tests | Modify (add scoped search tests) |

---

### Task 1: Query Parser and ScopeFilter Type

**Files:**
- Create: `crates/nexus-storage/src/search_scope.rs`
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Create search_scope.rs with types and tests**

Create `crates/nexus-storage/src/search_scope.rs`:

```rust
//! Scoped search query parsing and post-filtering.
//!
//! Extracts `tag:`, `path:`, and `prop:` prefixes from search queries
//! and filters Tantivy results using SQLite lookups.

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

/// Filter search results using scope filters and SQLite lookups.
///
/// Results must pass ALL filters (AND logic). Original scores are preserved.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any SQLite failure.
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
        if passes_all_filters(conn, &result.file_path, filters)? {
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
) -> Result<bool, StorageError> {
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
                    return Ok(false);
                }
            }
            ScopeFilter::Path(prefix) => {
                if !file_path.starts_with(prefix.as_str()) {
                    return Ok(false);
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
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn filter_results_with_tag() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        // Insert a file with tag "rust"
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

        // Insert a file without the tag
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
```

- [ ] **Step 2: Register module in lib.rs**

In `crates/nexus-storage/src/lib.rs`, add after `mod graph;`:

```rust
mod search_scope;
```

Add to pub use exports:

```rust
pub use search_scope::{ScopeFilter, parse_scoped_query};
```

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo test -p nexus-storage --lib search_scope`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/search_scope.rs crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): add scoped search query parser with tag:, path:, prop: operators"
```

---

### Task 2: Wire Scoping into StorageEngine::search

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Update StorageEngine::search**

Replace the current `search` method (around line 509):

```rust
/// Search the Tantivy index for `query`, returning up to `limit` results.
///
/// Supports scope operators: `tag:NAME`, `path:PREFIX`, `prop:KEY:VALUE`.
/// Scopes are extracted from the query, Tantivy searches the remaining
/// text, and results are post-filtered via SQLite.
///
/// # Errors
///
/// Returns [`StorageError`] on Tantivy or database failure.
pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
    let (text, filters) = search_scope::parse_scoped_query(query);

    // Run Tantivy on the plain-text portion.
    let results = if text.is_empty() {
        // Scope-only query: return all blocks up to limit (unscored).
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        let mut stmt = conn.prepare(
            "SELECT f.path, b.id, b.block_type, b.content
             FROM blocks b JOIN files f ON f.id = b.file_id
             WHERE f.is_deleted = 0
             ORDER BY f.path, b.start_line
             LIMIT ?1;"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(SearchResult {
                file_path: row.get(0)?,
                block_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                block_type: row.get(2)?,
                excerpt: String::new(),
                score: 0.0,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        self.search_index.search(&text, limit)?
    };

    // Post-filter with scopes if any.
    if filters.is_empty() {
        Ok(results)
    } else {
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        search_scope::filter_results(&conn, results, &filters)
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p nexus-storage`
Expected: PASS

- [ ] **Step 3: Run all existing tests**

Run: `cargo test -p nexus-storage`
Expected: All PASS (existing search tests should still work since queries without scope prefixes behave identically)

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): wire scoped search into StorageEngine with post-filtering"
```

---

### Task 3: Integration Tests

**Files:**
- Modify: `crates/nexus-storage/tests/prd-06-smoke.rs`

- [ ] **Step 1: Add scoped search integration tests**

Add to the end of `crates/nexus-storage/tests/prd-06-smoke.rs`:

```rust
#[test]
fn search_with_tag_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/rust.md", b"---\ntags:\n  - rust\n---\n# Rust Guide\n\nAsync programming in Rust.\n")
        .unwrap();
    engine
        .write_file("notes/python.md", b"---\ntags:\n  - python\n---\n# Python Guide\n\nAsync programming in Python.\n")
        .unwrap();

    // Rebuild search index
    engine.rebuild_search_index().unwrap();

    // Unscoped: both match "programming"
    let all = engine.search("programming", 10).unwrap();
    assert!(all.len() >= 2, "expected at least 2 results, got {}", all.len());

    // Scoped by tag: only rust matches
    let scoped = engine.search("tag:rust programming", 10).unwrap();
    assert_eq!(scoped.len(), 1, "expected 1 result for tag:rust, got {}", scoped.len());
    assert_eq!(scoped[0].file_path, "notes/rust.md");
}

#[test]
fn search_with_path_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"# Notes\n\nImportant content here.\n")
        .unwrap();
    engine
        .write_file("docs/b.md", b"# Docs\n\nImportant content here.\n")
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("path:notes/ important", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/a.md");
}

#[test]
fn search_with_prop_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file(
            "notes/done.md",
            b"---\nstatus: done\n---\n# Done Task\n\nCompleted work here.\n",
        )
        .unwrap();
    engine
        .write_file(
            "notes/wip.md",
            b"---\nstatus: wip\n---\n# WIP Task\n\nCompleted work here.\n",
        )
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("prop:status:done work", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/done.md");
}

#[test]
fn search_with_combined_scopes() {
    let (_dir, engine) = engine();

    engine
        .write_file(
            "notes/match.md",
            b"---\ntags:\n  - rust\n---\n# Match\n\nAsync programming patterns.\n",
        )
        .unwrap();
    engine
        .write_file(
            "docs/nomatch.md",
            b"---\ntags:\n  - rust\n---\n# No Match\n\nAsync programming patterns.\n",
        )
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("tag:rust path:notes/ programming", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/match.md");
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p nexus-storage --test prd-06-smoke`
Expected: All PASS (existing + 4 new)

- [ ] **Step 3: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All PASS (except known flaky credential test)

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/tests/prd-06-smoke.rs
git commit -m "test(storage): add scoped search integration tests — tag:, path:, prop: operators"
```
