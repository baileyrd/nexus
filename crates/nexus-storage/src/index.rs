//! Index operations: insert, query, and delete files in the `SQLite` index.
//!
//! Provides CRUD operations over the `files`, `blocks`, `links`, `tags`,
//! `properties`, and `fts_blocks` tables defined in the schema migration.

use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::parser::{ParsedFile, Property};
use crate::StorageError;

// ── Public types ──────────────────────────────────────────────────────────────

/// A row from the `files` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    /// Primary key.
    pub id: u64,
    /// Vault-relative path.
    pub path: String,
    /// MIME-style type string, e.g. `"markdown"` or `"attachment"`.
    pub file_type: String,
    /// SHA-256 hex digest of the file content.
    pub content_hash: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of first insert.
    pub created_at: i64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// Whether the file has been soft-deleted.
    pub is_deleted: bool,
}

/// Lightweight metadata used by callers that do not need a full `FileRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Vault-relative path.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// SHA-256 hex digest of the file content.
    pub content_hash: String,
}

/// A row from the `blocks` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    /// Primary key.
    pub id: u64,
    /// FK into `files`.
    pub file_id: u64,
    /// Kind of block: `"heading"`, `"paragraph"`, etc.
    pub block_type: String,
    /// Heading level 1-6; `None` for non-headings.
    pub level: Option<i32>,
    /// Plain-text content.
    pub content: String,
    /// 1-based start line.
    pub start_line: u32,
    /// 1-based end line.
    pub end_line: u32,
    /// Block reference anchor id.
    pub block_ref_id: Option<String>,
    /// Callout type for callout blocks.
    pub callout_type: Option<String>,
}

/// A row from the `links` table.
#[derive(Debug, Clone)]
pub struct LinkRecord {
    /// Primary key.
    pub id: u64,
    /// FK into `files` for the file that contains this link.
    pub source_file_id: u64,
    /// The link target as written in the source (may be `None` for bare wikilinks).
    pub target_path: Option<String>,
    /// Resolved FK into `files`; `None` if the target could not be found.
    pub target_file_id: Option<u64>,
    /// Display text of the link.
    pub link_text: String,
    /// Kind of link: `"wikilink"`, `"markdown"`, or `"embed"`.
    pub link_type: String,
    /// Whether `target_file_id` was successfully resolved.
    pub is_resolved: bool,
    /// Fragment identifier from the link target.
    pub fragment: Option<String>,
}

/// A tag together with the file it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagResult {
    /// Tag name (without the `#` prefix).
    pub name: String,
    /// FK into `files`.
    pub file_id: u64,
    /// Vault-relative path of the file.
    pub file_path: String,
    /// Where the tag came from: `"frontmatter"` or `"inline"`.
    pub source: String,
}

/// Filter options for [`query_files`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileFilter {
    /// Only return files whose path starts with this prefix.
    pub prefix: Option<String>,
    /// Only return files of this type.
    pub file_type: Option<String>,
    /// When `false` (default), soft-deleted files are excluded.
    pub include_deleted: bool,
}

/// Summary statistics returned after a full index rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildStats {
    /// Number of files processed.
    pub files_processed: usize,
    /// Total blocks indexed.
    pub blocks_indexed: usize,
    /// Total links found.
    pub links_found: usize,
    /// Total tags found.
    pub tags_found: usize,
    /// Wall-clock time in milliseconds.
    pub duration_ms: u64,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Insert a file and all its parsed content into the index.
