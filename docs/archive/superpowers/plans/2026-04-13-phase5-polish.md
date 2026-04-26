# Phase 5: Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add HTML export, enhanced TUI features (backlinks panel, task view, fuzzy search, status bar), and watcher-driven reconcile to complete the Growth Plan.

**Architecture:** Three independent features. HTML export adds a comrak HTML renderer in nexus-storage. TUI enhancements add new state/widgets to nexus-tui. Watcher reconcile adds a `process_watcher_events` method to StorageEngine.

**Tech Stack:** comrak HTML rendering (existing), ratatui (existing), nexus-storage API (existing)

---

## File Structure

| File | Role | Change |
|------|------|--------|
| `crates/nexus-storage/src/export.rs` | HTML rendering | **NEW** |
| `crates/nexus-storage/src/lib.rs` | Module + StorageEngine methods | Modify |
| `crates/nexus-cli/src/main.rs` | CLI args | Modify |
| `crates/nexus-cli/src/commands/content.rs` | Export handler | Modify |
| `crates/nexus-tui/src/app.rs` | TUI state | Modify |
| `crates/nexus-tui/src/input.rs` | Key bindings | Modify |
| `crates/nexus-tui/src/ui/mod.rs` | Layout | Modify |
| `crates/nexus-tui/src/ui/backlinks.rs` | Backlinks panel widget | **NEW** |
| `crates/nexus-tui/src/ui/tasks.rs` | Task list widget | **NEW** |
| `crates/nexus-tui/src/ui/status_bar.rs` | Status bar enhancements | Modify |
| `crates/nexus-storage/tests/prd-06-smoke.rs` | Integration tests | Modify |

---

### Task 1: HTML Export

**Files:**
- Create: `crates/nexus-storage/src/export.rs`
- Modify: `crates/nexus-storage/src/lib.rs`
- Modify: `crates/nexus-cli/src/main.rs`
- Modify: `crates/nexus-cli/src/commands/content.rs`
- Modify: `crates/nexus-storage/tests/prd-06-smoke.rs`

- [ ] **Step 1: Create export.rs with HTML rendering function**

Create `crates/nexus-storage/src/export.rs`:

```rust
//! HTML export for markdown content.

use comrak::{Arena, Options, format_html, parse_document};

/// Render markdown content to a complete HTML document.
///
/// Uses comrak's HTML rendering with safe mode (no raw HTML passthrough).
/// Includes embedded CSS for callouts, code blocks, tasks, and basic styling.
#[must_use]
pub fn export_to_html(content: &str, title: &str) -> String {
    let arena = Arena::new();
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.render.unsafe_ = false; // safe mode

    let root = parse_document(&arena, content, &opts);

    let mut html_body = Vec::new();
    format_html(root, &opts, &mut html_body).expect("HTML rendering should not fail");
    let body = String::from_utf8_lossy(&html_body);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 2rem; line-height: 1.6; color: #1a1a1a; }}
h1, h2, h3, h4 {{ margin-top: 1.5em; }}
pre {{ background: #f4f4f4; padding: 1em; border-radius: 4px; overflow-x: auto; }}
code {{ background: #f4f4f4; padding: 0.2em 0.4em; border-radius: 3px; font-size: 0.9em; }}
pre code {{ background: none; padding: 0; }}
blockquote {{ border-left: 4px solid #ddd; margin-left: 0; padding-left: 1em; color: #555; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ border: 1px solid #ddd; padding: 0.5em; text-align: left; }}
th {{ background: #f4f4f4; }}
a {{ color: #4A90E2; }}
ul {{ list-style-type: disc; }}
input[type="checkbox"] {{ margin-right: 0.5em; }}
</style>
</head>
<body>
{body}
</body>
</html>"#,
        title = html_escape(title),
        body = body,
    )
}

/// Minimal HTML escaping for attribute values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_renders_heading() {
        let html = export_to_html("# Hello World\n", "Test");
        assert!(html.contains("<h1>Hello World</h1>"));
        assert!(html.contains("<title>Test</title>"));
    }

    #[test]
    fn export_renders_code_block() {
        let html = export_to_html("```rust\nfn main() {}\n```\n", "Code");
        assert!(html.contains("<code"));
        assert!(html.contains("fn main()"));
    }

    #[test]
    fn export_renders_task_list() {
        let html = export_to_html("- [ ] Pending\n- [x] Done\n", "Tasks");
        assert!(html.contains("checkbox"));
    }

    #[test]
    fn export_renders_table() {
        let html = export_to_html("| A | B |\n|---|---|\n| 1 | 2 |\n", "Table");
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn export_has_complete_html_structure() {
        let html = export_to_html("Hello\n", "Doc");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<style>"));
    }
}
```

