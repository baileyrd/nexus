# PRD 06 Markdown Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the Nexus parser, schema, and CLI to support block references, callouts, task extraction with file writeback, and alias-based link resolution.

**Architecture:** Extend the existing single-pass comrak AST walk in `parser.rs` with detection for block refs, callouts, and tasks. Add a dedicated `tasks.rs` module for task DB operations and file writeback. Run a v2 schema migration to add the `tasks` table and new columns on `blocks` and `links`.

**Tech Stack:** comrak (existing), rusqlite (existing), regex-lite (existing), clap (existing). No new dependencies.

---

## File Structure

| File | Role | Change |
|------|------|--------|
| `crates/nexus-storage/src/schema.rs` | Schema migrations | Add `migrate_v1_to_v2` |
| `crates/nexus-storage/src/parser.rs` | Markdown AST walk | Add block_ref_id, callout_type, fragment, tasks fields + detection |
| `crates/nexus-storage/src/tasks.rs` | Task DB ops + file writeback | **NEW** |
| `crates/nexus-storage/src/index.rs` | Index CRUD | Extend insert/query for new columns, alias resolution |
| `crates/nexus-storage/src/lib.rs` | Public API facade | Expose tasks module, add task methods to StorageEngine |
| `crates/nexus-cli/src/main.rs` | CLI arg parser | Add Tasks subcommand to ContentCommand |
| `crates/nexus-cli/src/commands/content.rs` | CLI handlers | Add tasks list + toggle handlers |
| `crates/nexus-storage/tests/prd-06-smoke.rs` | Integration tests | **NEW** |

---

### Task 1: Schema Migration v2

**Files:**
- Modify: `crates/nexus-storage/src/schema.rs`

- [ ] **Step 1: Write failing test for v2 migration — tasks table**

Add to the `tests` module in `schema.rs`:

```rust
#[test]
fn migrate_v2_creates_tasks_table() {
    let conn = in_memory_db();
    migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES ('task.md', 'markdown', 'h1', 10, 0, 0);",
        [],
    )
    .unwrap();
    let fid: i64 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO tasks (file_id, content, completed, line_number, created_at, updated_at)
         VALUES (?1, 'do something', 0, 5, 0, 0);",
        rusqlite::params![fid],
    )
    .unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexus-storage migrate_v2_creates_tasks_table`
Expected: FAIL — "no such table: tasks"

- [ ] **Step 3: Write failing test for v2 migration — new block columns**

```rust
#[test]
fn migrate_v2_adds_block_ref_id_column() {
    let conn = in_memory_db();
    migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES ('ref.md', 'markdown', 'h2', 10, 0, 0);",
        [],
    )
    .unwrap();
    let fid: i64 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO blocks (file_id, block_type, content, start_line, end_line, block_ref_id, callout_type)
         VALUES (?1, 'paragraph', 'hello', 1, 1, 'abc123', NULL);",
        rusqlite::params![fid],
    )
    .unwrap();
}
```

- [ ] **Step 4: Write failing test for v2 migration — link fragment column**

```rust
#[test]
fn migrate_v2_adds_link_fragment_column() {
    let conn = in_memory_db();
    migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES ('frag.md', 'markdown', 'h3', 10, 0, 0);",
        [],
    )
    .unwrap();
    let fid: i64 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO links (source_file_id, link_text, link_type, fragment)
         VALUES (?1, 'some link', 'wikilink', '#heading');",
        rusqlite::params![fid],
    )
    .unwrap();
}
```

- [ ] **Step 5: Run all three new tests to verify they fail**

Run: `cargo test -p nexus-storage migrate_v2`
Expected: All three FAIL

- [ ] **Step 6: Implement migration v2**

In `schema.rs`, update `CURRENT_VERSION` to 2 and add the migration:

```rust
pub const CURRENT_VERSION: u32 = 2;
```

Add `apply_migration_002` function after `apply_migration_001`:

```rust
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

        -- Block reference anchors
        ALTER TABLE blocks ADD COLUMN block_ref_id TEXT;
        ALTER TABLE blocks ADD COLUMN callout_type TEXT;

        -- Link fragment
        ALTER TABLE links ADD COLUMN fragment TEXT;",
    )?;

    // Partial index for block refs (SQLite supports this via execute)
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_blocks_ref ON blocks(block_ref_id) WHERE block_ref_id IS NOT NULL;",
    )?;

    Ok(())
}
```

In the `migrate` function, add after the `if current < 1` block:

```rust
if current < 2 {
    let tx = conn.unchecked_transaction()?;
    apply_migration_002(&tx)?;
    tx.execute(
        "INSERT INTO _schema_version (version, applied_at) VALUES (2, unixepoch());",
        [],
    )?;
    tx.commit()?;
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p nexus-storage migrate_v2`
Expected: All three PASS

- [ ] **Step 8: Verify existing tests still pass**

Run: `cargo test -p nexus-storage --lib`
Expected: All PASS (idempotent migration test will now reach v2)

- [ ] **Step 9: Commit**

