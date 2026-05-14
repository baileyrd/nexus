//! Reconciliation engine: synchronise the `SQLite` index with the filesystem.
//!
//! Walks `notes/` and `attachments/` under a forge root, compares file hashes
//! against the index, and emits a [`ReconcileDelta`] describing what changed.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use nexus_formats::sha256_hex;

use crate::StorageError;
use crate::code_index;
use crate::index::{FileFilter, FileRecord, delete_file, insert_file, query_files, soft_delete_file};
use crate::parser::parse_markdown;
use crate::watcher::should_ignore;

// ── Public types ──────────────────────────────────────────────────────────────

/// A summary of changes made by a single reconciliation pass.
#[derive(Debug, Clone, Default)]
pub struct ReconcileDelta {
    /// Files that were in the filesystem but not the index.
    pub created: usize,
    /// Files whose content changed since the last index.
    pub modified: usize,
    /// Files that were moved/renamed on disk.
    pub renamed: usize,
    /// Files that were removed from disk (now soft-deleted in the index).
    pub deleted: usize,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Reconcile the `SQLite` index against the filesystem.
///
/// Scans `notes/` and `attachments/` under `forge_root`, then brings the
/// index up to date:
///
/// * **created** – file on disk, not in index → parse and insert.
/// * **modified** – file on disk with a different hash → re-parse and
///   replace the old index entry.
/// * **renamed** – file on disk whose hash matches an index entry whose path
///   no longer exists on disk → update the `path` column in place.
/// * **deleted** – index entry whose path no longer exists on disk → soft-delete.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or `SQLite` failures.
pub fn reconcile(conn: &Connection, forge_root: &Path) -> Result<ReconcileDelta, StorageError> {
    let mut delta = ReconcileDelta::default();

    // ── 1. Scan filesystem ────────────────────────────────────────────────────
    let disk_files = scan_directory(forge_root)?;

    // ── 2. Get all index entries including soft-deleted ──────────────────────
    // Soft-deleted rows still occupy the `files.path` UNIQUE slot, so any
    // disk file at a previously-deleted path must resurrect the existing row
    // rather than INSERT a duplicate and blow the UNIQUE constraint.
    let index_files = query_files(
        conn,
        &FileFilter {
            include_deleted: true,
            ..FileFilter::default()
        },
    )?;

    // ── 3. Build lookup maps ──────────────────────────────────────────────────
    let mut index_by_path: HashMap<String, FileRecord> = HashMap::new();
    let mut index_by_hash: HashMap<String, Vec<FileRecord>> = HashMap::new();

    for record in &index_files {
        index_by_path.insert(record.path.clone(), record.clone());
        index_by_hash
            .entry(record.content_hash.clone())
            .or_default()
            .push(record.clone());
    }

    // Build a set of paths currently on disk for the deletion pass.
    let disk_paths: std::collections::HashSet<String> =
        disk_files.iter().map(|(p, _, _)| p.clone()).collect();

    // Track record IDs that were consumed as rename sources (skip soft-delete).
    let mut renamed_source_ids: std::collections::HashSet<u64> =
        std::collections::HashSet::new();

    // ── 4. Process each file on disk ─────────────────────────────────────────
    for (rel_path, hash, size_bytes) in &disk_files {
        if let Some(record) = index_by_path.get(rel_path) {
            if record.content_hash == *hash {
                if record.is_deleted {
                    // Path resurrected on disk with identical content —
                    // clear the soft-delete flag; child rows are still valid.
                    conn.execute(
                        "UPDATE files SET is_deleted = 0 WHERE id = ?1",
                        rusqlite::params![record.id.cast_signed()],
                    )?;
                    delta.created += 1;
                }
                continue;
            }

            // Hash differs → modified (or resurrected with new content).
            let abs_path = forge_root.join(rel_path);
            let Some(content) = read_utf8_or_skip(&abs_path, rel_path) else {
                continue;
            };
            let parsed = parse_markdown(&content)?;
            // Clean up orphaned FTS rows before hard-deleting the file row.
            conn.execute(
                "DELETE FROM fts_blocks WHERE file_path = ?1",
                rusqlite::params![rel_path],
            )?;
            delete_file(conn, record.id)?;
            let file_type = infer_file_type(rel_path);
            insert_file(conn, rel_path, &file_type, *size_bytes, &parsed)?;
            refresh_code_symbols(conn, rel_path, &content);
            if record.is_deleted {
                delta.created += 1;
            } else {
                delta.modified += 1;
            }
        } else {
            // Path not in index — check for rename by hash.
            let renamed = if let Some(candidates) = index_by_hash.get(hash) {
                // A rename candidate is a record whose path is NOT on disk
                // and hasn't already been claimed as a rename source.
                candidates
                    .iter()
                    .find(|r| {
                        !disk_paths.contains(&r.path)
                            && !renamed_source_ids.contains(&r.id)
                    })
                    .cloned()
            } else {
                None
            };

            if let Some(old_record) = renamed {
                // Rename: update path in-place (and clear any soft-delete
                // flag, in case we're reviving a deleted row at a new path),
                // also update fts_blocks file_path.
                conn.execute(
                    "UPDATE files SET path = ?1, is_deleted = 0 WHERE id = ?2",
                    rusqlite::params![rel_path, old_record.id.cast_signed()],
                )?;
                conn.execute(
                    "UPDATE fts_blocks SET file_path = ?1 WHERE file_path = ?2",
                    rusqlite::params![rel_path, &old_record.path],
                )?;
                // BL-114: re-key any code symbols under the new path so
                // queries by path still resolve after a rename.
                conn.execute(
                    "UPDATE code_symbols SET path = ?1 WHERE path = ?2",
                    rusqlite::params![rel_path, &old_record.path],
                )?;
                renamed_source_ids.insert(old_record.id);
                delta.renamed += 1;
            } else {
                // Brand-new file.
                let abs_path = forge_root.join(rel_path);
                let Some(content) = read_utf8_or_skip(&abs_path, rel_path) else {
                    continue;
                };
                let parsed = parse_markdown(&content)?;
                let file_type = infer_file_type(rel_path);
                insert_file(conn, rel_path, &file_type, *size_bytes, &parsed)?;
                refresh_code_symbols(conn, rel_path, &content);
                delta.created += 1;
            }
        }
    }

    // ── 5. Soft-delete index entries that are no longer on disk ──────────────
    // Already-deleted rows are skipped so `delta.deleted` only counts
    // transitions from live→deleted on this pass.
    for record in &index_files {
        if record.is_deleted {
            continue;
        }
        if !disk_paths.contains(&record.path) && !renamed_source_ids.contains(&record.id) {
            soft_delete_file(conn, record.id)?;
            // BL-114: code symbols are path-keyed (not files.id) so
            // soft-deleting the files row would orphan them — drop
            // them outright.
            let _ = code_index::delete_file_symbols(conn, &record.path);
            delta.deleted += 1;
        }
    }

    Ok(delta)
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// BL-114: extract code symbols for `rel_path` and upsert them.
/// No-op when the path's extension isn't in the code-language set.
/// Failures are logged and swallowed — a single parser hiccup must
/// not abort a multi-file reconcile.
fn refresh_code_symbols(conn: &Connection, rel_path: &str, content: &str) {
    let Some(lang) = code_index::detect_language(rel_path) else {
        // Drop any stale symbols left over from a previous code→non-code
        // rewrite where the extension changed.
        let _ = code_index::delete_file_symbols(conn, rel_path);
        return;
    };
    let symbols = code_index::extract_symbols(lang, content);
    if let Err(e) = code_index::upsert_file_symbols(conn, rel_path, lang, &symbols) {
        tracing::warn!(
            path = rel_path,
            error = %e,
            "BL-114: reconcile code-symbol upsert failed",
        );
    }
}

/// Scan all user-facing directories under `forge_root`.
///
/// Walks the entire forge root, skipping metadata directories (`.forge/`,
/// `.git/`, etc.) via [`should_ignore`]. Returns a list of
/// `(relative_path, content_hash, size_bytes)` tuples for every non-ignored
/// file found.
fn scan_directory(forge_root: &Path) -> Result<Vec<(String, String, u64)>, StorageError> {
    let mut results = Vec::new();
    scan_dir_recursive(forge_root, forge_root, &mut results)?;
    Ok(results)
}

/// Recursively walk `dir`, collecting non-ignored files.
fn scan_dir_recursive(
    dir: &Path,
    forge_root: &Path,
    results: &mut Vec<(String, String, u64)>,
) -> Result<(), StorageError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if should_ignore(&path) {
            continue;
        }

        // BL-082: classify against the symlink's own metadata (not
        // its target). `Path::is_dir` / `is_file` follow symlinks,
        // which would (a) double-index a file reachable through both
        // the symlink path and the target path, and (b) follow a
        // symlink out of the forge root if the user has a stray
        // symlink to a system folder. `entry.file_type()` is the
        // documented un-followed shape — `is_symlink()` true means
        // we skip the entry without recursing.
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            tracing::info!(
                path = %path.display(),
                "BL-082: skipping symlink during reconcile (not followed, not indexed)",
            );
            continue;
        }

