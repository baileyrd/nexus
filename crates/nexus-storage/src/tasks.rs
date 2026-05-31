//! Task extraction, storage, and file-writeback operations.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::StorageError;

// ── Parsed types ─────────────────────────────────────────────────────────────

/// A task item parsed from a markdown checkbox list.
#[derive(Debug, Clone)]
pub struct ParsedTask {
    /// Task text without the checkbox prefix.
    pub content: String,
    /// Whether the checkbox is checked (`[x]`).
    pub completed: bool,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
}

// ── DB record types ──────────────────────────────────────────────────────────

/// A task record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    /// Primary key.
    pub id: u64,
    /// FK into `files`.
    pub file_id: u64,
    /// Vault-relative path of the file containing this task.
    pub file_path: String,
    /// Task text without the checkbox prefix.
    pub content: String,
    /// Whether the task is completed.
    pub completed: bool,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
    /// Unix timestamp of first insert.
    pub created_at: i64,
    /// Unix timestamp of last modification.
    pub updated_at: i64,
}

/// Filter options for querying tasks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskFilter {
    /// Only return tasks with this completion state.
    pub completed: Option<bool>,
    /// Only return tasks from the file at this path.
    pub file_path: Option<String>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Delete existing tasks for `file_id`, then bulk-insert `tasks`.
///
/// Uses [`now_unix`] for `created_at` and `updated_at` timestamps.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn insert_tasks(
    conn: &Connection,
    file_id: u64,
    tasks: &[ParsedTask],
) -> Result<(), StorageError> {
    let now = now_unix();

    // Remove any previously stored tasks for this file.
    conn.execute(
        "DELETE FROM tasks WHERE file_id = ?1;",
        params![file_id.cast_signed()],
    )?;

    for task in tasks {
        conn.execute(
            "INSERT INTO tasks (file_id, content, completed, line_number, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5);",
            params![
                file_id.cast_signed(),
                task.content,
                task.completed,
                task.line_number,
                now,
            ],
        )?;
    }

    Ok(())
}