```bash
git add crates/nexus-storage/src/schema.rs
git commit -m "feat(storage): add schema migration v2 — tasks table, block_ref_id, callout_type, link fragment"
```

---

### Task 2: Parser — Block References and Link Fragments

**Files:**
- Modify: `crates/nexus-storage/src/parser.rs`

- [ ] **Step 1: Write failing tests for block ref anchor detection**

Add to the `tests` module in `parser.rs`:

```rust
#[test]
fn parse_block_ref_anchor() {
    let pf = parse_markdown("Hello world ^abc123\n").unwrap();
    assert_eq!(pf.blocks.len(), 1);
    let b = &pf.blocks[0];
    assert_eq!(b.block_ref_id, Some("abc123".to_string()));
    assert_eq!(b.content, "Hello world");
}

#[test]
fn parse_block_ref_mid_paragraph_ignored() {
    let pf = parse_markdown("Hello ^mid world\n").unwrap();
    assert_eq!(pf.blocks[0].block_ref_id, None);
    assert!(pf.blocks[0].content.contains("^mid"));
}

#[test]
fn parse_heading_with_block_ref() {
    let pf = parse_markdown("## Section Title ^sec1\n").unwrap();
    let b = &pf.blocks[0];
    assert_eq!(b.block_ref_id, Some("sec1".to_string()));
    assert_eq!(b.content, "Section Title");
}
```

- [ ] **Step 2: Write failing tests for link fragments**

```rust
#[test]
fn parse_wikilink_with_heading_fragment() {
    let pf = parse_markdown("See [[note#Heading]]\n").unwrap();
    let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(wl.target_path, Some("note".to_string()));
    assert_eq!(wl.fragment, Some("Heading".to_string()));
}

#[test]
fn parse_wikilink_with_block_ref_fragment() {
    let pf = parse_markdown("See [[note#^ref1]]\n").unwrap();
    let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(wl.target_path, Some("note".to_string()));
    assert_eq!(wl.fragment, Some("^ref1".to_string()));
}

#[test]
fn parse_wikilink_no_fragment() {
    let pf = parse_markdown("See [[note]]\n").unwrap();
    let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(wl.fragment, None);
}

#[test]
fn parse_wikilink_display_text_with_fragment() {
    let pf = parse_markdown("See [[note#Heading|display text]]\n").unwrap();
    let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(wl.target_path, Some("note".to_string()));
    assert_eq!(wl.fragment, Some("Heading".to_string()));
    assert_eq!(wl.link_text, "display text");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p nexus-storage parse_block_ref parse_wikilink_with`
Expected: FAIL — `block_ref_id` and `fragment` fields don't exist

- [ ] **Step 4: Add new fields to ParsedBlock and ParsedLink**

In `parser.rs`, update the structs:

```rust
pub struct ParsedBlock {
    /// Kind of block: "heading", "paragraph", "codeblock", "list", "table", "callout".
    pub block_type: String,
    /// Heading level 1-6; `None` for non-headings.
    pub level: Option<i32>,
    /// Plain-text content.
    pub content: String,
    /// Raw markdown source (currently not populated).
    pub raw_markdown: Option<String>,
    /// 1-based start line in the source.
    pub start_line: u32,
    /// 1-based end line in the source.
    pub end_line: u32,
    /// Block reference anchor id (e.g. "abc123" from `^abc123`).
    pub block_ref_id: Option<String>,
    /// Callout type (e.g. "warning", "tip") for callout blocks.
    pub callout_type: Option<String>,
}

pub struct ParsedLink {
    /// Display text for the link.
    pub link_text: String,
    /// Target path or URL, if available.
    pub target_path: Option<String>,
    /// Kind of link: "wikilink", "markdown", or "embed".
    pub link_type: String,
    /// Fragment identifier (e.g. "Heading" or "^blockid" from `[[note#Heading]]`).
    pub fragment: Option<String>,
}
```

- [ ] **Step 5: Fix all compilation errors**

Every place that constructs a `ParsedBlock` needs `block_ref_id: None, callout_type: None`. Every `ParsedLink` needs `fragment: None`. Fix in:
- `parse_markdown` function: all 5 `ParsedBlock` push sites and all `ParsedLink` push sites in `extract_wikilinks_and_embeds` and `extract_markdown_links`
- Test helper `sample_parsed_file` in `index.rs`

- [ ] **Step 6: Implement block ref anchor detection**

Add a helper function in `parser.rs`:

```rust
/// Extract a trailing block reference anchor (` ^id`) from text.
/// Returns `(cleaned_content, Option<ref_id>)`.
fn extract_block_ref(content: &str) -> (String, Option<String>) {
    // Match: space + caret + one or more [a-zA-Z0-9_-] at end of string
    if let Some(pos) = content.rfind(" ^") {
        let candidate = &content[pos + 2..];
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return (content[..pos].to_string(), Some(candidate.to_string()));
        }
    }
    (content.to_string(), None)
}
```

In each block-creation site in `parse_markdown` (Heading, Paragraph, List), after `collect_text(child)`, call `extract_block_ref`:

```rust
let raw_text = collect_text(child);
let (text, block_ref_id) = extract_block_ref(&raw_text);
```

Then use `text` for content/links/tags extraction and `block_ref_id` in the `ParsedBlock`.

- [ ] **Step 7: Implement link fragment extraction**

In `extract_wikilinks_and_embeds`, after extracting `inner` (the text between `[[ ]]`), split on `#` before checking for `|`:

```rust
// After getting `inner` from between [[ and ]]:
if is_embed {
    let (target, fragment) = split_fragment(inner);
    links.push(ParsedLink {
        link_text: inner.to_string(),
        target_path: Some(target),
        link_type: "embed".to_string(),
        fragment,
    });
} else if let Some(pipe) = inner.find('|') {
    let before_pipe = &inner[..pipe];
    let display = inner[pipe + 1..].to_string();
    let (target, fragment) = split_fragment(before_pipe);
    links.push(ParsedLink {
        link_text: display,
        target_path: Some(target),
        link_type: "wikilink".to_string(),
        fragment,
    });
} else {
    let (target, fragment) = split_fragment(inner);
    links.push(ParsedLink {
        link_text: inner.to_string(),
        target_path: if target.is_empty() { None } else { Some(target) },
        link_type: "wikilink".to_string(),
        fragment,
    });
}
```

Add the helper:

```rust
/// Split a link target on `#` into (path, optional fragment).
fn split_fragment(target: &str) -> (String, Option<String>) {
    if let Some(hash_pos) = target.find('#') {
        let path = target[..hash_pos].to_string();
        let frag = target[hash_pos + 1..].to_string();
        (path, if frag.is_empty() { None } else { Some(frag) })
    } else {
        (target.to_string(), None)
    }
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib`
Expected: All PASS (new + existing)

- [ ] **Step 9: Commit**

```bash
git add crates/nexus-storage/src/parser.rs crates/nexus-storage/src/index.rs
git commit -m "feat(storage): add block reference anchors and link fragment parsing"
```

---

### Task 3: Parser — Callout Detection

**Files:**
- Modify: `crates/nexus-storage/src/parser.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn parse_callout_with_title() {
    let md = "> [!warning] Be careful\n> This is dangerous\n";
    let pf = parse_markdown(md).unwrap();
    let callout = pf.blocks.iter().find(|b| b.block_type == "callout");
    assert!(callout.is_some(), "no callout block found");
    let c = callout.unwrap();
    assert_eq!(c.callout_type, Some("warning".to_string()));
    assert_eq!(c.content, "Be careful\nThis is dangerous");
}

#[test]
fn parse_callout_no_title() {
    let md = "> [!tip]\n> Just a tip\n";
    let pf = parse_markdown(md).unwrap();
    let callout = pf.blocks.iter().find(|b| b.block_type == "callout");
    assert!(callout.is_some(), "no callout block found");
    let c = callout.unwrap();
    assert_eq!(c.callout_type, Some("tip".to_string()));
    assert_eq!(c.content, "Just a tip");
}

#[test]
fn parse_regular_blockquote_not_callout() {
    let md = "> Just a regular quote\n";
    let pf = parse_markdown(md).unwrap();
    let blocks: Vec<_> = pf.blocks.iter().filter(|b| b.block_type == "callout").collect();
    assert!(blocks.is_empty(), "regular blockquote should not be a callout");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexus-storage parse_callout parse_regular_blockquote`
Expected: FAIL — no callout detection yet

- [ ] **Step 3: Implement callout detection**

In `parse_markdown`, add a new match arm for `NodeValue::BlockQuote` before the `_ => {}` catch-all:

```rust
NodeValue::BlockQuote => {
    let raw_text = collect_text(child);
    let (block_type, callout_type, content) = detect_callout(&raw_text);
    let (content, block_ref_id) = extract_block_ref(&content);
    extract_wikilinks_and_embeds(&content, &mut links);
    extract_inline_tags(&content, &mut tags);
    blocks.push(ParsedBlock {
        block_type,
        level: None,
        content,
        raw_markdown: None,
        start_line,
        end_line,
        block_ref_id,
        callout_type,
    });
}
```

Add the helper function:

```rust
/// Detect if text is a callout (`[!TYPE] optional title`) or a regular blockquote.
/// Returns `(block_type, callout_type, content)`.
fn detect_callout(text: &str) -> (String, Option<String>, String) {
    // Pattern: [!TYPE] at the start, optionally followed by title text
    let trimmed = text.trim_start();
    if trimmed.starts_with("[!") {
        if let Some(close) = trimmed.find(']') {
            let callout_type = trimmed[2..close].to_lowercase();
            if !callout_type.is_empty()
                && callout_type.chars().all(|c| c.is_ascii_alphabetic())
            {
                let after_type = trimmed[close + 1..].trim_start();
                let content = if after_type.is_empty() {
                    String::new()
                } else {
                    after_type.to_string()
                };
                return (
                    "callout".to_string(),
                    Some(callout_type),
                    content,
                );
            }
        }
    }
    ("blockquote".to_string(), None, text.to_string())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/parser.rs
git commit -m "feat(storage): detect callout blocks from blockquote [!TYPE] syntax"
```

---

### Task 4: Parser — Task Extraction

**Files:**
- Modify: `crates/nexus-storage/src/parser.rs`
- Create: `crates/nexus-storage/src/tasks.rs`

- [ ] **Step 1: Create `tasks.rs` with ParsedTask type**

Create `crates/nexus-storage/src/tasks.rs`:

```rust
//! Task extraction, storage, and file-writeback operations.

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
```

- [ ] **Step 2: Add tasks field to ParsedFile and register the module**

In `parser.rs`, add the import and field:

```rust
use crate::tasks::ParsedTask;
```

Add to `ParsedFile`:

```rust
pub struct ParsedFile {
    pub content_hash: String,
    pub frontmatter: Vec<Property>,
    pub blocks: Vec<ParsedBlock>,
    pub links: Vec<ParsedLink>,
    pub tags: Vec<ParsedTag>,
    /// Task items extracted from checkbox lists.
    pub tasks: Vec<ParsedTask>,
}
```

In `lib.rs`, add the module declaration:

```rust
mod tasks;
```

And add to the pub use line:

```rust
pub use tasks::ParsedTask;
```

Initialize `tasks: Vec::new()` in `parse_markdown` and update the `Ok(ParsedFile { ... })` return.

- [ ] **Step 3: Write failing tests for task extraction**

In `parser.rs` tests:

```rust
#[test]
fn parse_tasks_from_list() {
    let md = "- [ ] Buy groceries\n- [x] Write tests\n- [ ] Deploy app\n";
    let pf = parse_markdown(md).unwrap();
    assert_eq!(pf.tasks.len(), 3);
    assert!(!pf.tasks[0].completed);
    assert_eq!(pf.tasks[0].content, "Buy groceries");
    assert!(pf.tasks[1].completed);
    assert_eq!(pf.tasks[1].content, "Write tests");
    assert!(!pf.tasks[2].completed);
}

#[test]
fn parse_no_tasks_in_regular_list() {
    let md = "- item one\n- item two\n";
    let pf = parse_markdown(md).unwrap();
    assert!(pf.tasks.is_empty());
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p nexus-storage parse_tasks parse_no_tasks`
Expected: FAIL — tasks vec is always empty

- [ ] **Step 5: Implement task extraction in the AST walk**

In `parse_markdown`, add a helper to extract tasks from list items. The comrak AST with `tasklist` extension marks list items with a `TaskItem` node. Add task extraction inside the `NodeValue::List(_)` arm:

```rust
NodeValue::List(_) => {
    let raw_text = collect_text(child);
    let (text, block_ref_id) = extract_block_ref(&raw_text);
    extract_wikilinks_and_embeds(&text, &mut links);
    extract_markdown_links(child, &mut links);
    extract_inline_tags(&text, &mut tags);
    // Extract tasks from list items
    extract_tasks(child, &mut tasks);
    blocks.push(ParsedBlock {
        block_type: "list".to_string(),
        level: None,
        content: text,
        raw_markdown: None,
        start_line,
        end_line,
        block_ref_id,
        callout_type: None,
    });
}
```

Add the `extract_tasks` function:

```rust
/// Walk list item children to find task checkboxes.
fn extract_tasks<'a>(list_node: &'a AstNode<'a>, tasks: &mut Vec<ParsedTask>) {
    for item in list_node.children() {
        let ast = item.data.borrow();
        if let NodeValue::TaskItem(nti) = &ast.value {
            let text = collect_text(item).trim().to_string();
            let line = u32::try_from(ast.sourcepos.start.line).unwrap_or(0);
            tasks.push(ParsedTask {
                content: text,
                completed: nti.symbol.is_some(), // Some('x') = checked, None = unchecked
                line_number: line,
            });
        }
    }
}
```

Note: comrak 0.52's `TaskItem` wraps `NodeTaskItem { symbol: Option<char> }` — `Some('x')` means checked, `None` means unchecked.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add crates/nexus-storage/src/tasks.rs crates/nexus-storage/src/parser.rs crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): extract task items from markdown checkbox lists"
```

---

### Task 5: Task Module — DB Operations

**Files:**
- Modify: `crates/nexus-storage/src/tasks.rs`
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Write failing tests for task DB operations**

In `tasks.rs`, add:

```rust
use rusqlite::{Connection, params};

use crate::StorageError;

/// A task record from the database.
#[derive(Debug, Clone)]
pub struct TaskRecord {
    /// Primary key.
    pub id: u64,
    /// FK into files table.
    pub file_id: u64,
    /// Vault-relative file path (denormalized via JOIN).
    pub file_path: String,
    /// Task text.
    pub content: String,
    /// Whether the task is completed.
    pub completed: bool,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Unix timestamp of last update.
    pub updated_at: i64,
}

/// Filter options for querying tasks.
#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    /// Filter by completion state. `None` = all tasks.
    pub completed: Option<bool>,
    /// Filter to tasks in a specific file path.
    pub file_path: Option<String>,
}

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

    #[test]
    fn insert_and_query_tasks() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);
        let tasks = vec![
            ParsedTask { content: "Buy milk".to_string(), completed: false, line_number: 3 },
            ParsedTask { content: "Write code".to_string(), completed: true, line_number: 4 },
        ];
        insert_tasks(&conn, file_id, &tasks).unwrap();
        let results = query_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_tasks_filter_completed() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);
        let tasks = vec![
            ParsedTask { content: "done".to_string(), completed: true, line_number: 1 },
            ParsedTask { content: "pending".to_string(), completed: false, line_number: 2 },
        ];
        insert_tasks(&conn, file_id, &tasks).unwrap();

        let done = query_tasks(&conn, &TaskFilter { completed: Some(true), ..Default::default() }).unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].content, "done");

        let pending = query_tasks(&conn, &TaskFilter { completed: Some(false), ..Default::default() }).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content, "pending");
    }

    #[test]
    fn toggle_task_flips_state() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);
        let tasks = vec![
            ParsedTask { content: "flip me".to_string(), completed: false, line_number: 1 },
        ];
        insert_tasks(&conn, file_id, &tasks).unwrap();
        let all = query_tasks(&conn, &TaskFilter::default()).unwrap();
        let record = toggle_task(&conn, all[0].id).unwrap();
        assert!(record.completed);

        // Toggle back
        let record2 = toggle_task(&conn, all[0].id).unwrap();
        assert!(!record2.completed);
    }

    #[test]
    fn insert_tasks_replaces_existing() {
        let conn = setup_db();
        let file_id = insert_test_file(&conn);
        let tasks1 = vec![
            ParsedTask { content: "old task".to_string(), completed: false, line_number: 1 },
        ];
        insert_tasks(&conn, file_id, &tasks1).unwrap();
        let tasks2 = vec![
            ParsedTask { content: "new task a".to_string(), completed: false, line_number: 1 },
            ParsedTask { content: "new task b".to_string(), completed: true, line_number: 2 },
        ];
        insert_tasks(&conn, file_id, &tasks2).unwrap();
        let all = query_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|t| t.content.starts_with("new")));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexus-storage --lib tasks`
Expected: FAIL — functions don't exist yet

- [ ] **Step 3: Implement DB operations**

Add to `tasks.rs`:

```rust
/// Insert parsed tasks for a file, replacing any existing tasks for that file.
pub fn insert_tasks(
    conn: &Connection,
    file_id: u64,
    tasks: &[ParsedTask],
) -> Result<(), StorageError> {
    // Delete existing tasks for this file (full replace on re-parse).
    conn.execute(
        "DELETE FROM tasks WHERE file_id = ?1;",
        params![file_id.cast_signed()],
    )?;

    let now = now_unix();
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

/// Query tasks with optional filters.
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

    if let Some(ref path) = filter.file_path {
        clauses.push(format!("f.path = ?{}", param_values.len() + 1));
        param_values.push(Box::new(path.clone()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT t.id, t.file_id, f.path, t.content, t.completed, t.line_number, t.created_at, t.updated_at
         FROM tasks t JOIN files f ON f.id = t.file_id
         {where_clause} ORDER BY f.path, t.line_number;"
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(std::convert::AsRef::as_ref).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
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
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Toggle a task's completion state and return the updated record.
pub fn toggle_task(conn: &Connection, task_id: u64) -> Result<TaskRecord, StorageError> {
    let now = now_unix();
    conn.execute(
        "UPDATE tasks SET completed = NOT completed, updated_at = ?1 WHERE id = ?2;",
        params![now, task_id.cast_signed()],
    )?;

    conn.query_row(
        "SELECT t.id, t.file_id, f.path, t.content, t.completed, t.line_number, t.created_at, t.updated_at
         FROM tasks t JOIN files f ON f.id = t.file_id
         WHERE t.id = ?1;",
        params![task_id.cast_signed()],
        |row| {
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
        },
    )
    .map_err(StorageError::Database)
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib tasks`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/tasks.rs crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): add task DB operations — insert, query, toggle"
```

---

### Task 6: Task Module — File Writeback

**Files:**
- Modify: `crates/nexus-storage/src/tasks.rs`

- [ ] **Step 1: Write failing tests**

Add to `tasks.rs` tests:

```rust
#[test]
fn toggle_task_in_file_unchecks() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, "- [x] Done task\n- [ ] Pending\n").unwrap();

    toggle_task_in_file(&file_path, 1, false).unwrap();
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("- [ ] Done task"), "task should be unchecked, got: {content}");
}