        if file_type.is_dir() {
            scan_dir_recursive(&path, forge_root, results)?;
        } else if file_type.is_file() {
            let bytes = std::fs::read(&path)?;
            let hash = sha256_hex(&bytes);
            let size = entry.metadata()?.len();
            let rel = path
                .strip_prefix(forge_root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Normalise Windows separators to forward slashes.
            let rel = rel.replace('\\', "/");

            results.push((rel, hash, size));
        }
    }

    Ok(())
}

/// Read `abs_path` as UTF-8, returning `None` (with a warning) if the file
/// is not valid UTF-8 or the read fails for any other reason.
///
/// Reconcile walks every non-ignored file under the forge root, but not every
/// file in a user's folder is a text note — binary attachments, images, PDFs,
/// and random stray files are common. A single non-UTF-8 file must not brick
/// the entire kernel boot, which is what a `?` on `read_to_string` would do.
/// Caller skips the file (it stays on disk, just unindexed); next reconcile
/// pass will retry.
fn read_utf8_or_skip(abs_path: &Path, rel_path: &str) -> Option<String> {
    match std::fs::read_to_string(abs_path) {
        Ok(content) => Some(content),
        Err(err) => {
            tracing::warn!(
                path = %rel_path,
                error = %err,
                "skipping file during reconcile (not valid UTF-8 or unreadable)",
            );
            None
        }
    }
}

