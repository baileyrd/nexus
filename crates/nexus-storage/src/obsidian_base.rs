//! Obsidian `.base` file query — read the YAML, walk the index, evaluate
//! the filter against each markdown note, project rows.
//!
//! See ADR 0019. The pure types and expression evaluator live in
//! [`nexus_types::obsidian_base`]; this module owns the `SQLite` /
//! filesystem glue. The view layer never sees this module directly —
//! it consumes [`ObsidianBaseQueryResult`] over IPC.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_types::obsidian_base::{
    self,
    filter::{evaluate, NoteFacts},
    ObsidianBase, ObsidianView,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::StorageError;

// ── Public surface ──────────────────────────────────────────────────────────

/// One row of a `.base` query result.
///
/// Shape mirrors `nexus_types::bases::BaseRecord` so the existing
/// shell view layer (`BasesTable`, `BasesGallery`, …) consumes both
/// formats through a single code path. `id` is the source note's
/// forge-relative path — stable across queries and unique by index
/// invariant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianBaseRow {
    /// Stable row identifier. Set to the note's forge-relative path.
    pub id: String,
    /// Field values keyed by the property paths declared in the
    /// `.base` file's `properties:` section.
    pub fields: serde_json::Map<String, Value>,
}

/// Result of querying a `.base` file against the current vault state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianBaseQueryResult {
    /// Property paths in declaration order (the synthesized "schema").
    pub columns: Vec<String>,
    /// Display-name overrides keyed by property path. Pulled from the
    /// `displayName` field under each `properties:` entry.
    pub display_names: BTreeMap<String, String>,
    /// Matched rows in vault-iteration order. Sort is applied by the
    /// view, not here.
    pub rows: Vec<ObsidianBaseRow>,
    /// View definitions verbatim from the `.base` file.
    pub views: Vec<ObsidianView>,
    /// Distinct filter expressions that fell outside the v1 grammar.
    /// Empty on the happy path. Surfaced by the UI as a banner.
    pub unsupported_filters: Vec<String>,
}

/// Read, parse, and evaluate `<forge_root>/<base_path>` against the
/// indexed vault state available through `conn`.
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] when the `.base` file is
/// missing or malformed, or [`StorageError::Database`] on `SQLite`
/// failure.
pub fn query(
    conn: &Connection,
    forge_root: &Path,
    base_path: &str,
) -> Result<ObsidianBaseQueryResult, StorageError> {
    let abs = forge_root.join(base_path);
    let yaml =
        std::fs::read_to_string(&abs).map_err(|e| StorageError::CorruptFile {
            path: base_path.to_string(),
            reason: format!("read .base: {e}"),
        })?;
    let base: ObsidianBase =
        obsidian_base::parse(&yaml).map_err(|e| StorageError::CorruptFile {
            path: base_path.to_string(),
            reason: e.to_string(),
        })?;

    let columns: Vec<String> = base.properties.keys().cloned().collect();
    let display_names = collect_display_names(&base);

    let mut rows = Vec::new();
    let mut unsupported_filters: Vec<String> = Vec::new();

    for note in load_candidate_notes(conn)? {
        let facts = build_note_facts(conn, &note)?;
        let report = evaluate(base.filters.as_ref(), &facts);
        for entry in report.unsupported {
            if !unsupported_filters.contains(&entry) {
                unsupported_filters.push(entry);
            }
        }
        if !report.matched {
            continue;
        }
        let fields = project_columns(&columns, &facts);
        rows.push(ObsidianBaseRow {
            id: facts.path.clone(),
            fields,
        });
    }

    Ok(ObsidianBaseQueryResult {
        columns,
        display_names,
        rows,
        views: base.views,
        unsupported_filters,
    })
}

// ── Internals ───────────────────────────────────────────────────────────────

fn collect_display_names(base: &ObsidianBase) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, value) in &base.properties {
        if let Some(name) = value.get("displayName").and_then(Value::as_str) {
            out.insert(key.clone(), name.to_string());
        }
    }
    out
}

/// Minimal note row needed before we hydrate frontmatter / tags.
struct NoteRow {
    file_id: i64,
    path: String,
    created_at: i64,
    modified_at: i64,
}

