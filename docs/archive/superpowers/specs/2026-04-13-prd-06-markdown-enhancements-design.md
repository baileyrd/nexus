# PRD 06: Markdown Enhancements Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** Block references, callouts, task extraction, alias-based link resolution
**Approach:** Hybrid (C) — unified AST walk with dedicated task module

---

## Overview

Extend the existing comrak-based markdown parser to handle four new features defined in PRD 06 and the Growth Plan. This is a targeted slice of PRD 06 focused on markdown parsing enhancements — Canvas, Bases, MDX, import/export, and attachment handling are deferred.

All detection happens in the existing single comrak AST walk in `parser.rs`. The task system gets its own module (`tasks.rs`) due to its DB table, query functions, and file-writeback logic. Block refs and callouts are lightweight field additions to existing structs.

---

## 1. Parser Extensions (`parser.rs`)

### 1.1 Block Reference Anchors

After extracting a block's content, check if it ends with ` ^some-id` (space + caret + alphanumeric/hyphen/underscore chars). If found:
- Strip the anchor from the block's `content` and `raw_markdown`
- Store the id in a new field

```rust
pub struct ParsedBlock {
    // ... existing fields ...
    pub block_ref_id: Option<String>,
}
```

Detection regex: `\s\^([a-zA-Z0-9_-]+)$` applied to the block content string after comrak extraction.

Only end-of-block anchors are valid. A `^id` in the middle of a paragraph is not a block reference.

### 1.2 Link Fragments

When extracting wikilinks, detect `#` in the target path. Split on first `#`:
- `[[note#Heading]]` → target_path=`"note"`, fragment=`Some("Heading")`
- `[[note#^blockid]]` → target_path=`"note"`, fragment=`Some("^blockid")`
- `[[note]]` → target_path=`"note"`, fragment=`None`

```rust
pub struct ParsedLink {
    // ... existing fields ...
    pub fragment: Option<String>,
}
```

### 1.3 Callout Detection

When visiting a `BlockQuote` node in the comrak AST, inspect the first line of text content for the pattern `[!TYPE]`:
- Regex: `^\[!([a-zA-Z]+)\]\s*(.*)`
- If matched: set `block_type = "callout"`, extract the type (lowercased), and the optional title
- The remaining lines of the blockquote become the callout body content

```rust
pub struct ParsedBlock {
    // ... existing fields ...
    pub callout_type: Option<String>,  // "warning", "tip", "note", "info", "danger", etc.
}
```

Supported types follow Obsidian conventions but are not restricted to a fixed set — any `[!TYPE]` is accepted and stored as-is (lowercased).

The title text (if present) becomes the first line of `content`. The remaining blockquote lines follow after a newline. Example: `> [!warning] Be careful\n> Details here` produces `content = "Be careful\nDetails here"`.

If a blockquote does not match the `[!TYPE]` pattern, it remains a regular `"blockquote"` block with `callout_type = None`.

### 1.4 Task Item Detection

When visiting list item nodes, detect GFM task list checkboxes:
- `- [ ] uncompleted task` → `ParsedTask { completed: false, ... }`
- `- [x] completed task` → `ParsedTask { completed: true, ... }`

Collected into `ParsedFile::tasks`:

```rust
pub struct ParsedFile {
    // ... existing fields ...
    pub tasks: Vec<ParsedTask>,
}
```

`ParsedTask` is defined in `tasks.rs` (see section 2).

---

## 2. Task Module (`crates/nexus-storage/src/tasks.rs`)

New file with three responsibilities: types, DB operations, and file writeback.

### 2.1 Types

```rust
pub struct ParsedTask {
    pub content: String,       // task text without the checkbox
    pub completed: bool,       // [ ] = false, [x] = true
    pub line_number: u32,      // 1-indexed, for file writeback
}

pub struct TaskRecord {
    pub id: u64,
    pub file_id: u64,
    pub file_path: String,     // denormalized via JOIN
    pub content: String,
    pub completed: bool,
    pub line_number: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct TaskFilter {
    pub completed: Option<bool>,   // None = all, Some(true) = done only
    pub file_path: Option<String>, // filter to specific file
}
```

### 2.2 DB Operations