///
/// Inserts rows into `files`, `blocks`, `fts_blocks`, `links`, `tags`, and
/// `properties`. Returns the new file's row ID.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn insert_file(
    conn: &Connection,
    path: &str,
    file_type: &str,
    size_bytes: u64,
    parsed: &ParsedFile,
) -> Result<u64, StorageError> {
    let now = now_unix();

    // ── 1. Insert the file row ────────────────────────────────────────────────
    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5);",
        params![
            path,
            file_type,
            parsed.content_hash,
            size_bytes.cast_signed(),
            now
        ],
    )?;
    let file_id = u64::try_from(conn.last_insert_rowid()).unwrap_or(0);

    // ── 2. Blocks + FTS ──────────────────────────────────────────────────────
    for block in &parsed.blocks {
        conn.execute(
            "INSERT INTO blocks (file_id, block_type, level, content, start_line, end_line, block_ref_id, callout_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
            params![
                file_id.cast_signed(),
                block.block_type,
                block.level,
                block.content,
                block.start_line,
                block.end_line,
                block.block_ref_id,
                block.callout_type,
            ],
        )?;
        let block_rowid = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO fts_blocks (rowid, file_path, block_content, block_type)
             VALUES (?1, ?2, ?3, ?4);",
            params![block_rowid, path, block.content, block.block_type],
        )?;
    }

    // ── 3. Links ─────────────────────────────────────────────────────────────
    for link in &parsed.links {
        let resolve_target = link
            .target_path
            .as_deref()
            .or(Some(link.link_text.as_str()));
        let (target_file_id, is_resolved) = resolve_link(conn, resolve_target);
        conn.execute(
            "INSERT INTO links
                (source_file_id, target_path, target_file_id, link_text, link_type, is_resolved, fragment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
            params![
                file_id.cast_signed(),
                link.target_path,
                target_file_id.map(|id: u64| id.cast_signed()),
                link.link_text,
                link.link_type,
                is_resolved,
                link.fragment,
            ],
        )?;
    }

    // ── 4. Tags ──────────────────────────────────────────────────────────────
    for tag in &parsed.tags {
        conn.execute(
            "INSERT INTO tags (name, file_id, source) VALUES (?1, ?2, ?3);",
            params![tag.name, file_id.cast_signed(), tag.source],
        )?;
    }

    // ── 5. Properties ────────────────────────────────────────────────────────
    for prop in &parsed.frontmatter {
        insert_property(conn, file_id, prop)?;
    }

    // ── 6. Tasks ────────────────────────────────────────────────────────────
    crate::tasks::insert_tasks(conn, file_id, &parsed.tasks)?;

    // ── 7. Bidirectional wikilink resolution (BL-004) ────────────────────────
    // A newly-landed file may satisfy wikilinks from other notes that
    // were previously phantom. Retry every unresolved link and upgrade
    // any that now resolve. `resolve_link` covers the case where the
    // newly-inserted file matches via tier 1/2/3; others stay phantom.
    reresolve_unresolved_links(conn)?;

    Ok(file_id)
}