#[test]
fn toggle_task_in_file_checks() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, "- [ ] Pending task\n").unwrap();

    toggle_task_in_file(&file_path, 1, true).unwrap();
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("- [x] Pending task"), "task should be checked, got: {content}");
}

#[test]
fn toggle_task_in_file_stale_line_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, "Just a paragraph\n").unwrap();

    let result = toggle_task_in_file(&file_path, 1, true);
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexus-storage toggle_task_in_file`
Expected: FAIL — function doesn't exist

- [ ] **Step 3: Implement file writeback**

Add to `tasks.rs`:

```rust
use std::path::Path;

/// Toggle a task checkbox in a markdown file by line number.
///
/// Reads the file, finds the specified line, replaces `- [ ]` with `- [x]`
/// (or vice versa), and writes back atomically.
///
/// # Errors
///
/// Returns `StorageError::IndexInconsistency` if the line does not contain
/// a checkbox, or I/O errors on read/write failure.
pub fn toggle_task_in_file(
    file_path: &Path,
    line_number: u32,
    new_state: bool,
) -> Result<(), StorageError> {
    let content = std::fs::read_to_string(file_path).map_err(|e| {
        StorageError::WriteFailed {
            path: file_path.display().to_string(),
            reason: e.to_string(),
        }
    })?;

    let mut lines: Vec<&str> = content.lines().collect();
    let idx = (line_number as usize).saturating_sub(1); // 1-indexed to 0-indexed

    if idx >= lines.len() {
        return Err(StorageError::IndexInconsistency {
            details: format!(
                "line {} out of range (file has {} lines)",
                line_number,
                lines.len()
            ),
        });
    }

    let line = lines[idx];
    let new_line;

    if new_state {
        // Check: - [ ] → - [x]
        if let Some(rest) = line.strip_prefix("- [ ] ") {
            new_line = format!("- [x] {rest}");
        } else {
            return Err(StorageError::IndexInconsistency {
                details: format!("line {} does not contain '- [ ]': {}", line_number, line),
            });
        }
    } else {
        // Uncheck: - [x] → - [ ]
        if let Some(rest) = line.strip_prefix("- [x] ") {
            new_line = format!("- [ ] {rest}");
        } else {
            return Err(StorageError::IndexInconsistency {
                details: format!("line {} does not contain '- [x]': {}", line_number, line),
            });
        }
    }

    lines[idx] = &new_line;
    let result = lines.join("\n") + "\n";

    std::fs::write(file_path, result).map_err(|e| StorageError::WriteFailed {
        path: file_path.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage toggle_task_in_file`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/tasks.rs
git commit -m "feat(storage): add task file writeback — toggle checkboxes on disk"
```

---

### Task 7: Index — Wire New Fields and Alias Resolution

**Files:**
- Modify: `crates/nexus-storage/src/index.rs`

- [ ] **Step 1: Update BlockRecord and LinkRecord structs**

Add to `BlockRecord`:

```rust
pub struct BlockRecord {
    // ... existing fields ...
    /// Block reference anchor id.
    pub block_ref_id: Option<String>,
    /// Callout type for callout blocks.
    pub callout_type: Option<String>,
}
```

Add to `LinkRecord`:

```rust
pub struct LinkRecord {
    // ... existing fields ...
    /// Fragment identifier from the link target.
    pub fragment: Option<String>,
}
```

- [ ] **Step 2: Update insert_file to write new columns**

In `insert_file`, update the blocks INSERT to include `block_ref_id` and `callout_type`:

```rust
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
```

Update the links INSERT to include `fragment`:

```rust
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
```

After properties, add task insertion:

```rust
// ── 6. Tasks ────────────────────────────────────────────────────────────
crate::tasks::insert_tasks(conn, file_id, &parsed.tasks)?;
```

- [ ] **Step 3: Update query_blocks to read new columns**

```rust
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
    // ... collect results ...
```

- [ ] **Step 4: Update query_links and query_backlinks to read fragment**

Update both functions to SELECT and map `fragment`:

```rust
"SELECT id, source_file_id, target_path, target_file_id, link_text, link_type, is_resolved, fragment
 FROM links WHERE ..."
```

Update `map_link_record`:

```rust
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
```

- [ ] **Step 5: Add alias resolution to resolve_link**

In `resolve_link`, add a third resolution step after the basename match:

```rust
// Alias match: check if any file has this as an alias in frontmatter properties.
let alias_result = conn.query_row(
    "SELECT file_id, value FROM properties WHERE key = 'aliases' AND value LIKE ?1;",
    params![format!("%\"{target}\"%")],
    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
);

if let Ok((fid, json_value)) = alias_result {
    // Verify exact match by deserializing the JSON array.
    if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&json_value) {
        if aliases.iter().any(|a| a == target) {
            return (Some(u64::try_from(fid).unwrap_or(0)), true);
        }
    }
}
```

- [ ] **Step 6: Update sample_parsed_file in test helper**

In the `tests` module, update `sample_parsed_file` to include new fields:

```rust
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
    // ... same for other blocks ...
],
links: vec![ParsedLink {
    link_text: "other note".to_string(),
    target_path: Some("other-note".to_string()),
    link_type: "wikilink".to_string(),
    fragment: None,
}],
// ... add tasks field:
tasks: vec![],
```

- [ ] **Step 7: Run all tests**

Run: `cargo test -p nexus-storage --lib`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add crates/nexus-storage/src/index.rs
git commit -m "feat(storage): wire block_ref_id, callout_type, fragment, tasks into index + alias resolution"
```

---

### Task 8: StorageEngine — Expose Task API

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Add public exports**

In `lib.rs`, add to the existing pub use lines:

```rust
pub use tasks::{ParsedTask, TaskRecord, TaskFilter, insert_tasks, query_tasks, toggle_task, toggle_task_in_file};
```

Change `mod tasks;` to `pub(crate) mod tasks;` if needed for the pub use to work, or keep it private and re-export the specific items.

- [ ] **Step 2: Add task methods to StorageEngine**

Add to the `impl StorageEngine` block:

```rust
// ── Tasks ────────────────────────────────────────────────────────────────

/// Query tasks with optional filters.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_tasks(&self, filter: &TaskFilter) -> Result<Vec<TaskRecord>, StorageError> {
    let conn = self.pool.get().map_err(|e| StorageError::Database(
        rusqlite::Error::InvalidParameterName(e.to_string()),
    ))?;
    tasks::query_tasks(&conn, filter)
}

/// Toggle a task's completion state in both the database and the source file.
///
/// # Errors
///
/// Returns [`StorageError`] on database or I/O failure.
///
/// # Panics
///
/// Panics if the internal write-connection mutex is poisoned.
pub fn toggle_task(&self, task_id: u64) -> Result<TaskRecord, StorageError> {
    let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
    let record = tasks::toggle_task(&conn, task_id)?;

    // Write back to file
    let abs_path = self.forge.root().join(&record.file_path);
    tasks::toggle_task_in_file(&abs_path, record.line_number, record.completed)?;

    Ok(record)
}
```

- [ ] **Step 3: Run all storage tests**

Run: `cargo test -p nexus-storage`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): expose task query and toggle on StorageEngine"
```

---

### Task 9: CLI — Tasks Subcommand

**Files:**
- Modify: `crates/nexus-cli/src/main.rs`
- Modify: `crates/nexus-cli/src/commands/content.rs`

- [ ] **Step 1: Add Tasks variants to ContentCommand**

In `main.rs`, add to the `ContentCommand` enum:

```rust
/// List tasks across the forge
Tasks {
    /// Show only completed tasks
    #[arg(long)]
    completed: bool,
    /// Show all tasks (completed and pending)
    #[arg(long)]
    all: bool,
    /// Filter to tasks in a specific file
    #[arg(long)]
    file: Option<String>,
},
/// Toggle a task's completion state
TaskToggle {
    /// Task ID to toggle
    id: u64,
},
```

- [ ] **Step 2: Add dispatch in main**

In the `Commands::Content(args) => match args.command { ... }` block, add:

```rust
ContentCommand::Tasks { completed, all, file } => {
    commands::content::tasks(&mut app, completed, all, file.as_deref())
}
ContentCommand::TaskToggle { id } => {
    commands::content::task_toggle(&mut app, id)
}
```

- [ ] **Step 3: Implement task list handler**

In `content.rs`, add:

```rust
/// List tasks across the forge.
pub fn tasks(app: &mut App, completed: bool, all: bool, file: Option<&str>) -> Result<()> {
    let storage = app.storage()?;

    let filter = nexus_storage::TaskFilter {
        completed: if all {
            None
        } else if completed {
            Some(true)
        } else {
            Some(false)
        },
        file_path: file.map(String::from),
    };

    let tasks = storage
        .query_tasks(&filter)
        .map_err(|e| anyhow::anyhow!("failed to query tasks: {e}"))?;

    let format = app.format();

    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }

    let headers = &["ID", "Status", "Content", "File", "Line"];
    let rows: Vec<Vec<String>> = tasks
        .iter()
        .map(|t| {
            vec![
                t.id.to_string(),
                if t.completed { "[x]".to_string() } else { "[ ]".to_string() },
                t.content.clone(),
                format!("{}:{}", t.file_path, t.line_number),
                t.line_number.to_string(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}
```

- [ ] **Step 4: Implement task toggle handler**

```rust
/// Toggle a task's completion state.
pub fn task_toggle(app: &mut App, task_id: u64) -> Result<()> {
    let storage = app.storage_mut()?;

    let record = storage
        .toggle_task(task_id)
        .map_err(|e| anyhow::anyhow!("failed to toggle task {task_id}: {e}"))?;

    let status = if record.completed { "completed" } else { "pending" };
    println!(
        "Task {} toggled to {}: {} ({}:{})",
        record.id, status, record.content, record.file_path, record.line_number
    );

    Ok(())
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p nexus-cli`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/nexus-cli/src/main.rs crates/nexus-cli/src/commands/content.rs
git commit -m "feat(cli): add tasks list and toggle subcommands"
```

---

### Task 10: Integration Tests

**Files:**
- Create: `crates/nexus-storage/tests/prd-06-smoke.rs`

- [ ] **Step 1: Write integration tests**

Create `crates/nexus-storage/tests/prd-06-smoke.rs`:

```rust
//! PRD 06 smoke tests — block refs, callouts, tasks, alias resolution.

use nexus_storage::{
    StorageEngine, FileFilter, TaskFilter,
    parse_markdown,
};

fn engine() -> (tempfile::TempDir, StorageEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(dir.path()).unwrap();
    (dir, engine)
}

#[test]
fn block_ref_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/refs.md", b"# Intro ^intro\n\nBody text ^body\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let blocks = engine.query_blocks(files[0].id).unwrap();

    let intro = blocks.iter().find(|b| b.block_ref_id == Some("intro".to_string()));
    assert!(intro.is_some(), "heading should have block_ref_id 'intro'");

    let body = blocks.iter().find(|b| b.block_ref_id == Some("body".to_string()));
    assert!(body.is_some(), "paragraph should have block_ref_id 'body'");
}

#[test]
fn callout_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/callouts.md", b"> [!warning] Watch out\n> Be careful here\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let blocks = engine.query_blocks(files[0].id).unwrap();

    let callout = blocks.iter().find(|b| b.callout_type == Some("warning".to_string()));
    assert!(callout.is_some(), "should have a warning callout block");
    assert_eq!(callout.unwrap().block_type, "callout");
}