- `insert_tasks(conn, file_id, tasks: &[ParsedTask])` — delete existing tasks for file_id, then bulk insert. This is a full-replace strategy: re-parsing a file replaces all its task records.
- `query_tasks(conn, filter: &TaskFilter) -> Result<Vec<TaskRecord>>` — filtered query with JOIN on files table for file_path.
- `toggle_task(conn, task_id: u64) -> Result<TaskRecord>` — flip `completed` in DB, update `updated_at`, return the updated record (caller needs file_path and line_number for writeback).

### 2.3 File Writeback

`toggle_task_in_file(file_path: &Path, line_number: u32, new_state: bool) -> Result<()>`

1. Read the file content
2. Split into lines, find line at `line_number` (1-indexed)
3. Verify the line contains a checkbox (`- [ ]` or `- [x]`). If not, return `StorageError::IndexInconsistency` (stale line number)
4. Replace `- [ ]` with `- [x]` (or vice versa)
5. Write back using `atomic_write`

The writeback triggers a re-parse via the normal `write_file` → `insert_file` path, which updates the DB to match the new file state. This keeps the DB and filesystem always in sync.

---

## 3. Schema Migration v2 (`schema.rs`)

Single migration from v1 to v2.

### 3.1 New Table

```sql
CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL,
    content     TEXT NOT NULL,
    completed   BOOLEAN DEFAULT 0,
    line_number INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);
CREATE INDEX idx_tasks_file ON tasks(file_id);
CREATE INDEX idx_tasks_completed ON tasks(completed);
```

### 3.2 New Columns

```sql
ALTER TABLE blocks ADD COLUMN block_ref_id TEXT;
CREATE INDEX idx_blocks_ref ON blocks(block_ref_id) WHERE block_ref_id IS NOT NULL;

ALTER TABLE blocks ADD COLUMN callout_type TEXT;

ALTER TABLE links ADD COLUMN fragment TEXT;
```

### 3.3 Migration Logic

Add `migrate_v1_to_v2(conn)` following the existing `migrate_v0_to_v1` pattern. The `migrate()` function applies it when `_schema_version` max is 1. After success, insert version 2 into `_schema_version`.

No data backfill — new columns are nullable. Existing rows pick up the new fields on next re-parse.

---

## 4. Index Extensions (`index.rs`)

### 4.1 `insert_file` Changes

Extend the existing function:
- Write `block_ref_id` and `callout_type` when inserting blocks
- Write `fragment` when inserting links
- After all other inserts, call `tasks::insert_tasks(conn, file_id, &parsed.tasks)`

### 4.2 New Query Functions

- `query_blocks_by_ref(conn, block_ref_id: &str) -> Result<Vec<BlockRecord>>` — find block(s) owning a `^blockid` anchor.
- `query_callout_blocks(conn, file_id: u64) -> Result<Vec<BlockRecord>>` — callout blocks for a file.

### 4.3 Updated Record Structs

```rust
pub struct BlockRecord {
    // ... existing fields ...
    pub block_ref_id: Option<String>,
    pub callout_type: Option<String>,
}

pub struct LinkRecord {
    // ... existing fields ...
    pub fragment: Option<String>,
}
```

### 4.4 Alias-Based Link Resolution

Extend the existing link resolution logic in `insert_file` with a third resolution step:

1. **Exact path match:** `[[folder/note]]` → `folder/note.md`
2. **Basename match:** `[[My Note]]` → file where stem = `My Note`
3. **Alias match (NEW):** query `properties` for `key = 'aliases'` with a LIKE pre-filter on the JSON value, then deserialize and exact-match in Rust

```sql
SELECT file_id FROM properties
WHERE key = 'aliases' AND value LIKE '%"link_target_text"%'
```

The LIKE is a pre-filter only — Rust code deserializes the JSON array and checks for exact string membership to avoid false positives.

No new table needed. Aliases are already stored as frontmatter properties during the existing `insert_file` flow.

---

## 5. CLI Commands

### 5.1 Task Subcommands

New subcommands under `nexus content`:

```
nexus content tasks                    — list all pending tasks
nexus content tasks --completed        — list completed tasks
nexus content tasks --all              — list all tasks
nexus content tasks --file <path>      — tasks in a specific file
nexus content tasks toggle <task-id>   — toggle completion (writes back to file)
```