fn load_candidate_notes(conn: &Connection) -> Result<Vec<NoteRow>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, path, created_at, modified_at
           FROM files
          WHERE file_type = 'markdown'
            AND is_deleted = 0
          ORDER BY path",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(NoteRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
                modified_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn build_note_facts(conn: &Connection, note: &NoteRow) -> Result<NoteFacts, StorageError> {
    let path = std::path::Path::new(&note.path);
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let folder = path
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();

    let frontmatter = load_frontmatter(conn, note.file_id)?;
    let tags = load_tags(conn, note.file_id)?;

    Ok(NoteFacts {
        name,
        path: note.path.clone(),
        ext,
        folder,
        ctime: note.created_at,
        mtime: note.modified_at,
        tags,
        frontmatter,
    })
}

fn load_frontmatter(conn: &Connection, file_id: i64) -> Result<BTreeMap<String, Value>, StorageError> {
    let mut stmt = conn.prepare("SELECT key, value FROM properties WHERE file_id = ?1")?;
    let rows = stmt
        .query_map(params![file_id], |row| {
            let key: String = row.get(0)?;
            let value_str: String = row.get(1)?;
            Ok((key, value_str))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut map = BTreeMap::new();
    for (key, value_str) in rows {
        // The `value` column holds JSON-encoded scalars/arrays/objects
        // (see `index::insert_property`). Fall back to the raw string
        // on parse failure so a corrupt row doesn't poison the query.
        let parsed: Value = match serde_json::from_str(&value_str) {
            Ok(v) => v,
            Err(_) => Value::String(value_str),
        };
        map.insert(key, parsed);
    }
    Ok(map)
}

fn load_tags(conn: &Connection, file_id: i64) -> Result<Vec<String>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT name FROM tags WHERE file_id = ?1 ORDER BY name")?;
    let rows = stmt
        .query_map(params![file_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Build one row's `fields` map by resolving each declared column
/// against the note's facts.
fn project_columns(columns: &[String], facts: &NoteFacts) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for col in columns {
        out.insert(col.clone(), resolve_column(col, facts));
    }
    out
}

fn resolve_column(col: &str, facts: &NoteFacts) -> Value {
    match col {
        "file.name" => Value::String(facts.name.clone()),
        "file.path" => Value::String(facts.path.clone()),
        "file.ext" => Value::String(facts.ext.clone()),
        "file.folder" => Value::String(facts.folder.clone()),
        "file.ctime" => Value::Number(facts.ctime.into()),
        "file.mtime" => Value::Number(facts.mtime.into()),
        "file.tags" => Value::Array(facts.tags.iter().map(|t| Value::String(t.clone())).collect()),
        other => facts
            .frontmatter
            .get(other)
            .cloned()
            .unwrap_or(Value::Null),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::migrate;
    use rusqlite::Connection;
    use tempfile::tempdir;

    /// In-memory test DB with the migrated schema.
    fn make_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    /// Insert a markdown file with the given frontmatter (key → JSON-encoded value)
    /// and tags. Returns the file id.
    fn insert_note(
        conn: &Connection,
        path: &str,
        frontmatter: &[(&str, &str)],
        tags: &[&str],
        ctime: i64,
        mtime: i64,
    ) -> i64 {
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES (?1, 'markdown', 'h', 0, ?2, ?3);",
            params![path, ctime, mtime],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        for (k, v) in frontmatter {
            conn.execute(
                "INSERT INTO properties (file_id, key, value, property_type)
                 VALUES (?1, ?2, ?3, NULL);",
                params![id, k, v],
            )
            .unwrap();
        }
        for tag in tags {
            conn.execute(
                "INSERT INTO tags (name, file_id, source) VALUES (?1, ?2, 'frontmatter');",
                params![tag, id],
            )
            .unwrap();
        }
        id
    }

    fn write_base(root: &Path, name: &str, contents: &str) -> String {
        let path = root.join(name);
        std::fs::write(&path, contents).unwrap();
        name.to_string()
    }

    const READING_BASE: &str = r#"filters:
  and:
    - 'type == "literature"'
properties:
  file.name:
    displayName: Note
  title:
    displayName: Title
  author:
    displayName: Author
  year:
    displayName: Year
views:
  - type: table
    name: Library
    order: [title, author, year]
"#;

    #[test]
    fn query_returns_only_matching_notes() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        insert_note(
            &conn,
            "books/dune.md",
            &[
                ("type", "\"literature\""),
                ("title", "\"Dune\""),
                ("author", "\"Frank Herbert\""),
                ("year", "1965"),
            ],
            &[],
            100,
            200,
        );
        insert_note(
            &conn,
            "movies/inception.md",
            &[
                ("type", "\"movie\""),
                ("title", "\"Inception\""),
            ],
            &[],
            100,
            200,
        );
        let base_path = write_base(dir.path(), "reading.base", READING_BASE);

        let result = query(&conn, dir.path(), &base_path).unwrap();

        assert_eq!(result.columns.len(), 4);
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert_eq!(row.id, "books/dune.md");
        assert_eq!(row.fields["title"].as_str().unwrap(), "Dune");
        assert_eq!(row.fields["author"].as_str().unwrap(), "Frank Herbert");
        assert_eq!(row.fields["year"].as_i64().unwrap(), 1965);
        assert_eq!(row.fields["file.name"].as_str().unwrap(), "dune");

        assert_eq!(result.display_names["title"], "Title");
        assert!(result.unsupported_filters.is_empty());
        assert_eq!(result.views.len(), 1);
        assert_eq!(result.views[0].name, "Library");
    }

    #[test]
    fn empty_filter_section_matches_every_markdown_note() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        insert_note(&conn, "a.md", &[], &[], 0, 0);
        insert_note(&conn, "b.md", &[], &[], 0, 0);
        let base_path = write_base(
            dir.path(),
            "all.base",
            "properties:\n  file.name:\n    displayName: Name\n",
        );
        let result = query(&conn, dir.path(), &base_path).unwrap();
        assert_eq!(result.rows.len(), 2);
        assert!(result.unsupported_filters.is_empty());
    }

    #[test]
    fn unsupported_filter_surfaces_with_zero_rows_for_that_branch() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        insert_note(&conn, "a.md", &[("type", "\"book\"")], &[], 0, 0);
        let base_path = write_base(
            dir.path(),
            "x.base",
            "filters:\n  and:\n    - 'formula(x) > 1'\nproperties:\n  title: {}\n",
        );
        let result = query(&conn, dir.path(), &base_path).unwrap();
        assert_eq!(result.rows.len(), 0);
        assert_eq!(result.unsupported_filters.len(), 1);
        assert!(result.unsupported_filters[0].contains("formula(x) > 1"));
    }

    #[test]
    fn file_intrinsics_resolve_in_columns_and_filters() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        insert_note(&conn, "books/a.md", &[], &[], 100, 200);
        insert_note(&conn, "movies/b.md", &[], &[], 100, 200);
        let base_path = write_base(
            dir.path(),
            "books.base",
            r#"filters:
  and:
    - 'file.folder == "books"'
properties:
  file.name: {}
  file.path: {}
  file.mtime: {}
"#,
        );
        let result = query(&conn, dir.path(), &base_path).unwrap();
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert_eq!(row.fields["file.name"].as_str().unwrap(), "a");
        assert_eq!(row.fields["file.path"].as_str().unwrap(), "books/a.md");
        assert_eq!(row.fields["file.mtime"].as_i64().unwrap(), 200);
    }

    #[test]
    fn tag_filter_via_array_equality() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        insert_note(&conn, "a.md", &[], &["book", "scifi"], 0, 0);
        insert_note(&conn, "b.md", &[], &["movie"], 0, 0);
        let base_path = write_base(
            dir.path(),
            "tagged.base",
            r#"filters:
  and:
    - 'file.tags == "book"'
properties:
  file.name: {}
"#,
        );
        let result = query(&conn, dir.path(), &base_path).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].fields["file.name"].as_str().unwrap(), "a");
    }

    #[test]
    fn missing_base_file_returns_corrupt_file() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        let err = query(&conn, dir.path(), "nope.base").unwrap_err();
        assert!(matches!(err, StorageError::CorruptFile { .. }));
    }

    #[test]
    fn malformed_yaml_returns_corrupt_file() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        let path = write_base(dir.path(), "bad.base", "filters:\n  and: [oops");
        let err = query(&conn, dir.path(), &path).unwrap_err();
        assert!(matches!(err, StorageError::CorruptFile { .. }));
    }

    #[test]
    fn deleted_files_are_excluded() {
        let dir = tempdir().unwrap();
        let conn = make_conn();
        let id = insert_note(&conn, "a.md", &[("type", "\"book\"")], &[], 0, 0);
        conn.execute("UPDATE files SET is_deleted = 1 WHERE id = ?1;", params![id])
            .unwrap();
        let base_path = write_base(
            dir.path(),
            "x.base",
            "properties:\n  file.name: {}\n",
        );
        let result = query(&conn, dir.path(), &base_path).unwrap();
        assert_eq!(result.rows.len(), 0);
    }
}