/// Return all files matching `filter`, ordered by path.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_files(
    conn: &Connection,
    filter: &FileFilter,
) -> Result<Vec<FileRecord>, StorageError> {
    // Build the WHERE clauses and a parallel params list dynamically.
    let mut clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !filter.include_deleted {
        clauses.push("is_deleted = 0".to_string());
    }

    if let Some(prefix) = &filter.prefix {
        clauses.push(format!("path LIKE ?{}", param_values.len() + 1));
        param_values.push(Box::new(format!("{prefix}%")));
    }

    if let Some(ft) = &filter.file_type {
        clauses.push(format!("file_type = ?{}", param_values.len() + 1));
        param_values.push(Box::new(ft.clone()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT id, path, file_type, content_hash, size_bytes, created_at, modified_at, is_deleted
         FROM files {where_clause} ORDER BY path;"
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values
        .iter()
        .map(std::convert::AsRef::as_ref)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), map_file_record)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Return all blocks belonging to `file_id`.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_blocks(conn: &Connection, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, block_type, level, content, start_line, end_line, block_ref_id, callout_type
         FROM blocks WHERE file_id = ?1 ORDER BY start_line;",
    )?;
    let rows = stmt.query_map(params![file_id.cast_signed()], |row| {
        Ok(BlockRecord {
            id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
            file_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
            block_type: row.get(2)?,
            level: row.get(3)?,
            content: row.get(4)?,
            start_line: u32::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
            end_line: u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
            block_ref_id: row.get(7)?,
            callout_type: row.get(8)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Return all outgoing links from `file_id`.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_links(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, source_file_id, target_path, target_file_id, link_text, link_type, is_resolved, fragment
         FROM links WHERE source_file_id = ?1;",
    )?;
    let rows = stmt.query_map(params![file_id.cast_signed()], map_link_record)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Return all links whose `target_file_id` is `file_id` (backlinks).
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_backlinks(conn: &Connection, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, source_file_id, target_path, target_file_id, link_text, link_type, is_resolved, fragment
         FROM links WHERE target_file_id = ?1;",
    )?;
    let rows = stmt.query_map(params![file_id.cast_signed()], map_link_record)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Return all tags with the given `name`, joined to the file path.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_tags(conn: &Connection, name: &str) -> Result<Vec<TagResult>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT t.name, t.file_id, f.path, t.source
         FROM tags t JOIN files f ON f.id = t.file_id
         WHERE t.name = ?1;",
    )?;
    let rows = stmt.query_map(params![name], |row| {
        Ok(TagResult {
            name: row.get(0)?,
            file_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
            file_path: row.get(2)?,
            source: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Hard-delete a file row (cascades to blocks, links, tags, properties).
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM files WHERE id = ?1;",
        params![file_id.cast_signed()],
    )?;
    Ok(())
}

/// Mark a file as deleted without removing its row from the database.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn soft_delete_file(conn: &Connection, file_id: u64) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE files SET is_deleted = 1 WHERE id = ?1;",
        params![file_id.cast_signed()],
    )?;
    // BL-004: any link that pointed to this file is now phantom —
    // clear its `target_file_id` + `is_resolved` so the graph + UI
    // reflect the deletion.
    invalidate_links_to(conn, file_id)?;
    Ok(())
}

/// Look up a file by its vault-relative path.
///
/// Returns `None` if no matching file is found.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure other than
/// `QueryReturnedNoRows`.
pub fn file_by_path(conn: &Connection, path: &str) -> Result<Option<FileRecord>, StorageError> {
    let result = conn.query_row(
        "SELECT id, path, file_type, content_hash, size_bytes, created_at, modified_at, is_deleted
         FROM files WHERE path = ?1;",
        params![path],
        map_file_record,
    );

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StorageError::Database(e)),
    }
}

// ── JSX component operations ─────────────────────────────────────────────────

/// A row from the `jsx_components` table.
#[derive(Debug, Clone)]
pub struct JsxRecord {
    /// Primary key.
    pub id: u64,
    /// Foreign key to the file.
    pub file_id: u64,
    /// Component name (e.g. `"Chart"`).
    pub name: String,
    /// Raw props as a JSON string.
    pub props_json: Option<String>,
    /// 1-based line number in the source file.
    pub line_number: Option<u32>,
    /// Whether the tag is self-closing.
    pub self_closing: bool,
    /// Unix timestamp of when the record was created.
    pub created_at: i64,
}

/// Insert a batch of JSX components for a given file.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn insert_jsx_components(
    conn: &Connection,
    file_id: u64,
    components: &[crate::mdx::ParsedJsxComponent],
) -> Result<(), StorageError> {
    let now = now_unix();
    for comp in components {
        conn.execute(
            "INSERT INTO jsx_components (file_id, name, props_json, line_number, self_closing, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![
                file_id.cast_signed(),
                comp.name,
                comp.props_json,
                comp.line_number,
                comp.self_closing,
                now,
            ],
        )?;
    }
    Ok(())
}

/// Query all JSX components belonging to a file.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_jsx_components(
    conn: &Connection,
    file_id: u64,
) -> Result<Vec<JsxRecord>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, name, props_json, line_number, self_closing, created_at
         FROM jsx_components WHERE file_id = ?1 ORDER BY line_number;",
    )?;
    let rows = stmt.query_map(params![file_id.cast_signed()], |row| {
        Ok(JsxRecord {
            id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
            file_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
            name: row.get(2)?,
            props_json: row.get(3)?,
            line_number: row.get::<_, Option<u32>>(4)?,
            self_closing: row.get::<_, bool>(5)?,
            created_at: row.get(6)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Attempt to resolve a link target to an existing file ID.
///
/// Cascade (BL-004):
/// 1. Exact path match — `[[folder/note]]` → `folder/note.md`.
/// 2. Filename-stem match — `[[note]]` → any file whose stem is `note`.
/// 3. Case-insensitive filename-stem / path match — `[[Note]]` → `note.md`.
///
/// A fourth, non-standard tier matches the target against frontmatter
/// `aliases` arrays. It's tried last so explicit wikilinks always win.
///
/// Returns `(Option<file_id>, is_resolved)`.
fn resolve_link(conn: &Connection, target: Option<&str>) -> (Option<u64>, bool) {
    let Some(target) = target else {
        return (None, false);
    };

    // Tier 1: exact path match.
    if let Ok(id) = conn.query_row(
        "SELECT id FROM files WHERE path = ?1 AND is_deleted = 0;",
        params![target],
        |row| row.get::<_, i64>(0),
    ) {
        return (Some(u64::try_from(id).unwrap_or(0)), true);
    }

    // Tier 2: filename-stem match (case-sensitive).
    let pattern_slash = format!("%/{target}.md");
    let bare = format!("{target}.md");
    if let Ok(id) = conn.query_row(
        "SELECT id FROM files WHERE (path LIKE ?1 OR path = ?2) AND is_deleted = 0;",
        params![pattern_slash, bare],
        |row| row.get::<_, i64>(0),
    ) {
        return (Some(u64::try_from(id).unwrap_or(0)), true);
    }

    // Tier 3: case-insensitive fallback. Matches both the full path and
    // the bare `.md` stem so `[[Note]]` resolves to `notes/note.md` just
    // as `[[note]]` would.
    if let Ok(id) = conn.query_row(
        "SELECT id FROM files
         WHERE is_deleted = 0
           AND (
             LOWER(path) = LOWER(?1)
             OR LOWER(path) LIKE LOWER(?2)
             OR LOWER(path) = LOWER(?3)
           )
         LIMIT 1;",
        params![target, pattern_slash, bare],
        |row| row.get::<_, i64>(0),
    ) {
        return (Some(u64::try_from(id).unwrap_or(0)), true);
    }

    // Alias match: check if any file has this as an alias in frontmatter
    // properties. Not part of the BL-004 spec but kept for backward
    // compatibility with notes that declare `aliases: [...]`.
    let alias_result = conn.query_row(
        "SELECT file_id, value FROM properties WHERE key = 'aliases' AND value LIKE ?1;",
        params![format!("%\"{target}\"%")],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    );

    if let Ok((fid, json_value)) = alias_result {
        if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&json_value) {
            if aliases.iter().any(|a| a == target) {
                return (Some(u64::try_from(fid).unwrap_or(0)), true);
            }
        }
    }

    (None, false)
}

/// Walk every unresolved link and retry resolution. Any link whose
/// target now resolves to a live file gets its `target_file_id` +
/// `is_resolved` columns updated.
///
/// Called from `insert_file` after a new file lands so pre-existing
/// phantom links pointing at this file upgrade automatically. Scoped by
/// cost, not correctness — running it any time the file set changes
/// would be equivalent.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn reresolve_unresolved_links(conn: &Connection) -> Result<usize, StorageError> {
    let mut stmt =
        conn.prepare("SELECT id, target_path, link_text FROM links WHERE is_resolved = 0;")?;
    let rows: Vec<(i64, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut upgraded = 0usize;
    for (link_id, target_path, link_text) in rows {
        let target = target_path.as_deref().or(Some(link_text.as_str()));
        let (new_file_id, is_resolved) = resolve_link(conn, target);
        if is_resolved {
            conn.execute(
                "UPDATE links SET target_file_id = ?1, is_resolved = 1 WHERE id = ?2;",
                params![new_file_id.map(|id: u64| id.cast_signed()), link_id],
            )?;
            upgraded += 1;
        }
    }
    Ok(upgraded)
}

/// Mark every link that pointed to `file_id` as unresolved. Called
/// from `soft_delete_file` so a deletion flips the affected links
/// back to phantom status instead of leaving stale `target_file_id`
/// references.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn invalidate_links_to(conn: &Connection, file_id: u64) -> Result<usize, StorageError> {
    let affected = conn.execute(
        "UPDATE links SET is_resolved = 0, target_file_id = NULL WHERE target_file_id = ?1;",
        params![file_id.cast_signed()],
    )?;
    Ok(affected)
}

/// Insert a single property row, ignoring duplicate `(file_id, key)` pairs.
/// Populates typed columns (`value_num`, `value_date`, `value_bool`) based on
/// the JSON value and `property_type` hint.
fn insert_property(conn: &Connection, file_id: u64, prop: &Property) -> Result<(), StorageError> {
    let (value_num, value_date, value_bool) =
        extract_typed_values(&prop.value, prop.property_type.as_deref());
    conn.execute(
        "INSERT OR IGNORE INTO properties (file_id, key, value, property_type, value_num, value_date, value_bool)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
        params![
            file_id.cast_signed(),
            prop.key,
            prop.value,
            prop.property_type,
            value_num,
            value_date,
            value_bool,
        ],
    )?;
    Ok(())
}

/// Extract typed values from a JSON-serialized property value.
fn extract_typed_values(
    json_value: &str,
    property_type: Option<&str>,
) -> (Option<f64>, Option<i64>, Option<bool>) {
    let mut num = None;
    let mut date = None;
    let mut bool_val = None;

    match property_type {
        Some("number") => {
            // JSON value like "42" or "3.14"
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_value) {
                num = v.as_f64();
            }
        }
        Some("string") => {
            // Check if the string value looks like YYYY-MM-DD
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_value) {
                if let Some(s) = v.as_str() {
                    if let Some(ts) = parse_date_to_unix(s) {
                        date = Some(ts);
                    }
                }
            }
        }
        _ => {}
    }

    // Check for boolean regardless of type hint
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_value) {
        if let Some(b) = v.as_bool() {
            bool_val = Some(b);
        }
    }

    (num, date, bool_val)
}