Default text output format:
```
[ ] Fix the login bug          notes/todo.md:14
[ ] Write unit tests           notes/todo.md:15
[x] Set up CI pipeline         notes/project.md:8
```

JSON output via `--format json` returns `Vec<TaskRecord>`.

### 5.2 Enhanced `content read` Output

- Callout blocks render with type prefix: `[!warning] Critical issue`
- Block ref anchors display as `^blockid` suffix on the block
- Tasks show with checkbox state in the block listing

### 5.3 Implementation

Add `tasks` as a new variant to the content subcommand enum in the CLI arg parser. The `toggle` subcommand:
1. Calls `tasks::toggle_task(conn, task_id)` to get the record
2. Calls `tasks::toggle_task_in_file(path, line_number, new_state)` to update the file
3. The file change triggers re-parse via the storage engine
4. Prints confirmation with new state

---

## 6. Testing Strategy

### 6.1 Parser Unit Tests (`parser.rs`)

- Block ref anchor detection and content stripping
- Block ref in middle of paragraph (ignored)
- Wikilink with heading fragment
- Wikilink with block ref fragment
- Callout with type and title
- Callout with type only (no title)
- Regular blockquote (no callout pattern) unchanged
- Task checkbox detection with correct line numbers
- Combined test: single file with all features, verify full `ParsedFile`

### 6.2 Task Module Tests (`tasks.rs`)

- `insert_tasks` + `query_tasks` round-trip (in-memory SQLite)
- `query_tasks` with filter variants (completed, file-scoped, all)
- `toggle_task` flips DB state correctly
- `toggle_task_in_file`: write temp file, toggle, verify file content
- `toggle_task_in_file` on stale line (no checkbox) returns error

### 6.3 Schema Tests (`schema.rs`)

- v1 → v2 migration: new table and columns exist
- Fresh v0 → v2: both migrations apply in sequence
- Existing data preserved after migration

### 6.4 Integration Test (`tests/prd-06-smoke.rs`)

- Init forge, write file with all new features, verify index queries
- Toggle task, re-read file, verify checkbox changed on disk
- Alias resolution: file with `aliases: ["Alt Name"]`, second file with `[[Alt Name]]`, verify link resolves

### 6.5 No Benchmarks

Parse time targets from PRD 06 are noted but not benchmarked in this pass. The enhancements are lightweight additions to the existing comrak walk.

---

## Files Changed

| File | Change |
|------|--------|
| `crates/nexus-storage/src/parser.rs` | Add block_ref_id, fragment, callout_type, tasks detection |
| `crates/nexus-storage/src/tasks.rs` | **NEW** — ParsedTask, TaskRecord, TaskFilter, DB ops, file writeback |
| `crates/nexus-storage/src/schema.rs` | Add migrate_v1_to_v2 (tasks table, new columns) |
| `crates/nexus-storage/src/index.rs` | Extend insert_file, add query functions, update record structs |
| `crates/nexus-storage/src/lib.rs` | Expose tasks module, add task methods to StorageEngine |
| `crates/nexus-storage/src/error.rs` | No changes expected (existing variants cover new error cases) |
| `crates/nexus-cli/src/commands/content.rs` | Add tasks subcommand with toggle |
| `crates/nexus-cli/src/main.rs` | Wire tasks subcommand |
| `crates/nexus-storage/tests/prd-06-smoke.rs` | **NEW** — integration tests |

---

## Dependencies

No new crate dependencies. Everything uses existing comrak, rusqlite, serde_json, and regex-lite.

---

## Out of Scope

- MDX/JSX parsing (PRD 06 section 3)
- Canvas format (PRD 06 section 4)
- Bases format (PRD 06 section 5)
- Config format parsing (PRD 06 section 6)
- Import/export (PRD 06 section 10)
- Attachment handling (PRD 06 section 11)
- Format versioning headers in markdown files (PRD 06 section 9)
- TUI changes (backlinks panel, task view — deferred to Growth Plan Phase 1)
- Knowledge graph (Growth Plan Phase 1 — depends on this work but is separate)