- [ ] **Step 2: Register module and add pub use in lib.rs**

In `crates/nexus-storage/src/lib.rs`, add after `mod search_scope;`:

```rust
mod export;
```

Add to pub use exports:

```rust
pub use export::export_to_html;
```

- [ ] **Step 3: Add CLI export command**

In `crates/nexus-cli/src/main.rs`, add to `ContentCommand` enum:

```rust
/// Export a note to HTML
Export {
    /// Path of the note to export
    path: String,
    /// Output file path (prints to stdout if omitted)
    #[arg(short, long)]
    output: Option<String>,
},
```

Add dispatch:

```rust
ContentCommand::Export { path, output } => {
    commands::content::export(&mut app, &path, output.as_deref())
}
```

In `crates/nexus-cli/src/commands/content.rs`, add:

```rust
/// Export a note to HTML.
pub fn export(app: &mut App, path: &str, output: Option<&str>) -> Result<()> {
    let storage = app.storage()?;
    let bytes = storage
        .read_file(path)
        .map_err(|e| anyhow::anyhow!("failed to read file '{path}': {e}"))?;
    let text = String::from_utf8_lossy(&bytes);

    let title = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");

    let html = nexus_storage::export_to_html(&text, title);

    if let Some(out_path) = output {
        std::fs::write(out_path, &html)
            .map_err(|e| anyhow::anyhow!("failed to write '{out_path}': {e}"))?;
        println!("Exported to {out_path}");
    } else {
        print!("{html}");
    }

    Ok(())
}
```

- [ ] **Step 4: Add integration test**

Add to `crates/nexus-storage/tests/prd-06-smoke.rs`:

```rust
#[test]
fn html_export_renders_markdown() {
    let html = nexus_storage::export_to_html("# Hello\n\nWorld\n", "Test");
    assert!(html.contains("<h1>Hello</h1>"));
    assert!(html.contains("World"));
    assert!(html.contains("<!DOCTYPE html>"));
}
```

- [ ] **Step 5: Verify and commit**

Run: `cargo test -p nexus-storage --lib export && cargo test -p nexus-storage --test prd-06-smoke && cargo check --workspace`

```bash
git add crates/nexus-storage/src/export.rs crates/nexus-storage/src/lib.rs crates/nexus-cli/src/main.rs crates/nexus-cli/src/commands/content.rs crates/nexus-storage/tests/prd-06-smoke.rs
git commit -m "feat(storage): add HTML export for markdown notes"
```

---

### Task 2: Watcher Reconcile

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Add process_watcher_events method to StorageEngine**

Add to `impl StorageEngine`, in a new `// ── Watcher Reconcile ──` section before the `// ── Accessor ──` section:

```rust
/// Process pending file watcher events, re-indexing changed files.
///
/// Drains all pending events from the watcher (non-blocking). For each event:
/// - `FileCreated`/`FileModified`: re-reads the file from disk and re-indexes it
/// - `FileDeleted`: removes the file from the index and graph
/// - `FileRenamed`: removes the old path and indexes the new path
///
/// Returns the number of events processed.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or database failure.
///
/// # Panics
///
/// Panics if the write-connection mutex is poisoned.
pub fn process_watcher_events(&self) -> Result<usize, StorageError> {
    let rx = match self.watcher.as_ref() {
        Some(w) => w.events(),
        None => return Ok(0),
    };

    let mut count = 0;
    loop {
        match rx.try_recv() {
            Ok(event) => {
                match &event {
                    StorageEvent::FileCreated { path, .. }
                    | StorageEvent::FileModified { path, .. } => {
                        let abs = self.forge.root().join(path);
                        if let Ok(bytes) = std::fs::read(&abs) {
                            let _ = self.write_file(path, &bytes);
                        }
                    }
                    StorageEvent::FileDeleted { path } => {
                        let _ = self.delete_file(path);
                    }
                    StorageEvent::FileRenamed { from, to, .. } => {
                        let _ = self.delete_file(from);
                        let abs = self.forge.root().join(to);
                        if let Ok(bytes) = std::fs::read(&abs) {
                            let _ = self.write_file(to, &bytes);
                        }
                    }
                }
                count += 1;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }

    Ok(count)
}
```

- [ ] **Step 2: Verify and commit**

