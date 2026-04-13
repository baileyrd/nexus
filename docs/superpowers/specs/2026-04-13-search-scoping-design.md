# Search Scoping Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** Add tag:, path:, prop: query operators to search
**Source:** Backlog BL-003, Growth Plan Phase 2

---

## Overview

Extend the existing Tantivy-based search with scope prefix operators. Queries like `tag:rust path:notes/ async programming` first search Tantivy for "async programming", then post-filter results using SQLite to only include files tagged "rust" under "notes/".

The `SearchIndex` module is unchanged. Scoping is handled at the `StorageEngine` level where both Tantivy and SQLite are available.

---

## 1. Query Parsing

New function `parse_scoped_query(input: &str) -> (String, Vec<ScopeFilter>)` in a new file `crates/nexus-storage/src/search_scope.rs`.

Splits the input on whitespace tokens. Tokens matching `tag:`, `path:`, or `prop:` prefixes are extracted as filters. Remaining tokens are joined back as the plain-text query.

```rust
pub enum ScopeFilter {
    Tag(String),           // tag:rust
    Path(String),          // path:notes/
    Property {             // prop:status:done
        key: String,
        value: String,
    },
}
```

Parsing rules:
- `tag:X` → `ScopeFilter::Tag("X")`
- `path:X` → `ScopeFilter::Path("X")`
- `prop:KEY:VALUE` → `ScopeFilter::Property { key: "KEY", value: "VALUE" }`
- `prop:KEY` without `:VALUE` → ignored (malformed, treated as plain text)
- Multiple scopes are ANDed
- If no plain text remains after extracting scopes, return empty string (no Tantivy search — filter only)

## 2. Post-Filtering

New function `filter_results(conn: &Connection, results: Vec<SearchResult>, filters: &[ScopeFilter]) -> Result<Vec<SearchResult>>` in the same file.

For each filter type:
- **Tag**: query `SELECT 1 FROM tags t JOIN files f ON f.id = t.file_id WHERE f.path = ?1 AND t.name = ?2`
- **Path**: `result.file_path.starts_with(prefix)` — no SQL needed
- **Property**: query `SELECT 1 FROM properties p JOIN files f ON f.id = p.file_id WHERE f.path = ?1 AND p.key = ?2 AND p.value LIKE ?3` with `%value%` pattern for substring match

Results must pass ALL filters (AND logic). Original Tantivy scores are preserved.

## 3. StorageEngine Integration

The existing `StorageEngine::search` method is updated:
1. Call `parse_scoped_query(query)` to split scopes from text
2. If plain text is non-empty, run through Tantivy as before
3. If plain text is empty but scopes exist, return all indexed blocks (unscored) up to `limit`
4. Post-filter results through `filter_results` using a pool connection
5. Return filtered results

## 4. Files

| File | Change |
|------|--------|
| `crates/nexus-storage/src/search_scope.rs` | **NEW** — parse_scoped_query, ScopeFilter, filter_results |
| `crates/nexus-storage/src/lib.rs` | Add module, pub use, update StorageEngine::search |

## 5. Testing

**Unit tests** in `search_scope.rs`:
- Parse `"tag:rust programming"` → filters=[Tag("rust")], text="programming"
- Parse `"tag:rust path:notes/ hello world"` → filters=[Tag("rust"), Path("notes/")], text="hello world"
- Parse `"prop:status:done tasks"` → filters=[Property{key:"status", value:"done"}], text="tasks"
- Parse `"no scopes here"` → filters=[], text="no scopes here"
- Parse `"tag:rust"` → filters=[Tag("rust")], text=""
- Parse `"prop:malformed stuff"` → filters=[], text="prop:malformed stuff" (malformed prop treated as text)

**Integration tests** added to existing `prd-06-smoke.rs`:
- Write files with different tags, search with `tag:` scope, verify filtering
- Write files in different paths, search with `path:` scope
- Write files with properties, search with `prop:` scope
- Combined scope: `tag:X path:Y query`

## Out of Scope

- `type:heading` operator (filtering by block type within Tantivy — would require Tantivy schema change)
- Regex or glob patterns in scope values
- OR logic between scopes