/// Parse a "YYYY-MM-DD" string to a Unix timestamp (midnight UTC).
/// Returns `None` if the string doesn't match the expected format.
fn parse_date_to_unix(s: &str) -> Option<i64> {
    // Simple validation: must be exactly 10 chars, format YYYY-MM-DD
    if s.len() != 10 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;

    // Validate ranges
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return None;
    }

    // Use chrono for accurate conversion
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// Map a `SQLite` row to a [`FileRecord`].
fn map_file_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        path: row.get(1)?,
        file_type: row.get(2)?,
        content_hash: row.get(3)?,
        size_bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
        created_at: row.get(5)?,
        modified_at: row.get(6)?,
        is_deleted: row.get::<_, bool>(7)?,
    })
}

/// Map a `SQLite` row to a [`LinkRecord`].
fn map_link_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LinkRecord> {
    Ok(LinkRecord {
        id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        source_file_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        target_path: row.get(2)?,
        target_file_id: row
            .get::<_, Option<i64>>(3)?
            .map(|id| u64::try_from(id).unwrap_or(0)),
        link_text: row.get(4)?,
        link_type: row.get(5)?,
        is_resolved: row.get::<_, bool>(6)?,
        fragment: row.get(7)?,
    })
}

/// Return the current Unix timestamp in seconds.
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ParsedBlock, ParsedLink, ParsedTag};

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();
        conn
    }

    /// A `ParsedFile` with 1 heading block, 1 paragraph block, 1 wikilink,
    /// 1 tag ("rust", "inline"), and 1 frontmatter property ("title").
    fn sample_parsed_file() -> ParsedFile {
        ParsedFile {
            content_hash: "abc123".to_string(),
            frontmatter: vec![Property {
                key: "title".to_string(),
                value: "\"My Note\"".to_string(),
                property_type: Some("string".to_string()),
            }],
            blocks: vec![
                ParsedBlock {
                    block_type: "heading".to_string(),
                    level: Some(1),
                    content: "Hello World".to_string(),
                    raw_markdown: None,
                    start_line: 1,
                    end_line: 1,
                    block_ref_id: None,
                    callout_type: None,
                },
                ParsedBlock {
                    block_type: "paragraph".to_string(),
                    level: None,
                    content: "Some sample content here.".to_string(),
                    raw_markdown: None,
                    start_line: 3,
                    end_line: 3,
                    block_ref_id: None,
                    callout_type: None,
                },
            ],
            links: vec![ParsedLink {
                link_text: "other note".to_string(),
                target_path: Some("other-note".to_string()),
                link_type: "wikilink".to_string(),
                fragment: None,
            }],
            tags: vec![ParsedTag {
                name: "rust".to_string(),
                source: "inline".to_string(),
            }],
            tasks: vec![],
        }
    }

    // ── 1. insert_file_returns_row_id ─────────────────────────────────────────
    #[test]
    fn insert_file_returns_row_id() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let id = insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        assert!(id > 0, "expected row id > 0, got {id}");
    }

    // ── 2. insert_file_stores_blocks ─────────────────────────────────────────
    #[test]
    fn insert_file_stores_blocks() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        let blocks = query_blocks(&conn, file_id).unwrap();
        assert_eq!(blocks.len(), 2, "expected 2 blocks, got {}", blocks.len());
    }

    // ── 3. insert_file_stores_links ──────────────────────────────────────────
    #[test]
    fn insert_file_stores_links() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        let links = query_links(&conn, file_id).unwrap();
        assert_eq!(links.len(), 1, "expected 1 link, got {}", links.len());
        assert_eq!(links[0].link_type, "wikilink");
    }

    // ── 4. insert_file_stores_tags ───────────────────────────────────────────
    #[test]
    fn insert_file_stores_tags() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        let tags = query_tags(&conn, "rust").unwrap();
        assert_eq!(tags.len(), 1, "expected 1 tag result, got {}", tags.len());
        assert_eq!(tags[0].file_path, "notes/test.md");
    }

    // ── 5. insert_file_stores_properties ─────────────────────────────────────
    #[test]
    fn insert_file_stores_properties() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        let value: String = conn
            .query_row(
                "SELECT value FROM properties WHERE file_id = ?1 AND key = 'title';",
                params![file_id.cast_signed()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "\"My Note\"");
    }

    // ── 6. query_files_with_prefix ───────────────────────────────────────────
    #[test]
    fn query_files_with_prefix() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/a.md", "markdown", 10, &parsed).unwrap();

        let parsed2 = ParsedFile {
            content_hash: "def456".to_string(),
            ..sample_parsed_file()
        };
        insert_file(&conn, "notes/b.md", "markdown", 10, &parsed2).unwrap();

        let parsed3 = ParsedFile {
            content_hash: "ghi789".to_string(),
            ..sample_parsed_file()
        };
        insert_file(&conn, "attachments/img.png", "attachment", 5000, &parsed3).unwrap();

        let filter = FileFilter {
            prefix: Some("notes/".to_string()),
            ..Default::default()
        };
        let results = query_files(&conn, &filter).unwrap();
        assert_eq!(results.len(), 2, "expected 2 notes, got {}", results.len());
    }

    // ── 7. query_files_with_type_filter ──────────────────────────────────────
    #[test]
    fn query_files_with_type_filter() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/a.md", "markdown", 10, &parsed).unwrap();

        let parsed2 = ParsedFile {
            content_hash: "def456".to_string(),
            ..sample_parsed_file()
        };
        insert_file(&conn, "attachments/img.png", "attachment", 5000, &parsed2).unwrap();

        let filter = FileFilter {
            file_type: Some("attachment".to_string()),
            ..Default::default()
        };
        let results = query_files(&conn, &filter).unwrap();
        assert_eq!(
            results.len(),
            1,
            "expected 1 attachment, got {}",
            results.len()
        );
        assert_eq!(results[0].file_type, "attachment");
    }

    // ── 8. query_files_excludes_deleted_by_default ───────────────────────────
    #[test]
    fn query_files_excludes_deleted_by_default() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/gone.md", "markdown", 10, &parsed).unwrap();
        soft_delete_file(&conn, file_id).unwrap();

        let results = query_files(&conn, &FileFilter::default()).unwrap();
        assert!(results.is_empty(), "soft-deleted file should not appear");
    }

    // ── 9. query_files_includes_deleted_when_requested ───────────────────────
    #[test]
    fn query_files_includes_deleted_when_requested() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/gone.md", "markdown", 10, &parsed).unwrap();
        soft_delete_file(&conn, file_id).unwrap();

        let filter = FileFilter {
            include_deleted: true,
            ..Default::default()
        };
        let results = query_files(&conn, &filter).unwrap();
        assert_eq!(results.len(), 1, "expected 1 result, got {}", results.len());
        assert!(results[0].is_deleted, "expected is_deleted = true");
    }

    // ── 10. query_backlinks_finds_linking_files ───────────────────────────────
    #[test]
    fn query_backlinks_finds_linking_files() {
        let conn = setup_db();

        // File A has no links.
        let file_a = ParsedFile {
            content_hash: "aaa".to_string(),
            frontmatter: vec![],
            blocks: vec![],
            links: vec![],
            tags: vec![],
            tasks: vec![],
        };
        let a_id = insert_file(&conn, "notes/a.md", "markdown", 10, &file_a).unwrap();

        // File B links to A's path.
        let file_b = ParsedFile {
            content_hash: "bbb".to_string(),
            frontmatter: vec![],
            blocks: vec![],
            links: vec![ParsedLink {
                link_text: "a".to_string(),
                target_path: Some("notes/a.md".to_string()),
                link_type: "wikilink".to_string(),
                fragment: None,
            }],
            tags: vec![],
            tasks: vec![],
        };
        insert_file(&conn, "notes/b.md", "markdown", 10, &file_b).unwrap();

        let backlinks = query_backlinks(&conn, a_id).unwrap();
        assert_eq!(
            backlinks.len(),
            1,
            "expected 1 backlink, got {}",
            backlinks.len()
        );
    }

    // ── 11. delete_file_cascades ──────────────────────────────────────────────
    #[test]
    fn delete_file_cascades() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        let file_id = insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();

        // Sanity: blocks exist before deletion.
        let before = query_blocks(&conn, file_id).unwrap();
        assert!(!before.is_empty());

        delete_file(&conn, file_id).unwrap();

        let after = query_blocks(&conn, file_id).unwrap();
        assert!(
            after.is_empty(),
            "cascaded delete should remove child blocks"
        );
    }

    // ── 12. file_by_path_returns_none_for_missing ─────────────────────────────
    #[test]
    fn file_by_path_returns_none_for_missing() {
        let conn = setup_db();
        let result = file_by_path(&conn, "does/not/exist.md").unwrap();
        assert!(result.is_none());
    }

    // ── 13. file_by_path_returns_some_for_existing ────────────────────────────
    #[test]
    fn file_by_path_returns_some_for_existing() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();
        let result = file_by_path(&conn, "notes/test.md").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().path, "notes/test.md");
    }

    // ── 14. insert_file_populates_fts ─────────────────────────────────────────
    #[test]
    fn insert_file_populates_fts() {
        let conn = setup_db();
        let parsed = sample_parsed_file();
        insert_file(&conn, "notes/test.md", "markdown", 100, &parsed).unwrap();

        // "content" appears in the paragraph block: "Some sample content here."
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_blocks WHERE fts_blocks MATCH 'content';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            count > 0,
            "FTS should have at least one match for 'content'"
        );
    }

    // ── 15. insert_property_populates_typed_columns ──────────────────────────
    #[test]
    fn insert_property_populates_typed_columns() {
        let conn = setup_db();
        let parsed = ParsedFile {
            content_hash: "typed1".to_string(),
            frontmatter: vec![
                Property {
                    key: "count".to_string(),
                    value: "42".to_string(),
                    property_type: Some("number".to_string()),
                },
                Property {
                    key: "date".to_string(),
                    value: "\"2026-04-13\"".to_string(),
                    property_type: Some("string".to_string()),
                },
                Property {
                    key: "draft".to_string(),
                    value: "true".to_string(),
                    property_type: Some("string".to_string()),
                },
            ],
            blocks: vec![],
            links: vec![],
            tags: vec![],
            tasks: vec![],
        };
        let file_id = insert_file(&conn, "notes/typed.md", "markdown", 100, &parsed).unwrap();

        // Check value_num
        let num: Option<f64> = conn
            .query_row(
                "SELECT value_num FROM properties WHERE file_id = ?1 AND key = 'count';",
                params![file_id.cast_signed()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(num, Some(42.0));

        // Check value_date
        let date: Option<i64> = conn
            .query_row(
                "SELECT value_date FROM properties WHERE file_id = ?1 AND key = 'date';",
                params![file_id.cast_signed()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(date.is_some(), "date should be populated");

        // Check value_bool
        let bool_val: Option<bool> = conn
            .query_row(
                "SELECT value_bool FROM properties WHERE file_id = ?1 AND key = 'draft';",
                params![file_id.cast_signed()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(bool_val, Some(true));
    }

    // ── BL-004: 3-tier wikilink resolution ───────────────────────────────────

    /// Build a `ParsedFile` with one outgoing wikilink to `target`.
    fn parsed_with_link(target: &str) -> ParsedFile {
        ParsedFile {
            content_hash: format!("hash-for-{target}"),
            frontmatter: vec![],
            blocks: vec![],
            links: vec![ParsedLink {
                link_text: target.to_string(),
                target_path: Some(target.to_string()),
                link_type: "wikilink".to_string(),
                fragment: None,
            }],
            tags: vec![],
            tasks: vec![],
        }
    }

    fn parsed_empty() -> ParsedFile {
        ParsedFile {
            content_hash: "empty".to_string(),
            frontmatter: vec![],
            blocks: vec![],
            links: vec![],
            tags: vec![],
            tasks: vec![],
        }
    }

    #[test]
    fn resolve_link_exact_path_wins() {
        let conn = setup_db();
        let target_id =
            insert_file(&conn, "notes/target.md", "markdown", 10, &parsed_empty()).unwrap();
        let src_id = insert_file(
            &conn,
            "notes/src.md",
            "markdown",
            10,
            &parsed_with_link("notes/target.md"),
        )
        .unwrap();
        let links = query_links(&conn, src_id).unwrap();
        assert_eq!(links.len(), 1);
        assert!(links[0].is_resolved);
        assert_eq!(links[0].target_file_id, Some(target_id));
    }

    #[test]
    fn resolve_link_case_insensitive_fallback() {
        // Tier 3: [[Target]] resolves to `notes/target.md` even with
        // different case.
        let conn = setup_db();
        let target_id =
            insert_file(&conn, "notes/target.md", "markdown", 10, &parsed_empty()).unwrap();
        let src_id = insert_file(
            &conn,
            "notes/src.md",
            "markdown",
            10,
            &parsed_with_link("Target"),
        )
        .unwrap();
        let links = query_links(&conn, src_id).unwrap();
        assert!(
            links[0].is_resolved,
            "[[Target]] should resolve to target.md"
        );
        assert_eq!(links[0].target_file_id, Some(target_id));
    }

    #[test]
    fn insert_upgrades_phantom_links_pointing_to_new_file() {
        // Bidirectional resolution: a note with a phantom `[[target]]`
        // link should have that link upgraded when `target.md` later
        // lands.
        let conn = setup_db();
        let src_id = insert_file(
            &conn,
            "notes/src.md",
            "markdown",
            10,
            &parsed_with_link("target"),
        )
        .unwrap();
        let links_before = query_links(&conn, src_id).unwrap();
        assert!(
            !links_before[0].is_resolved,
            "precondition: link starts unresolved"
        );

        let target_id =
            insert_file(&conn, "notes/target.md", "markdown", 10, &parsed_empty()).unwrap();

        let links_after = query_links(&conn, src_id).unwrap();
        assert!(
            links_after[0].is_resolved,
            "link should have been upgraded after target landed"
        );
        assert_eq!(links_after[0].target_file_id, Some(target_id));
    }

    #[test]
    fn soft_delete_invalidates_incoming_links() {
        // Deleting `target.md` should flip links pointing to it back to
        // phantom (is_resolved = 0, target_file_id = NULL).
        let conn = setup_db();
        let target_id =
            insert_file(&conn, "notes/target.md", "markdown", 10, &parsed_empty()).unwrap();
        let src_id = insert_file(
            &conn,
            "notes/src.md",
            "markdown",
            10,
            &parsed_with_link("notes/target.md"),
        )
        .unwrap();
        let links_before = query_links(&conn, src_id).unwrap();
        assert!(links_before[0].is_resolved);

        soft_delete_file(&conn, target_id).unwrap();

        let links_after = query_links(&conn, src_id).unwrap();
        assert!(
            !links_after[0].is_resolved,
            "link should be phantom after target was deleted"
        );
        assert_eq!(links_after[0].target_file_id, None);
    }
}
