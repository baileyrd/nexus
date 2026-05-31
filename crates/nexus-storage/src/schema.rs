//! `SQLite` schema definitions and migration runner for the nexus-storage crate.
//!
//! Manages pragma configuration and incremental schema migrations for the
//! nexus index database.
//!
//! #202 / R19 — the module-wide `#![allow(dead_code)]` was previously
//! suppressing every `pub` item here. Today `configure_pragmas` and
//! `migrate` are only reached from the `index.rs` test fixture, so the
//! per-item allow on [`CURRENT_VERSION`] (the one constant that has no
//! caller yet) is sufficient and the broad allow has been removed.

use crate::StorageError;
use rusqlite::Connection;

/// The current schema version this crate expects.
///
/// #202 / R19 — exported as part of the public schema-introspection
/// surface; no in-crate caller today beyond the migration tests, but
/// the value is the canonical answer to "what version does this build
/// know how to drive". Kept `pub` so external consumers can compare;
/// `#[allow(dead_code)]` documents the intentional unused-locally
/// state without leaking the suppression to the rest of the module.
#[allow(dead_code)]
pub const CURRENT_VERSION: u32 = 8;

/// Configure `SQLite` pragmas for optimal performance and consistency.
///
/// Sets WAL journal mode, NORMAL synchronous mode, a 16 MB page cache,
/// and enforces foreign-key constraints.
///
/// # Errors
///
/// Returns `StorageError` if the database operation fails.
pub fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -16000;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