#[test]
fn link_fragment_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/links.md", b"See [[other#^ref1]] and [[other#Heading]]\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let links = engine.query_links(files[0].id).unwrap();

    assert_eq!(links.len(), 2);
    let ref_link = links.iter().find(|l| l.fragment == Some("^ref1".to_string()));
    assert!(ref_link.is_some(), "should have fragment ^ref1");

    let heading_link = links.iter().find(|l| l.fragment == Some("Heading".to_string()));
    assert!(heading_link.is_some(), "should have fragment Heading");
}

#[test]
fn task_extraction_and_query() {
    let (_dir, engine) = engine();
    engine
        .write_file(
            "notes/tasks.md",
            b"# Tasks\n\n- [ ] Buy milk\n- [x] Write tests\n- [ ] Deploy\n",
        )
        .unwrap();

    let all = engine.query_tasks(&TaskFilter::default()).unwrap();
    assert_eq!(all.len(), 3);

    let pending = engine
        .query_tasks(&TaskFilter { completed: Some(false), ..Default::default() })
        .unwrap();
    assert_eq!(pending.len(), 2);

    let done = engine
        .query_tasks(&TaskFilter { completed: Some(true), ..Default::default() })
        .unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].content, "Write tests");
}

#[test]
fn task_toggle_writes_back_to_file() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/toggle.md", b"- [ ] Pending task\n")
        .unwrap();

    let tasks = engine.query_tasks(&TaskFilter::default()).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(!tasks[0].completed);

    let toggled = engine.toggle_task(tasks[0].id).unwrap();
    assert!(toggled.completed);

    // Verify file on disk was updated
    let content = engine.read_file("notes/toggle.md").unwrap();
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains("- [x]"), "file should have checked checkbox, got: {text}");
}