/// Return all tasks matching `filter`, ordered by file path then line number.
///
/// JOINs with the `files` table to populate `file_path` on each record.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_tasks(
    conn: &Connection,
    filter: &TaskFilter,
) -> Result<Vec<TaskRecord>, StorageError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(completed) = filter.completed {
        clauses.push(format!("t.completed = ?{}", param_values.len() + 1));
        param_values.push(Box::new(completed));
    }

    if let Some(file_path) = &filter.file_path {
        clauses.push(format!("f.path = ?{}", param_values.len() + 1));
        param_values.push(Box::new(file_path.clone()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT t.id, t.file_id, f.path, t.content, t.completed, t.line_number,
                t.created_at, t.updated_at
         FROM tasks t JOIN files f ON f.id = t.file_id
         {where_clause}
         ORDER BY f.path, t.line_number;"
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values
        .iter()
        .map(std::convert::AsRef::as_ref)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), map_task_record)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Toggle a task's completion state and return the updated record.
///
/// Flips `completed` from `true` to `false` (or vice-versa) and updates
/// the `updated_at` timestamp.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure, including
/// when no row matches `task_id`.
pub fn toggle_task(conn: &Connection, task_id: u64) -> Result<TaskRecord, StorageError> {
    let now = now_unix();

    conn.execute(
        "UPDATE tasks SET completed = NOT completed, updated_at = ?1 WHERE id = ?2;",
        params![now, task_id.cast_signed()],
    )?;

    let record = conn.query_row(
        "SELECT t.id, t.file_id, f.path, t.content, t.completed, t.line_number,
                t.created_at, t.updated_at
         FROM tasks t JOIN files f ON f.id = t.file_id
         WHERE t.id = ?1;",
        params![task_id.cast_signed()],
        map_task_record,
    )?;

    Ok(record)
}

/// Toggle a checkbox on disk at the given `line_number` (1-indexed).
///
/// When `new_state` is `true`, replaces `- [ ] ` with `- [x] `.
/// When `new_state` is `false`, replaces `- [x] ` with `- [ ] `.
///
/// # Errors
///
/// Returns [`StorageError::IndexInconsistency`] when the target line does
/// not contain the expected checkbox marker.
/// Returns [`StorageError::WriteFailed`] on I/O failure during read or write.
pub fn toggle_task_in_file(
    file_path: &Path,
    line_number: u32,
    new_state: bool,
) -> Result<(), StorageError> {
    let content = std::fs::read_to_string(file_path).map_err(|e| StorageError::WriteFailed {
        path: file_path.display().to_string(),
        reason: e.to_string(),
    })?;

    let lines: Vec<&str> = content.lines().collect();
    let idx = (line_number as usize).wrapping_sub(1);

    if idx >= lines.len() {
        return Err(StorageError::IndexInconsistency {
            details: format!(
                "line {line_number} out of range (file has {} lines)",
                lines.len()
            ),
        });
    }

    let line = lines[idx];

    let updated_line: String = if new_state {
        // Expect unchecked, replace with checked.
        if let Some(rest) = line.strip_prefix("- [ ] ") {
            format!("- [x] {rest}")
        } else {
            return Err(StorageError::IndexInconsistency {
                details: format!("expected '- [ ] ' at line {line_number}, found: {line}"),
            });
        }
    } else {
        // Expect checked, replace with unchecked.
        if let Some(rest) = line.strip_prefix("- [x] ") {
            format!("- [ ] {rest}")
        } else {
            return Err(StorageError::IndexInconsistency {
                details: format!("expected '- [x] ' at line {line_number}, found: {line}"),
            });
        }
    };

    // Allocate owned strings so we can swap in the updated line.
    let mut owned_lines: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    owned_lines[idx] = updated_line;

    let output = owned_lines.join("\n") + "\n";
    std::fs::write(file_path, output).map_err(|e| StorageError::WriteFailed {
        path: file_path.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Map a `SQLite` row to a [`TaskRecord`].
fn map_task_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        file_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        file_path: row.get(2)?,
        content: row.get(3)?,
        completed: row.get::<_, bool>(4)?,
        line_number: u32::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();
        conn
    }

    fn insert_test_file(conn: &Connection) -> u64 {
        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/tasks.md', 'markdown', 'abc', 100, 0, 0);",
            [],
        )
        .unwrap();
        u64::try_from(conn.last_insert_rowid()).unwrap_or(0)
    }

    // ── 1. insert_and_query_tasks ────────────────────────────────────────────

    #[test]
    fn insert_and_query_tasks() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);

        let tasks = vec![
            ParsedTask {
                content: "Buy groceries".to_string(),
                completed: false,
                line_number: 3,
            },
            ParsedTask {
                content: "Write tests".to_string(),
                completed: true,
                line_number: 4,
            },
        ];

        insert_tasks(&conn, file_id, &tasks).unwrap();

        let results = query_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(results.len(), 2, "expected 2 tasks, got {}", results.len());
    }

    // ── 2. query_tasks_filter_completed ──────────────────────────────────────

    #[test]
    fn query_tasks_filter_completed() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);

        let tasks = vec![
            ParsedTask {
                content: "Done task".to_string(),
                completed: true,
                line_number: 1,
            },
            ParsedTask {
                content: "Pending task".to_string(),
                completed: false,
                line_number: 2,
            },
        ];

        insert_tasks(&conn, file_id, &tasks).unwrap();

        // Filter for completed only.
        let done = query_tasks(
            &conn,
            &TaskFilter {
                completed: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            done.len(),
            1,
            "expected 1 completed task, got {}",
            done.len()
        );
        assert!(done[0].completed);

        // Filter for pending only.
        let pending = query_tasks(
            &conn,
            &TaskFilter {
                completed: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            pending.len(),
            1,
            "expected 1 pending task, got {}",
            pending.len()
        );
        assert!(!pending[0].completed);
    }

    // ── 3. toggle_task_flips_state ───────────────────────────────────────────

    #[test]
    fn toggle_task_flips_state() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);

        let tasks = vec![ParsedTask {
            content: "Toggleable".to_string(),
            completed: false,
            line_number: 1,
        }];

        insert_tasks(&conn, file_id, &tasks).unwrap();

        let all = query_tasks(&conn, &TaskFilter::default()).unwrap();
        let task_id = all[0].id;

        // Toggle: false -> true.
        let toggled = toggle_task(&conn, task_id).unwrap();
        assert!(toggled.completed, "expected completed after first toggle");

        // Toggle: true -> false.
        let toggled_back = toggle_task(&conn, task_id).unwrap();
        assert!(
            !toggled_back.completed,
            "expected uncompleted after second toggle"
        );
    }

    // ── 4. insert_tasks_replaces_existing ────────────────────────────────────

    #[test]
    fn insert_tasks_replaces_existing() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);

        // First insert: 1 task.
        let batch1 = vec![ParsedTask {
            content: "Original".to_string(),
            completed: false,
            line_number: 1,
        }];
        insert_tasks(&conn, file_id, &batch1).unwrap();

        let after_first = query_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(after_first.len(), 1);

        // Second insert: 2 tasks for the same file_id.
        let batch2 = vec![
            ParsedTask {
                content: "Replacement A".to_string(),
                completed: false,
                line_number: 1,
            },
            ParsedTask {
                content: "Replacement B".to_string(),
                completed: true,
                line_number: 2,
            },
        ];
        insert_tasks(&conn, file_id, &batch2).unwrap();

        let after_second = query_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(
            after_second.len(),
            2,
            "expected 2 tasks after replace, got {}",
            after_second.len()
        );
    }

    // ── 5. toggle_task_in_file_checks ────────────────────────────────────────

    #[test]
    fn toggle_task_in_file_checks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tasks.md");
        std::fs::write(&file, "# Tasks\n- [ ] Pending task\n").unwrap();

        toggle_task_in_file(&file, 2, true).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(
            content.contains("- [x] Pending task"),
            "expected checked task, got: {content}"
        );
    }

    // ── 6. toggle_task_in_file_unchecks ──────────────────────────────────────

    #[test]
    fn toggle_task_in_file_unchecks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tasks.md");
        std::fs::write(&file, "# Tasks\n- [x] Done task\n").unwrap();

        toggle_task_in_file(&file, 2, false).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(
            content.contains("- [ ] Done task"),
            "expected unchecked task, got: {content}"
        );
    }

    // ── 7. toggle_task_in_file_stale_line_errors ─────────────────────────────

    #[test]
    fn toggle_task_in_file_stale_line_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain.md");
        std::fs::write(&file, "Just a paragraph.\n").unwrap();

        let result = toggle_task_in_file(&file, 1, true);
        assert!(
            matches!(result, Err(StorageError::IndexInconsistency { .. })),
            "expected IndexInconsistency error, got: {result:?}"
        );
    }
}