/// Run all pending migrations and return the current schema version.
///
/// Creates the `_schema_version` tracking table on first call. Each
/// migration runs inside a transaction; a failure rolls back automatically.
/// Calling this function multiple times is a no-op after the database has
/// already been migrated to `CURRENT_VERSION`.
///
/// # Errors
///
/// Returns `StorageError` if the database operation fails.
pub fn migrate(conn: &Connection) -> Result<u32, StorageError> {
    // Ensure the version-tracking table exists.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_version (
            version    INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )?;

    // Determine which version we are currently at.
    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _schema_version;",
        [],
        |row| row.get(0),
    )?;

    if current < 1 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_001(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (1, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 2 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_002(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (2, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 3 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_003(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (3, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 4 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_004(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (4, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 5 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_005(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (5, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 6 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_006(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (6, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 7 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_007(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (7, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    if current < 8 {
        let tx = conn.unchecked_transaction()?;
        apply_migration_008(&tx)?;
        tx.execute(
            "INSERT INTO _schema_version (version, applied_at) VALUES (8, unixepoch());",
            [],
        )?;
        tx.commit()?;
    }

    // Re-read the authoritative version from the table.
    let version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _schema_version;",
        [],
        |row| row.get(0),
    )?;

    Ok(version)
}

// ---------------------------------------------------------------------------
// Private migration steps
// ---------------------------------------------------------------------------

fn apply_migration_001(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        // ── files ──────────────────────────────────────────────────────────
        "CREATE TABLE IF NOT EXISTS files (
            id           INTEGER PRIMARY KEY,
            path         TEXT    NOT NULL UNIQUE,
            file_type    TEXT    NOT NULL,
            content_hash TEXT    NOT NULL,
            size_bytes   INTEGER NOT NULL,
            created_at   INTEGER NOT NULL,
            modified_at  INTEGER NOT NULL,
            is_deleted   BOOLEAN DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_files_path_type ON files(path, file_type);
        CREATE INDEX IF NOT EXISTS idx_files_hash      ON files(content_hash);

        -- ── blocks ─────────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS blocks (
            id              INTEGER PRIMARY KEY,
            file_id         INTEGER NOT NULL,
            block_type      TEXT    NOT NULL,
            level           INTEGER,
            content         TEXT    NOT NULL,
            raw_markdown    TEXT,
            start_line      INTEGER NOT NULL,
            end_line        INTEGER NOT NULL,
            parent_block_id INTEGER,
            FOREIGN KEY(file_id)         REFERENCES files(id)  ON DELETE CASCADE,
            FOREIGN KEY(parent_block_id) REFERENCES blocks(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_blocks_file_id ON blocks(file_id);
        CREATE INDEX IF NOT EXISTS idx_blocks_type    ON blocks(block_type);

        -- ── links ──────────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS links (
            id             INTEGER PRIMARY KEY,
            source_file_id INTEGER NOT NULL,
            source_block_id INTEGER,
            target_path    TEXT,
            target_file_id INTEGER,
            link_text      TEXT    NOT NULL,
            link_type      TEXT    NOT NULL,
            is_resolved    BOOLEAN DEFAULT 0,
            FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(target_file_id) REFERENCES files(id) ON DELETE SET NULL
        );
        CREATE INDEX IF NOT EXISTS idx_links_source ON links(source_file_id);
        CREATE INDEX IF NOT EXISTS idx_links_target ON links(target_file_id);

        -- ── tags ───────────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS tags (
            id       INTEGER PRIMARY KEY,
            name     TEXT    NOT NULL,
            file_id  INTEGER NOT NULL,
            block_id INTEGER,
            source   TEXT    NOT NULL,
            FOREIGN KEY(file_id)  REFERENCES files(id)  ON DELETE CASCADE,
            FOREIGN KEY(block_id) REFERENCES blocks(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);
        CREATE INDEX IF NOT EXISTS idx_tags_file ON tags(file_id);

        -- ── properties ─────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS properties (
            id            INTEGER PRIMARY KEY,
            file_id       INTEGER NOT NULL,
            key           TEXT    NOT NULL,
            value         TEXT    NOT NULL,
            property_type TEXT,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            UNIQUE(file_id, key)
        );

        -- ── fts_blocks (FTS5 virtual table) ────────────────────────────────
        CREATE VIRTUAL TABLE IF NOT EXISTS fts_blocks USING fts5(
            file_path    UNINDEXED,
            block_content,
            block_type   UNINDEXED
        );",
    )?;
    Ok(())
}

/// Add typed property columns for numeric, date, and boolean values.
fn apply_migration_003(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "ALTER TABLE properties ADD COLUMN value_num REAL;
         ALTER TABLE properties ADD COLUMN value_date INTEGER;
         ALTER TABLE properties ADD COLUMN value_bool BOOLEAN;",
    )?;
    Ok(())
}

/// Create the `embeddings` table for storing chunk vectors.
fn apply_migration_004(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embeddings (
            id          INTEGER PRIMARY KEY,
            file_path   TEXT NOT NULL,
            block_id    INTEGER NOT NULL,
            chunk_text  TEXT NOT NULL,
            embedding   BLOB NOT NULL,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_embeddings_file ON embeddings(file_path);",
    )?;
    Ok(())
}

/// Create the `jsx_components` table for MDX component tracking.
fn apply_migration_005(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jsx_components (
            id           INTEGER PRIMARY KEY,
            file_id      INTEGER NOT NULL,
            name         TEXT NOT NULL,
            props_json   TEXT,
            line_number  INTEGER,
            self_closing BOOLEAN DEFAULT 0,
            created_at   INTEGER NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_jsx_file ON jsx_components(file_id);
        CREATE INDEX IF NOT EXISTS idx_jsx_name ON jsx_components(name);",
    )?;
    Ok(())
}

/// Create tables for canvas nodes/edges and bases databases.
fn apply_migration_006(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "-- Canvas nodes
        CREATE TABLE IF NOT EXISTS canvas_nodes (
            id           INTEGER PRIMARY KEY,
            file_id      INTEGER NOT NULL,
            node_id      TEXT NOT NULL,
            node_type    TEXT NOT NULL,
            x            REAL NOT NULL,
            y            REAL NOT NULL,
            width        REAL NOT NULL,
            height       REAL NOT NULL,
            color        TEXT,
            label        TEXT,
            collapsed    BOOLEAN DEFAULT 0,
            content_json TEXT,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_canvas_nodes_file ON canvas_nodes(file_id);

        -- Canvas edges
        CREATE TABLE IF NOT EXISTS canvas_edges (
            id           INTEGER PRIMARY KEY,
            file_id      INTEGER NOT NULL,
            edge_id      TEXT NOT NULL,
            from_node    TEXT NOT NULL,
            to_node      TEXT NOT NULL,
            edge_type    TEXT NOT NULL DEFAULT 'solid',
            label        TEXT,
            color        TEXT,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_canvas_edges_file ON canvas_edges(file_id);

        -- Bases databases
        CREATE TABLE IF NOT EXISTS bases (
            id            INTEGER PRIMARY KEY,
            path          TEXT NOT NULL UNIQUE,
            name          TEXT NOT NULL,
            schema_json   TEXT NOT NULL,
            metadata_json TEXT,
            created_at    INTEGER NOT NULL,
            modified_at   INTEGER NOT NULL
        );

        -- Base records
        CREATE TABLE IF NOT EXISTS bases_records (
            id          INTEGER PRIMARY KEY,
            base_id     INTEGER NOT NULL,
            record_id   TEXT NOT NULL,
            data_json   TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            modified_at INTEGER NOT NULL,
            FOREIGN KEY(base_id) REFERENCES bases(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bases_records_base ON bases_records(base_id);

        -- Base views
        CREATE TABLE IF NOT EXISTS bases_views (
            id          INTEGER PRIMARY KEY,
            base_id     INTEGER NOT NULL,
            name        TEXT NOT NULL,
            view_type   TEXT NOT NULL,
            config_json TEXT NOT NULL,
            FOREIGN KEY(base_id) REFERENCES bases(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bases_views_base ON bases_views(base_id);",
    )?;
    Ok(())
}

fn apply_migration_007(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "-- Schema version tracking for database engine migrations
        CREATE TABLE IF NOT EXISTS bases_schema_versions (
            id          INTEGER PRIMARY KEY,
            base_id     INTEGER NOT NULL,
            version     INTEGER NOT NULL,
            operation   TEXT NOT NULL,
            applied_at  INTEGER NOT NULL,
            FOREIGN KEY(base_id) REFERENCES bases(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bsv_base ON bases_schema_versions(base_id);",
    )?;
    Ok(())
}

fn apply_migration_008(conn: &Connection) -> Result<(), StorageError> {
    // BL-114 — code-symbol index. One row per declared symbol; parent_id
    // chains a Method back to its enclosing Class / Impl so callers can
    // resolve `Type::method` without re-parsing. `path` is forge-relative;
    // the row is keyed by id so a path-rename only needs to flip `path`
    // rather than rewrite parent pointers. ON DELETE CASCADE on parent_id
    // keeps an enclosing-symbol removal from leaving orphan children.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS code_symbols (
            id           INTEGER PRIMARY KEY,
            path         TEXT NOT NULL,
            language     TEXT NOT NULL,
            kind         TEXT NOT NULL,
            name         TEXT NOT NULL,
            line_start   INTEGER NOT NULL,
            line_end     INTEGER NOT NULL,
            parent_id    INTEGER REFERENCES code_symbols(id) ON DELETE CASCADE,
            doc_comment  TEXT,
            indexed_at   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_code_symbols_name      ON code_symbols(name);
        CREATE INDEX IF NOT EXISTS idx_code_symbols_path      ON code_symbols(path);
        CREATE INDEX IF NOT EXISTS idx_code_symbols_path_name ON code_symbols(path, name);
        CREATE INDEX IF NOT EXISTS idx_code_symbols_parent    ON code_symbols(parent_id);",
    )?;
    Ok(())
}

fn apply_migration_002(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "-- Task tracking
        CREATE TABLE IF NOT EXISTS tasks (
            id          INTEGER PRIMARY KEY,
            file_id     INTEGER NOT NULL,
            content     TEXT NOT NULL,
            completed   BOOLEAN DEFAULT 0,
            line_number INTEGER NOT NULL,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_file ON tasks(file_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed);

        -- Block reference anchors and callout type
        ALTER TABLE blocks ADD COLUMN block_ref_id TEXT;
        ALTER TABLE blocks ADD COLUMN callout_type TEXT;

        -- Link fragment
        ALTER TABLE links ADD COLUMN fragment TEXT;

        -- Partial index for block refs
        CREATE INDEX IF NOT EXISTS idx_blocks_ref ON blocks(block_ref_id) WHERE block_ref_id IS NOT NULL;",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    // ── 1. configure_pragmas_sets_wal_mode ──────────────────────────────────
    #[test]
    fn configure_pragmas_sets_wal_mode() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .unwrap();
        // In-memory DBs report "memory"; file DBs report "wal".
        assert!(
            mode == "wal" || mode == "memory",
            "unexpected journal_mode: {mode}"
        );
    }

    // ── 2. configure_pragmas_enables_foreign_keys ───────────────────────────
    #[test]
    fn configure_pragmas_enables_foreign_keys() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    // ── 3. migrate_creates_schema_version_table ─────────────────────────────
    #[test]
    fn migrate_creates_schema_version_table() {
        let conn = in_memory_db();
        let version = migrate(&conn).unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    // ── 4. migrate_creates_files_table ─────────────────────────────────────
    #[test]
    fn migrate_creates_files_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.md', 'markdown', 'abc123', 42, 0, 0);",
            [],
        )
        .unwrap();
    }

    // ── 5. migrate_creates_blocks_table ────────────────────────────────────
    #[test]
    fn migrate_creates_blocks_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.md', 'markdown', 'abc123', 42, 0, 0);",
            [],
        )
        .unwrap();
        let file_id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO blocks (file_id, block_type, content, start_line, end_line)
             VALUES (?1, 'heading', 'Hello', 1, 1);",
            rusqlite::params![file_id],
        )
        .unwrap();
    }

    // ── 6. migrate_creates_links_table ─────────────────────────────────────
    #[test]
    fn migrate_creates_links_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('src.md', 'markdown', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let src_id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO links (source_file_id, link_text, link_type)
             VALUES (?1, 'some link', 'wiki');",
            rusqlite::params![src_id],
        )
        .unwrap();
    }

    // ── 7. migrate_creates_tags_table ──────────────────────────────────────
    #[test]
    fn migrate_creates_tags_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('tagged.md', 'markdown', 'h2', 5, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO tags (name, file_id, source) VALUES ('rust', ?1, 'frontmatter');",
            rusqlite::params![fid],
        )
        .unwrap();
    }

    // ── 8. migrate_creates_properties_table ────────────────────────────────
    #[test]
    fn migrate_creates_properties_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('props.md', 'markdown', 'h3', 7, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (?1, 'title', 'Hello');",
            rusqlite::params![fid],
        )
        .unwrap();
    }

    // ── 9. migrate_enforces_unique_file_path ───────────────────────────────
    #[test]
    fn migrate_enforces_unique_file_path() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('dup.md', 'markdown', 'h4', 1, 0, 0);",
            [],
        )
        .unwrap();
        let result = conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('dup.md', 'markdown', 'h5', 1, 0, 0);",
            [],
        );
        assert!(result.is_err(), "duplicate path should fail");
    }

    // ── 10. migrate_enforces_unique_property_per_file ──────────────────────
    #[test]
    fn migrate_enforces_unique_property_per_file() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('uniq.md', 'markdown', 'h6', 2, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (?1, 'title', 'A');",
            rusqlite::params![fid],
        )
        .unwrap();
        let result = conn.execute(
            "INSERT INTO properties (file_id, key, value) VALUES (?1, 'title', 'B');",
            rusqlite::params![fid],
        );
        assert!(result.is_err(), "duplicate (file_id, key) should fail");
    }

    // ── 11. migrate_is_idempotent ──────────────────────────────────────────
    #[test]
    fn migrate_is_idempotent() {
        let conn = in_memory_db();
        let v1 = migrate(&conn).unwrap();
        let v2 = migrate(&conn).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v2, CURRENT_VERSION);
    }

    // ── 12. cascade_delete_removes_blocks ─────────────────────────────────
    #[test]
    fn cascade_delete_removes_blocks() {
        let conn = in_memory_db();
        // Foreign-key enforcement must be ON for CASCADE to fire.
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('cascade.md', 'markdown', 'h7', 3, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO blocks (file_id, block_type, content, start_line, end_line)
             VALUES (?1, 'paragraph', 'body', 1, 5);",
            rusqlite::params![fid],
        )
        .unwrap();

        // Sanity: block exists.
        let count_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM blocks WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_before, 1);

        conn.execute("DELETE FROM files WHERE id = ?1;", rusqlite::params![fid])
            .unwrap();

        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM blocks WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0, "cascaded delete should remove child blocks");
    }

    // ── 13. migrate_creates_fts5_table ────────────────────────────────────
    #[test]
    fn migrate_creates_fts5_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO fts_blocks (file_path, block_content, block_type)
             VALUES ('notes/hello.md', 'Hello world content', 'paragraph');",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_blocks WHERE fts_blocks MATCH 'hello';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "FTS5 MATCH query should return the inserted row");
    }

    // ── 14. migrate_v2_creates_tasks_table ────────────────────────────────
    #[test]
    fn migrate_v2_creates_tasks_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('tasks.md', 'markdown', 'h8', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO tasks (file_id, content, completed, line_number, created_at, updated_at)
             VALUES (?1, 'Buy groceries', 0, 5, 1000, 1000);",
            rusqlite::params![fid],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "tasks table should contain the inserted row");
    }

    // ── 15. migrate_v2_adds_block_ref_id_column ───────────────────────────
    #[test]
    fn migrate_v2_adds_block_ref_id_column() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('ref.md', 'markdown', 'h9', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO blocks (file_id, block_type, content, start_line, end_line, block_ref_id, callout_type)
             VALUES (?1, 'paragraph', 'Some block', 1, 2, 'anchor42', 'warning');",
            rusqlite::params![fid],
        )
        .unwrap();
        let bid: i64 = conn.last_insert_rowid();
        let (ref_id, callout): (String, String) = conn
            .query_row(
                "SELECT block_ref_id, callout_type FROM blocks WHERE id = ?1;",
                rusqlite::params![bid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ref_id, "anchor42");
        assert_eq!(callout, "warning");
    }

    // ── 16. migrate_v2_adds_link_fragment_column ──────────────────────────
    #[test]
    fn migrate_v2_adds_link_fragment_column() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('frag.md', 'markdown', 'h10', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO links (source_file_id, link_text, link_type, fragment)
             VALUES (?1, 'note#heading', 'wiki', 'heading');",
            rusqlite::params![fid],
        )
        .unwrap();
        let lid: i64 = conn.last_insert_rowid();
        let fragment: String = conn
            .query_row(
                "SELECT fragment FROM links WHERE id = ?1;",
                rusqlite::params![lid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fragment, "heading");
    }

    // ── 17. migrate_v4_creates_embeddings_table ────────────────────────────
    #[test]
    fn migrate_v4_creates_embeddings_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO embeddings (file_path, block_id, chunk_text, embedding, created_at)
             VALUES ('embed.md', 1, 'some chunk', X'00000000', unixepoch());",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "embeddings table should contain the inserted row");
    }

    // ── 18. migrate_v5_creates_jsx_components_table ────────────────────────
    #[test]
    fn migrate_v5_creates_jsx_components_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('comp.mdx', 'mdx', 'h11', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO jsx_components (file_id, name, props_json, line_number, self_closing, created_at)
             VALUES (?1, 'Chart', '{\"type\":\"bar\"}', 5, 0, 1000);",
            rusqlite::params![fid],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jsx_components WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "jsx_components table should contain the inserted row"
        );
    }

    // ── 19. migrate_v6_creates_canvas_nodes_table ──────────────────────────
    #[test]
    fn migrate_v6_creates_canvas_nodes_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.canvas', 'canvas', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO canvas_nodes (file_id, node_id, node_type, x, y, width, height, content_json)
             VALUES (?1, 'n1', 'text', 0.0, 0.0, 300.0, 200.0, '{\"text\":\"hello\"}');",
            rusqlite::params![fid],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canvas_nodes WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── 20. migrate_v6_creates_canvas_edges_table ──────────────────────────
    #[test]
    fn migrate_v6_creates_canvas_edges_table() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.canvas', 'canvas', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO canvas_edges (file_id, edge_id, from_node, to_node, edge_type)
             VALUES (?1, 'e1', 'n1', 'n2', 'solid');",
            rusqlite::params![fid],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canvas_edges WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── 21. migrate_v6_creates_bases_table ──────────────────────────────────
    #[test]
    fn migrate_v6_creates_bases_table() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO bases (path, name, schema_json, created_at, modified_at)
             VALUES ('tasks.bases', 'Tasks', '{}', 0, 0);",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bases;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── 22. migrate_v6_cascade_deletes_canvas_nodes ─────────────────────────
    #[test]
    fn migrate_v6_cascade_deletes_canvas_nodes() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('del.canvas', 'canvas', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO canvas_nodes (file_id, node_id, node_type, x, y, width, height)
             VALUES (?1, 'n1', 'text', 0.0, 0.0, 100.0, 100.0);",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO canvas_edges (file_id, edge_id, from_node, to_node, edge_type)
             VALUES (?1, 'e1', 'n1', 'n2', 'solid');",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute("DELETE FROM files WHERE id = ?1;", rusqlite::params![fid])
            .unwrap();
        let nodes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canvas_nodes WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        let edges: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canvas_edges WHERE file_id = ?1;",
                rusqlite::params![fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nodes, 0, "cascade should remove canvas nodes");
        assert_eq!(edges, 0, "cascade should remove canvas edges");
    }

    // ── 23. migrate_v6_cascade_deletes_bases_records ────────────────────────
    #[test]
    fn migrate_v6_cascade_deletes_bases_records() {
        let conn = in_memory_db();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO bases (path, name, schema_json, created_at, modified_at)
             VALUES ('del.bases', 'Del', '{}', 0, 0);",
            [],
        )
        .unwrap();
        let bid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO bases_records (base_id, record_id, data_json, created_at, modified_at)
             VALUES (?1, 'r1', '{}', 0, 0);",
            rusqlite::params![bid],
        )
        .unwrap();
        conn.execute("DELETE FROM bases WHERE id = ?1;", rusqlite::params![bid])
            .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bases_records WHERE base_id = ?1;",
                rusqlite::params![bid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "cascade should remove bases records");
    }

    // ── 24. migrate_v3_adds_typed_property_columns ───────────────────────────
    #[test]
    fn migrate_v3_adds_typed_property_columns() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('typed.md', 'markdown', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let fid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO properties (file_id, key, value, property_type, value_num, value_date, value_bool)
             VALUES (?1, 'count', '42', 'number', 42.0, NULL, NULL);",
            rusqlite::params![fid],
        )
        .unwrap();
    }
}