Run: `cargo check -p nexus-storage && cargo test -p nexus-storage --lib`

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): add process_watcher_events for auto-reconcile on file changes"
```

---

### Task 3: Enhanced TUI — Backlinks Panel

**Files:**
- Modify: `crates/nexus-tui/src/app.rs`
- Create: `crates/nexus-tui/src/ui/backlinks.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`
- Modify: `crates/nexus-tui/src/input.rs`

- [ ] **Step 1: Add backlinks state to TuiApp**

In `crates/nexus-tui/src/app.rs`, add to the app state:

```rust
/// State for the backlinks panel.
pub struct BacklinksState {
    /// Whether the backlinks panel is visible.
    pub visible: bool,
    /// Backlink entries for the current file.
    pub entries: Vec<BacklinkEntry>,
    /// Selected index in the backlinks list.
    pub selected: usize,
    /// ratatui list state.
    pub list_state: ListState,
}

/// A single backlink entry.
pub struct BacklinkEntry {
    /// Path of the file that links to the current file.
    pub source_path: String,
    /// Link text.
    pub link_text: String,
}

impl BacklinksState {
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn load(&mut self, entries: Vec<BacklinkEntry>) {
        self.entries = entries;
        self.selected = 0;
        self.list_state.select(if self.entries.is_empty() { None } else { Some(0) });
    }
}
```

Add a `backlinks: BacklinksState` field to the `TuiApp` struct and initialize it in `new()`.

Also add a method to load backlinks when a file is selected:

```rust
pub fn load_backlinks_for_current_file(&mut self) {
    let path = match &self.viewer.file_path {
        Some(p) => p.clone(),
        None => { self.backlinks.load(vec![]); return; }
    };
    let entries = self.storage.backlinks(&path)
        .unwrap_or_default()
        .into_iter()
        .map(|bl| BacklinkEntry {
            source_path: bl.source_path,
            link_text: bl.link_text,
        })
        .collect();
    self.backlinks.load(entries);
}
```

Call `load_backlinks_for_current_file()` whenever a file is loaded into the viewer.

- [ ] **Step 2: Create backlinks panel widget**

Create `crates/nexus-tui/src/ui/backlinks.rs`:

```rust
//! Backlinks panel widget.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::BacklinksState;