/// Infer a file type string from the relative path.
///
/// Returns `"attachment"` for paths starting with `"attachments/"`, and
/// `"note"` for everything else.
fn infer_file_type(path: &str) -> String {
    if path.starts_with("attachments/") {
        "attachment".to_string()
    } else {
        "note".to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();
        conn
    }

    /// Write a file to disk, creating parent directories as needed.
    fn write_file(root: &Path, rel_path: &str, content: &str) {
        let abs = root.join(rel_path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, content).unwrap();
    }

    // ── 1. reconcile_empty_forge_empty_index ──────────────────────────────────
    #[test]
    fn reconcile_empty_forge_empty_index() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        std::fs::create_dir_all(forge_root.join("attachments")).unwrap();

        let conn = setup_db();
        let delta = reconcile(&conn, forge_root).unwrap();

        assert_eq!(delta.created, 0);
        assert_eq!(delta.modified, 0);
        assert_eq!(delta.renamed, 0);
        assert_eq!(delta.deleted, 0);
    }

    // ── 2. reconcile_detects_new_files ────────────────────────────────────────
    #[test]
    fn reconcile_detects_new_files() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();

        write_file(forge_root, "notes/a.md", "# Alpha\n");
        write_file(forge_root, "notes/b.md", "# Beta\n");

        let conn = setup_db();
        let delta = reconcile(&conn, forge_root).unwrap();

        assert_eq!(delta.created, 2, "expected 2 created, got {}", delta.created);
        assert_eq!(delta.modified, 0);
        assert_eq!(delta.renamed, 0);
        assert_eq!(delta.deleted, 0);

        // Verify files are in the index.
        let files = query_files(&conn, &FileFilter::default()).unwrap();
        assert_eq!(files.len(), 2);
    }

    // ── 3. reconcile_detects_modified_files ───────────────────────────────────
    #[test]
    fn reconcile_detects_modified_files() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/note.md", "# Original\n");

        let conn = setup_db();
        let delta1 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta1.created, 1);

        // Modify the file.
        write_file(forge_root, "notes/note.md", "# Modified content\n");

        let delta2 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta2.created, 0);
        assert_eq!(delta2.modified, 1, "expected 1 modified, got {}", delta2.modified);
        assert_eq!(delta2.renamed, 0);
        assert_eq!(delta2.deleted, 0);
    }

    // ── 4. reconcile_detects_deleted_files ────────────────────────────────────
    #[test]
    fn reconcile_detects_deleted_files() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/gone.md", "# Will be deleted\n");

        let conn = setup_db();
        let delta1 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta1.created, 1);

        // Delete the file.
        std::fs::remove_file(forge_root.join("notes/gone.md")).unwrap();

        let delta2 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta2.created, 0);
        assert_eq!(delta2.modified, 0);
        assert_eq!(delta2.renamed, 0);
        assert_eq!(delta2.deleted, 1, "expected 1 deleted, got {}", delta2.deleted);

        // Verify it's soft-deleted in the index.
        let filter = FileFilter {
            include_deleted: true,
            ..Default::default()
        };
        let all_files = query_files(&conn, &filter).unwrap();
        assert_eq!(all_files.len(), 1);
        assert!(all_files[0].is_deleted);
    }

    // ── 5. reconcile_detects_renamed_files ────────────────────────────────────
    #[test]
    fn reconcile_detects_renamed_files() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/original.md", "# Rename me\n");

        let conn = setup_db();
        let delta1 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta1.created, 1);

        // Rename the file.
        std::fs::rename(
            forge_root.join("notes/original.md"),
            forge_root.join("notes/renamed.md"),
        )
        .unwrap();

        let delta2 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta2.created, 0, "expected 0 created, got {}", delta2.created);
        assert_eq!(delta2.modified, 0);
        assert_eq!(delta2.renamed, 1, "expected 1 renamed, got {}", delta2.renamed);
        assert_eq!(delta2.deleted, 0, "expected 0 deleted, got {}", delta2.deleted);

        // Verify new path is in the index.
        let record = crate::index::file_by_path(&conn, "notes/renamed.md").unwrap();
        assert!(record.is_some(), "notes/renamed.md should be in index");
    }

    // ── 6. reconcile_idempotent_no_changes ────────────────────────────────────
    #[test]
    fn reconcile_idempotent_no_changes() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/stable.md", "# Stable\n");

        let conn = setup_db();
        reconcile(&conn, forge_root).unwrap();

        // Second reconcile — nothing changed.
        let delta2 = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta2.created, 0);
        assert_eq!(delta2.modified, 0);
        assert_eq!(delta2.renamed, 0);
        assert_eq!(delta2.deleted, 0);
    }

    // ── 7. reconcile_handles_nested_directories ───────────────────────────────
    #[test]
    fn reconcile_handles_nested_directories() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes/sub/deep")).unwrap();
        write_file(forge_root, "notes/sub/deep/deep.md", "# Deep\n");

        let conn = setup_db();
        let delta = reconcile(&conn, forge_root).unwrap();

        assert_eq!(delta.created, 1, "expected 1 created, got {}", delta.created);

        let record = crate::index::file_by_path(&conn, "notes/sub/deep/deep.md").unwrap();
        assert!(record.is_some(), "notes/sub/deep/deep.md should be in index");
    }

    // ── 7b. reconcile_resurrects_soft_deleted_same_hash ───────────────────────
    #[test]
    fn reconcile_resurrects_soft_deleted_same_hash() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/come-back.md", "# Back\n");

        let conn = setup_db();
        reconcile(&conn, forge_root).unwrap();

        std::fs::remove_file(forge_root.join("notes/come-back.md")).unwrap();
        let delta_del = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta_del.deleted, 1);

        // Re-create with identical content — a soft-deleted row already
        // owns the path's UNIQUE slot, so a naive INSERT would blow up.
        write_file(forge_root, "notes/come-back.md", "# Back\n");
        let delta_res = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta_res.created, 1, "resurrection should count as created");
        assert_eq!(delta_res.modified, 0);
        assert_eq!(delta_res.deleted, 0);

        let record = crate::index::file_by_path(&conn, "notes/come-back.md")
            .unwrap()
            .expect("row should exist");
        assert!(!record.is_deleted, "is_deleted should be cleared");
    }

    // ── 7c. reconcile_resurrects_soft_deleted_new_hash ────────────────────────
    #[test]
    fn reconcile_resurrects_soft_deleted_new_hash() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        write_file(forge_root, "notes/revived.md", "# Original\n");

        let conn = setup_db();
        reconcile(&conn, forge_root).unwrap();
        std::fs::remove_file(forge_root.join("notes/revived.md")).unwrap();
        reconcile(&conn, forge_root).unwrap();

        // Recreate with different content — same UNIQUE path collision risk.
        write_file(forge_root, "notes/revived.md", "# Different\n");
        let delta = reconcile(&conn, forge_root).unwrap();
        assert_eq!(delta.created, 1);
        assert_eq!(delta.modified, 0);
        assert_eq!(delta.deleted, 0);

        let record = crate::index::file_by_path(&conn, "notes/revived.md")
            .unwrap()
            .expect("row should exist");
        assert!(!record.is_deleted);
    }

    // ── 8. scan_ignores_git_and_forge_temp ────────────────────────────────────
    #[test]
    fn scan_ignores_git_and_forge_temp() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        std::fs::create_dir_all(forge_root.join(".git")).unwrap();
        std::fs::create_dir_all(forge_root.join(".forge/temp")).unwrap();

        // This should be found.
        write_file(forge_root, "notes/visible.md", "# Visible\n");
        // These should be ignored.
        write_file(forge_root, ".git/COMMIT_EDITMSG", "commit msg");
        write_file(forge_root, ".forge/temp/scratch.md", "# Scratch\n");

        let results = scan_directory(forge_root).unwrap();
        assert_eq!(
            results.len(),
            1,
            "expected 1 result (visible.md), got {:?}",
            results.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
        );
        assert_eq!(results[0].0, "notes/visible.md");
    }

    // ── BL-082: symlinks must be skipped, not followed ───────────────────────

    /// Creating a symlink to a sibling file inside the forge should
    /// not produce a duplicate scan result for the symlink path. The
    /// target file is indexed exactly once via its real path.
    #[cfg(unix)]
    #[test]
    fn bl082_intra_forge_symlink_is_skipped() {
        let dir = TempDir::new().unwrap();
        let forge_root = dir.path();
        write_file(forge_root, "notes/real.md", "real content");
        std::os::unix::fs::symlink(
            forge_root.join("notes/real.md"),
            forge_root.join("notes/alias.md"),
        )
        .unwrap();

        let mut results = Vec::new();
        scan_dir_recursive(forge_root, forge_root, &mut results).unwrap();
        assert_eq!(
            results.len(),
            1,
            "expected the alias to be skipped; got {:?}",
            results.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
        );
        assert_eq!(results[0].0, "notes/real.md");
    }

    /// A symlink whose target is outside the forge root must not
    /// follow the link out of the sandbox during reconcile.
    #[cfg(unix)]
    #[test]
    fn bl082_external_symlink_is_skipped() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "off-limits").unwrap();
        let forge_root = dir.path();
        std::fs::create_dir_all(forge_root.join("notes")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            forge_root.join("notes/leak.md"),
        )
        .unwrap();

        let mut results = Vec::new();
        scan_dir_recursive(forge_root, forge_root, &mut results).unwrap();
        assert!(
            results.is_empty(),
            "external-target symlink must not surface in reconcile; got {:?}",
            results
        );
    }
}