#[test]
fn alias_link_resolution() {
    let (_dir, engine) = engine();

    // File with aliases
    engine
        .write_file(
            "notes/real-name.md",
            b"---\naliases:\n  - Alt Name\n  - Another Alias\n---\n# Real Name\n",
        )
        .unwrap();

    // File linking via alias
    engine
        .write_file("notes/linker.md", b"See [[Alt Name]]\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let linker = files.iter().find(|f| f.path == "notes/linker.md").unwrap();
    let links = engine.query_links(linker.id).unwrap();

    assert_eq!(links.len(), 1);
    assert!(
        links[0].is_resolved,
        "link via alias should be resolved"
    );
    assert!(
        links[0].target_file_id.is_some(),
        "target_file_id should be set"
    );
}

#[test]
fn combined_markdown_features() {
    let md = concat!(
        "---\ntags:\n  - test\n---\n",
        "# Title ^title\n\n",
        "> [!note] Important\n> Remember this\n\n",
        "- [ ] First task\n- [x] Done task\n\n",
        "See [[other#^ref1]]\n\n",
        "Some text #inline-tag\n",
    );
    let pf = parse_markdown(md).unwrap();

    // Tags: 1 frontmatter + 1 inline
    assert!(pf.tags.iter().any(|t| t.name == "test" && t.source == "frontmatter"));
    assert!(pf.tags.iter().any(|t| t.name == "inline-tag" && t.source == "inline"));

    // Block ref on heading
    let title = pf.blocks.iter().find(|b| b.block_type == "heading").unwrap();
    assert_eq!(title.block_ref_id, Some("title".to_string()));

    // Callout
    let callout = pf.blocks.iter().find(|b| b.block_type == "callout").unwrap();
    assert_eq!(callout.callout_type, Some("note".to_string()));

    // Tasks
    assert_eq!(pf.tasks.len(), 2);

    // Link with fragment
    let link = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(link.fragment, Some("^ref1".to_string()));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p nexus-storage --test prd-06-smoke`
Expected: All PASS

- [ ] **Step 3: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All PASS (except known flaky credential vault test)

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace`
Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/tests/prd-06-smoke.rs
git commit -m "test(storage): add PRD 06 integration tests — block refs, callouts, tasks, alias resolution"
```