/// Render the backlinks panel.
pub fn render(frame: &mut Frame, area: Rect, state: &mut BacklinksState) {
    let items: Vec<ListItem> = state
        .entries
        .iter()
        .map(|entry| {
            let line = Line::from(vec![
                Span::styled(&entry.source_path, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(&entry.link_text, Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = format!(" Backlinks ({}) ", state.entries.len());
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut state.list_state);
}
```

- [ ] **Step 3: Wire into layout and input**

In `crates/nexus-tui/src/ui/mod.rs`, register the module:

```rust
pub mod backlinks;
```

Update the layout rendering to split the viewer area when backlinks are visible — use `Layout::vertical` to split the right pane into viewer (70%) + backlinks (30%) when `app.backlinks.visible` is true.

In `crates/nexus-tui/src/input.rs`, add handling for the `b` key in Normal mode:

```rust
KeyCode::Char('b') => {
    app.backlinks.toggle();
    if app.backlinks.visible {
        app.load_backlinks_for_current_file();
    }
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p nexus-tui`

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add toggleable backlinks panel with b key"
```

---

### Task 4: Enhanced TUI — Task List View

**Files:**
- Modify: `crates/nexus-tui/src/app.rs`
- Create: `crates/nexus-tui/src/ui/tasks.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`
- Modify: `crates/nexus-tui/src/input.rs`

- [ ] **Step 1: Add task view state**

In `crates/nexus-tui/src/app.rs`, add:

```rust
/// State for the task list view.
pub struct TaskViewState {
    /// Whether task view mode is active (replaces viewer content).
    pub active: bool,
    /// Task entries.
    pub entries: Vec<TaskEntry>,
    /// Selected index.
    pub selected: usize,
    /// ratatui list state.
    pub list_state: ListState,
}

pub struct TaskEntry {
    pub id: u64,
    pub completed: bool,
    pub content: String,
    pub file_path: String,
    pub line_number: u32,
}

impl TaskViewState {
    pub fn new() -> Self {
        Self { active: false, entries: Vec::new(), selected: 0, list_state: ListState::default() }
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    pub fn load(&mut self, entries: Vec<TaskEntry>) {
        self.entries = entries;
        self.selected = 0;
        self.list_state.select(if self.entries.is_empty() { None } else { Some(0) });
    }
}
```

Add `task_view: TaskViewState` to `TuiApp` and a method:

```rust
pub fn load_tasks(&mut self) {
    let filter = nexus_storage::TaskFilter::default();
    let entries = self.storage.query_tasks(&filter)
        .unwrap_or_default()
        .into_iter()
        .map(|t| TaskEntry {
            id: t.id,
            completed: t.completed,
            content: t.content,
            file_path: t.file_path,
            line_number: t.line_number,
        })
        .collect();
    self.task_view.load(entries);
}
```

- [ ] **Step 2: Create task list widget**

Create `crates/nexus-tui/src/ui/tasks.rs`:

```rust
//! Task list view widget.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::TaskViewState;

/// Render the task list view.
pub fn render(frame: &mut Frame, area: Rect, state: &mut TaskViewState) {
    let items: Vec<ListItem> = state
        .entries
        .iter()
        .map(|entry| {
            let checkbox = if entry.completed { "[x]" } else { "[ ]" };
            let style = if entry.completed {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            let line = Line::from(vec![
                Span::styled(checkbox, style),
                Span::raw(" "),
                Span::styled(&entry.content, style),
                Span::raw("  "),
                Span::styled(
                    format!("{}:{}", entry.file_path, entry.line_number),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let pending = state.entries.iter().filter(|t| !t.completed).count();
    let title = format!(" Tasks ({} pending, {} total) ", pending, state.entries.len());
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut state.list_state);
}
```

- [ ] **Step 3: Wire into layout and input**

Register module in `ui/mod.rs`:
```rust
pub mod tasks;
```

In the layout rendering, when `app.task_view.active` is true, render the task list widget in the viewer area instead of the file viewer.

In `input.rs`, add `t` key handling in Normal mode:

```rust
KeyCode::Char('t') => {
    app.task_view.toggle();
    if app.task_view.active {
        app.load_tasks();
    }
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p nexus-tui`

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add task list view with t key toggle"
```

---

### Task 5: Enhanced TUI — Fuzzy Search + Status Bar

**Files:**
- Modify: `crates/nexus-tui/src/app.rs`
- Modify: `crates/nexus-tui/src/ui/file_tree.rs`
- Modify: `crates/nexus-tui/src/ui/status_bar.rs`

- [ ] **Step 1: Add search filtering to file tree**

In `app.rs`, add to `SearchState` (or the existing search overlay state):

```rust
/// Filtered entries matching the current search query (for file tree filtering).
pub fn filter_tree_entries(entries: &[TreeEntry], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..entries.len()).collect();
    }
    let query_lower = query.to_lowercase();
    entries.iter().enumerate()
        .filter(|(_, e)| e.path.to_lowercase().contains(&query_lower))
        .map(|(i, _)| i)
        .collect()
}
```

When the search overlay is active and the user types, filter the file tree entries to only show matching paths. The existing search overlay likely has a `query: String` field — use it to drive the filter.

- [ ] **Step 2: Enhance status bar**

In `crates/nexus-tui/src/ui/status_bar.rs`, update the status bar to show additional stats. Add a `StatusInfo` struct to `app.rs`:

```rust
pub struct StatusInfo {
    pub file_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub pending_tasks: usize,
}
```

Add a method to `TuiApp`:

```rust
pub fn refresh_status(&mut self) {
    let file_count = self.storage.list_files("").map(|f| f.len()).unwrap_or(0);
    let graph = self.storage.graph_stats().unwrap_or(nexus_storage::GraphStats {
        node_count: 0, edge_count: 0, unresolved_count: 0,
    });
    let pending = self.storage.query_tasks(&nexus_storage::TaskFilter {
        completed: Some(false), ..Default::default()
    }).map(|t| t.len()).unwrap_or(0);
    self.status = StatusInfo {
        file_count,
        node_count: graph.node_count,
        edge_count: graph.edge_count,
        pending_tasks: pending,
    };
}
```

Update the status bar renderer to display: `Files: N | Links: N | Tasks: N pending`

- [ ] **Step 3: Verify and commit**

Run: `cargo check -p nexus-tui`

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add fuzzy file search and enhanced status bar"
```

---

### Task 6: Final Verification

- [ ] **Step 1: Run all workspace tests**

Run: `cargo test --workspace`
Expected: All PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace`
Expected: No new warnings

- [ ] **Step 3: Manual TUI test**

Run: `cargo run -p nexus-tui -- /tmp/test-forge` (init a forge first)
- Create some notes with links and tasks
- Press `b` to toggle backlinks panel
- Press `t` to toggle task view
- Type in search overlay to filter files
- Check status bar shows stats

- [ ] **Step 4: Test HTML export**

```bash
cargo run -p nexus-cli -- --forge-path /tmp/test-forge content export notes/hello.md --output /tmp/test.html
```
Verify the HTML file opens correctly in a browser.

- [ ] **Step 5: Commit any fixes**

If any issues, fix and commit.
