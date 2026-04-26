# Nexus TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus-tui` binary — a ratatui terminal UI for browsing, searching, and viewing forge content with a two-panel layout (file tree + read-only viewer).

**Architecture:** Binary crate `nexus-tui` using ratatui + crossterm. App state struct drives rendering. Event loop: render → poll crossterm events → dispatch input → update state. File tree built from StorageEngine queries. Viewer displays file content with markdown syntax highlighting. Search overlay uses Tantivy. In-file find uses substring matching.

**Tech Stack:** Rust (edition 2024), `ratatui` 0.29, `crossterm` 0.28, `anyhow` 1.0.

**Parent docs:**
- [`2026-04-12-nexus-tui-design.md`](../specs/2026-04-12-nexus-tui-design.md) — **the contract this plan implements**

---

## Prerequisites

1. M1 complete, all 340 tests pass.
2. Verify: `cargo nextest run --workspace` passes.

---

## File Structure

```
crates/nexus-tui/
├── Cargo.toml
└── src/
    ├── main.rs          # entry point, terminal setup/teardown, event loop
    ├── app.rs           # TuiApp state, mode, focus, tree/viewer/search/find state
    ├── input.rs         # keyboard + mouse input dispatch
    └── ui/
        ├── mod.rs       # top-level render function
        ├── file_tree.rs # left panel: directory tree widget
        ├── viewer.rs    # right panel: syntax-highlighted file viewer
        ├── status_bar.rs # bottom: mode, file, stats
        ├── search.rs    # Ctrl+F overlay: forge-wide search
        └── find.rs      # / bar: in-file text search
```

Modifications to existing files:
- `Cargo.toml` (workspace root): add `nexus-tui` to members, add `ratatui`, `crossterm` deps

---

## Task Overview

12 tasks across 6 phases:

1. Phase 1: Crate skeleton + event loop (Tasks 1–2)
2. Phase 2: App state + file tree (Tasks 3–4)
3. Phase 3: Viewer with syntax highlighting (Tasks 5–6)
4. Phase 4: Status bar + input wiring (Tasks 7–8)
5. Phase 5: Search overlay + in-file find (Tasks 9–10)
6. Phase 6: $EDITOR integration + polish (Tasks 11–12)

---

## Phase 1: Crate Skeleton + Event Loop

### Task 1: Create nexus-tui crate

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-tui/Cargo.toml`
- Create: `crates/nexus-tui/src/main.rs`

- [ ] **Step 1: Add workspace deps**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

Add `"crates/nexus-tui"` to `[workspace]` members.

Add to `[workspace.dependencies]`:
```toml
ratatui = "0.29"
crossterm = "0.28"
```

- [ ] **Step 2: Create Cargo.toml**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/Cargo.toml`:

```toml
[package]
name = "nexus-tui"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus IDE — terminal UI"

[[bin]]
name = "nexus-tui"
path = "src/main.rs"

[dependencies]
nexus-storage = { path = "../nexus-storage" }
nexus-kernel = { path = "../nexus-kernel" }
nexus-types = { path = "../nexus-types" }
ratatui = { workspace = true }
crossterm = { workspace = true }
anyhow = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 3: Create main.rs with terminal setup and empty event loop**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/main.rs`:

```rust
use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{DefaultTerminal, Frame};

fn main() -> Result<()> {
    // Set up terminal
    crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;
    let mut terminal = ratatui::init();

    // Run app (catch errors to ensure cleanup)
    let result = run(&mut terminal);

    // Restore terminal
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    crossterm::terminal::disable_raw_mode().ok();
    ratatui::restore();

    result
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(|frame| render(frame))?;

        if event::poll(Duration::from_millis(16))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press
                    && key.code == event::KeyCode::Char('q')
                {
                    return Ok(());
                }
            }
        }
    }
}

fn render(frame: &mut Frame) {
    frame.render_widget(
        ratatui::widgets::Paragraph::new("Nexus TUI — press 'q' to quit"),
        frame.area(),
    );
}
```

- [ ] **Step 4: Verify**

Run: `cargo build -p nexus-tui`
Run: `./target/debug/nexus-tui` — shows message, press `q` to quit, terminal restores cleanly.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-tui/
git commit -m "feat(tui): scaffold nexus-tui binary with terminal setup and event loop"
```

---

### Task 2: Add two-panel layout

**Files:**
- Create: `crates/nexus-tui/src/ui/mod.rs`
- Modify: `crates/nexus-tui/src/main.rs`

- [ ] **Step 1: Create ui/mod.rs with layout**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/mod.rs`:

```rust
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the full TUI layout.
pub fn render(frame: &mut Frame) {
    // Vertical: body + status bar
    let [body, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // Horizontal: file tree (25%) + viewer (75%)
    let [tree_area, viewer_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ])
    .areas(body);

    // File tree placeholder
    let tree = Paragraph::new("File tree here")
        .block(Block::default().borders(Borders::ALL).title(" Files "));
    frame.render_widget(tree, tree_area);

    // Viewer placeholder
    let viewer = Paragraph::new("Viewer here")
        .block(Block::default().borders(Borders::ALL).title(" Preview "));
    frame.render_widget(viewer, viewer_area);

    // Status bar placeholder
    let status_bar = Paragraph::new(" NORMAL │ no file │ press q to quit");
    frame.render_widget(status_bar, status);
}
```

- [ ] **Step 2: Wire into main.rs**

In `main.rs`, replace the inline `render` function:

```rust
mod ui;

// ... in run():
terminal.draw(|frame| ui::render(frame))?;

// Remove the old render function
```

- [ ] **Step 3: Verify**

Run: `cargo build -p nexus-tui && ./target/debug/nexus-tui`
Should show two bordered panels + status bar. `q` to quit.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add two-panel layout (file tree + viewer + status bar)"
```

---

## Phase 2: App State + File Tree

### Task 3: Create app state

**Files:**
- Create: `crates/nexus-tui/src/app.rs`
- Modify: `crates/nexus-tui/src/main.rs`

- [ ] **Step 1: Create app.rs with TuiApp and state types**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/app.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nexus_storage::{FileFilter, StorageConfig, StorageEngine};

/// Application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Find,
}

/// Which panel has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    FileTree,
    Viewer,
}

/// A file tree entry.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub depth: usize,
}

/// File tree state.
pub struct TreeState {
    pub entries: Vec<TreeEntry>,
    pub selected: usize,
    pub list_state: ratatui::widgets::ListState,
}

impl TreeState {
    pub fn new() -> Self {
        let mut list_state = ratatui::widgets::ListState::default();
        list_state.select(Some(0));
        Self {
            entries: Vec::new(),
            selected: 0,
            list_state,
        }
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.list_state.select(Some(index));
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.select(self.selected - 1);
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.select(self.selected + 1);
        }
    }
}

/// Viewer state.
pub struct ViewerState {
    pub file_path: Option<String>,
    pub content: String,
    pub lines: Vec<String>,
    pub scroll_offset: usize,
}

impl ViewerState {
    pub fn new() -> Self {
        Self {
            file_path: None,
            content: String::new(),
            lines: Vec::new(),
            scroll_offset: 0,
        }
    }

    pub fn load_content(&mut self, path: &str, content: &str) {
        self.file_path = Some(path.to_string());
        self.content = content.to_string();
        self.lines = content.lines().map(String::from).collect();
        self.scroll_offset = 0;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        let max = self.lines.len().saturating_sub(1);
        self.scroll_offset = self.scroll_offset.min(max);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.lines.len().saturating_sub(1);
    }
}

/// Search overlay state.
pub struct SearchState {
    pub query: String,
    pub results: Vec<nexus_storage::SearchResult>,
    pub selected: usize,
    pub cursor_pos: usize,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            cursor_pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.results.clear();
        self.selected = 0;
        self.cursor_pos = 0;
    }
}

/// In-file find state.
pub struct FindState {
    pub query: String,
    pub matches: Vec<(usize, usize)>, // (line, col)
    pub current_match: usize,
    pub cursor_pos: usize,
}

impl FindState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_match: 0,
            cursor_pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
        self.cursor_pos = 0;
    }

    /// Find all occurrences of the query in the given lines.
    pub fn update_matches(&mut self, lines: &[String]) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let query_lower = self.query.to_lowercase();
        for (line_idx, line) in lines.iter().enumerate() {
            let line_lower = line.to_lowercase();
            let mut start = 0;
            while let Some(pos) = line_lower[start..].find(&query_lower) {
                self.matches.push((line_idx, start + pos));
                start += pos + query_lower.len();
            }
        }
        self.current_match = 0;
    }

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }
}

/// The main TUI application state.
pub struct TuiApp {
    pub mode: Mode,
    pub focus: Focus,
    pub tree: TreeState,
    pub viewer: ViewerState,
    pub search: SearchState,
    pub find: FindState,
    pub storage: StorageEngine,
    pub forge_root: PathBuf,
    pub should_quit: bool,
}

impl TuiApp {
    /// Create a new TuiApp for the given forge root.
    pub fn new(forge_root: &Path) -> Result<Self> {
        let storage = StorageEngine::open(forge_root, &StorageConfig::default())
            .context("failed to open forge")?;

        let mut app = Self {
            mode: Mode::Normal,
            focus: Focus::FileTree,
            tree: TreeState::new(),
            viewer: ViewerState::new(),
            search: SearchState::new(),
            find: FindState::new(),
            storage,
            forge_root: forge_root.to_path_buf(),
            should_quit: false,
        };

        app.refresh_tree()?;
        Ok(app)
    }

    /// Rebuild the file tree from the storage index.
    pub fn refresh_tree(&mut self) -> Result<()> {
        let files = self
            .storage
            .query_files(&FileFilter::default())
            .context("failed to query files")?;

        let mut entries = Vec::new();
        let mut dirs_seen = std::collections::BTreeSet::new();

        // Sort files by path
        let mut sorted: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        sorted.sort();

        for path in &sorted {
            // Add directory entries for parent dirs
            let parts: Vec<&str> = path.split('/').collect();
            for i in 0..parts.len() - 1 {
                let dir_path = parts[..=i].join("/");
                if dirs_seen.insert(dir_path.clone()) {
                    entries.push(TreeEntry {
                        path: dir_path.clone(),
                        name: parts[i].to_string(),
                        is_dir: true,
                        is_expanded: true,
                        depth: i,
                    });
                }
            }
            // Add file entry
            entries.push(TreeEntry {
                path: path.clone(),
                name: parts.last().unwrap_or(&"").to_string(),
                is_dir: false,
                is_expanded: false,
                depth: parts.len() - 1,
            });
        }

        self.tree.entries = entries;
        if !self.tree.entries.is_empty() && self.tree.selected >= self.tree.entries.len() {
            self.tree.select(0);
        }

        Ok(())
    }

    /// Open the selected file in the viewer.
    pub fn open_selected_file(&mut self) -> Result<()> {
        if self.tree.selected >= self.tree.entries.len() {
            return Ok(());
        }
        let entry = &self.tree.entries[self.tree.selected];
        if entry.is_dir {
            return Ok(());
        }
        let path = entry.path.clone();
        let content = self
            .storage
            .read_file(&path)
            .context("failed to read file")?;
        let text = String::from_utf8_lossy(&content).to_string();
        self.viewer.load_content(&path, &text);
        self.focus = Focus::Viewer;
        Ok(())
    }

    /// Toggle expansion of a directory entry.
    pub fn toggle_dir(&mut self) {
        if self.tree.selected >= self.tree.entries.len() {
            return;
        }
        let is_dir = self.tree.entries[self.tree.selected].is_dir;
        if is_dir {
            let expanded = self.tree.entries[self.tree.selected].is_expanded;
            self.tree.entries[self.tree.selected].is_expanded = !expanded;
        }
    }

    /// Get visible tree entries (respecting collapsed directories).
    pub fn visible_entries(&self) -> Vec<(usize, &TreeEntry)> {
        let mut visible = Vec::new();
        let mut skip_depth: Option<usize> = None;

        for (i, entry) in self.tree.entries.iter().enumerate() {
            if let Some(d) = skip_depth {
                if entry.depth > d {
                    continue;
                }
                skip_depth = None;
            }
            visible.push((i, entry));
            if entry.is_dir && !entry.is_expanded {
                skip_depth = Some(entry.depth);
            }
        }
        visible
    }
}
```

- [ ] **Step 2: Wire into main.rs**

Update `main.rs` to create `TuiApp` and pass it to the event loop:

```rust
mod app;
mod ui;

use std::path::PathBuf;

fn main() -> Result<()> {
    let forge_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("NEXUS_FORGE_PATH").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".nexus/default")
        });

    let mut app = app::TuiApp::new(&forge_path)?;

    // Terminal setup
    crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;
    let mut terminal = ratatui::init();

    let result = run(&mut terminal, &mut app);

    // Restore
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    crossterm::terminal::disable_raw_mode().ok();
    ratatui::restore();

    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut app::TuiApp) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(16))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press
                    && key.code == event::KeyCode::Char('q')
                {
                    return Ok(());
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
```

Update `ui::render` to accept `&app::TuiApp`.

- [ ] **Step 3: Verify**

Run: `cargo build -p nexus-tui`
Create a test forge: `./target/debug/nexus --forge-path /tmp/test-tui forge init`
Create a file: `./target/debug/nexus --forge-path /tmp/test-tui content create notes/hello.md --content "# Hello"`
Run TUI: `./target/debug/nexus-tui /tmp/test-tui`

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add TuiApp state with tree, viewer, search, find"
```

---

### Task 4: Render file tree widget

**Files:**
- Create: `crates/nexus-tui/src/ui/file_tree.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Create file_tree.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/file_tree.rs`:

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::app::{Focus, TuiApp};

pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let focused = app.focus == Focus::FileTree;
    let border_color = if focused { Color::Blue } else { Color::DarkGray };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Files ");

    let visible = app.visible_entries();

    let items: Vec<ListItem> = visible
        .iter()
        .map(|(_, entry)| {
            let indent = "  ".repeat(entry.depth);
            let icon = if entry.is_dir {
                if entry.is_expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };
            let style = if entry.is_dir {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::raw(indent),
                Span::raw(icon),
                Span::styled(&entry.name, style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Map the selected index to the visible index
    let visible_selected = visible
        .iter()
        .position(|(idx, _)| *idx == app.tree.selected);
    if let Some(vis_idx) = visible_selected {
        app.tree.list_state.select(Some(vis_idx));
    }

    frame.render_stateful_widget(list, area, &mut app.tree.list_state);
}
```

- [ ] **Step 2: Update ui/mod.rs to use file_tree**

```rust
mod file_tree;

pub fn render(frame: &mut Frame, app: &mut TuiApp) {
    let [body, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(frame.area());

    let [tree_area, viewer_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ]).areas(body);

    file_tree::render(frame, app, tree_area);

    // Viewer placeholder
    let viewer_block = Block::default().borders(Borders::ALL).title(" Preview ");
    let viewer_text = match &app.viewer.file_path {
        Some(p) => format!("Viewing: {p}"),
        None => "Select a file".to_string(),
    };
    frame.render_widget(Paragraph::new(viewer_text).block(viewer_block), viewer_area);

    // Status bar placeholder
    let status_text = format!(
        " {:?} │ {} │ {} files ",
        app.mode,
        app.viewer.file_path.as_deref().unwrap_or("no file"),
        app.tree.entries.iter().filter(|e| !e.is_dir).count(),
    );
    frame.render_widget(Paragraph::new(status_text), status);
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add file tree widget with expand/collapse and selection"
```

---

## Phase 3: Viewer

### Task 5: Render viewer with syntax highlighting

**Files:**
- Create: `crates/nexus-tui/src/ui/viewer.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Create viewer.rs with markdown highlighting**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/viewer.rs`:

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{Focus, TuiApp};

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let focused = app.focus == Focus::Viewer;
    let border_color = if focused { Color::Blue } else { Color::DarkGray };

    let title = match &app.viewer.file_path {
        Some(p) => format!(" {p} "),
        None => " Preview ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    if app.viewer.lines.is_empty() {
        let empty = Paragraph::new("Select a file from the tree (Enter)")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    // Build styled lines with syntax highlighting + line numbers
    let inner_height = area.height.saturating_sub(2) as usize; // minus borders
    let visible_lines = &app.viewer.lines[app.viewer.scroll_offset..];
    let line_num_width = app.viewer.lines.len().to_string().len();

    let styled_lines: Vec<Line> = visible_lines
        .iter()
        .take(inner_height)
        .enumerate()
        .map(|(i, line)| {
            let line_num = app.viewer.scroll_offset + i + 1;
            let num_str = format!("{:>width$} │ ", line_num, width = line_num_width);
            let mut spans = vec![Span::styled(num_str, Style::default().fg(Color::DarkGray))];
            spans.extend(highlight_line(line));
            Line::from(spans)
        })
        .collect();

    let paragraph = Paragraph::new(styled_lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Apply simple markdown syntax highlighting to a line.
fn highlight_line(line: &str) -> Vec<Span<'_>> {
    // Heading
    if line.starts_with('#') {
        let level = line.chars().take_while(|c| *c == '#').count();
        let color = match level {
            1 => Color::Magenta,
            2 => Color::Blue,
            3 => Color::Cyan,
            _ => Color::Green,
        };
        return vec![Span::styled(
            line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )];
    }

    // Code fence
    if line.starts_with("```") {
        return vec![Span::styled(line, Style::default().fg(Color::DarkGray))];
    }

    // Blockquote
    if line.starts_with('>') {
        return vec![Span::styled(
            line,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )];
    }

    // Horizontal rule
    if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
        return vec![Span::styled(line, Style::default().fg(Color::DarkGray))];
    }

    // List items
    if line.trim_start().starts_with("- ")
        || line.trim_start().starts_with("* ")
        || line.trim_start().starts_with("+ ")
    {
        return vec![Span::styled(line, Style::default().fg(Color::White))];
    }

    // Default: scan for inline elements
    highlight_inline(line)
}

/// Highlight inline markdown: `code`, [[wikilinks]], #tags.
fn highlight_inline(line: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = line.as_bytes();

    while pos < bytes.len() {
        // Inline code
        if bytes[pos] == b'`' {
            if let Some(end) = line[pos + 1..].find('`') {
                let end = pos + 1 + end + 1;
                spans.push(Span::styled(
                    &line[pos..end],
                    Style::default().fg(Color::Yellow),
                ));
                pos = end;
                continue;
            }
        }

        // Wikilink
        if pos + 1 < bytes.len() && bytes[pos] == b'[' && bytes[pos + 1] == b'[' {
            if let Some(end) = line[pos + 2..].find("]]") {
                let end = pos + 2 + end + 2;
                spans.push(Span::styled(
                    &line[pos..end],
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                pos = end;
                continue;
            }
        }

        // Tag
        if bytes[pos] == b'#'
            && (pos == 0 || bytes[pos - 1].is_ascii_whitespace())
            && pos + 1 < bytes.len()
            && bytes[pos + 1].is_ascii_alphanumeric()
        {
            let tag_end = line[pos + 1..]
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                .map(|e| pos + 1 + e)
                .unwrap_or(bytes.len());
            spans.push(Span::styled(
                &line[pos..tag_end],
                Style::default().fg(Color::Green),
            ));
            pos = tag_end;
            continue;
        }

        // Regular character — accumulate until next special char
        let next_special = line[pos..]
            .find(|c: char| c == '`' || c == '[' || c == '#')
            .map(|p| pos + p)
            .unwrap_or(bytes.len());
        spans.push(Span::raw(&line[pos..next_special]));
        pos = next_special;
    }

    if spans.is_empty() {
        spans.push(Span::raw(line));
    }
    spans
}
```

- [ ] **Step 2: Wire into ui/mod.rs**

Add `mod viewer;` and call `viewer::render(frame, app, viewer_area);` instead of the placeholder.

- [ ] **Step 3: Verify**

Run TUI against a forge with markdown files. Should see syntax highlighting, line numbers.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add viewer with markdown syntax highlighting and line numbers"
```

---

### Task 6: Create status bar

**Files:**
- Create: `crates/nexus-tui/src/ui/status_bar.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Create status_bar.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/status_bar.rs`:

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{Mode, TuiApp};

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let mode_str = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Search => "SEARCH",
        Mode::Find => "FIND",
    };

    let mode_color = match app.mode {
        Mode::Normal => Color::Green,
        Mode::Search => Color::Yellow,
        Mode::Find => Color::Cyan,
    };

    let file = app
        .viewer
        .file_path
        .as_deref()
        .unwrap_or("no file");

    let file_count = app.tree.entries.iter().filter(|e| !e.is_dir).count();

    let scroll_info = if !app.viewer.lines.is_empty() {
        let pos = app.viewer.scroll_offset + 1;
        let total = app.viewer.lines.len();
        format!(" {pos}/{total}")
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {mode_str} "),
            Style::default()
                .fg(Color::Black)
                .bg(mode_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(file, Style::default().fg(Color::White)),
        Span::styled(scroll_info, Style::default().fg(Color::DarkGray)),
        Span::raw(" │ "),
        Span::styled(
            format!("{file_count} files"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" │ "),
        Span::styled("Ctrl+? help", Style::default().fg(Color::DarkGray)),
    ]);

    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));
    frame.render_widget(bar, area);
}
```

- [ ] **Step 2: Wire into ui/mod.rs**

Add `mod status_bar;` and call `status_bar::render(frame, app, status);`.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add status bar with mode, file, scroll position"
```

---

## Phase 4: Input Handling

### Task 7: Create input handler

**Files:**
- Create: `crates/nexus-tui/src/input.rs`
- Modify: `crates/nexus-tui/src/main.rs`

- [ ] **Step 1: Create input.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/input.rs`:

```rust
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};

use crate::app::{Focus, Mode, TuiApp};

/// Handle a crossterm event. Returns Ok(()) always; sets app.should_quit to exit.
pub fn handle_event(app: &mut TuiApp, event: Event) -> Result<()> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key(app, key)?;
        }
        Event::Mouse(mouse) => {
            handle_mouse(app, mouse)?;
        }
        _ => {}
    }
    Ok(())
}

fn handle_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match app.mode {
        Mode::Normal => handle_normal_key(app, key),
        Mode::Search => handle_search_key(app, key),
        Mode::Find => handle_find_key(app, key),
    }
}

fn handle_normal_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    // Global keys
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::CONTROL, KeyCode::Char('f')) => {
            app.mode = Mode::Search;
            app.search.clear();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.mode = Mode::Find;
            app.find.clear();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.focus = match app.focus {
                Focus::FileTree => Focus::Viewer,
                Focus::Viewer => Focus::FileTree,
            };
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        _ => {}
    }

    // Panel-specific keys
    match app.focus {
        Focus::FileTree => handle_tree_key(app, key),
        Focus::Viewer => handle_viewer_key(app, key),
    }
}

fn handle_tree_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.tree.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.tree.move_up(),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            let selected = app.tree.selected;
            if selected < app.tree.entries.len() {
                if app.tree.entries[selected].is_dir {
                    app.toggle_dir();
                } else {
                    app.open_selected_file()?;
                }
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.toggle_dir();
        }
        _ => {}
    }
    Ok(())
}

fn handle_viewer_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Char('j')) | (_, KeyCode::Down) => app.viewer.scroll_down(1),
        (_, KeyCode::Char('k')) | (_, KeyCode::Up) => app.viewer.scroll_up(1),
        (_, KeyCode::Char('g')) | (_, KeyCode::Home) => app.viewer.scroll_to_top(),
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (_, KeyCode::End) => {
            app.viewer.scroll_to_bottom();
        }
        (KeyModifiers::CONTROL, KeyCode::Char('d')) | (_, KeyCode::PageDown) => {
            app.viewer.scroll_down(20);
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) | (_, KeyCode::PageUp) => {
            app.viewer.scroll_up(20);
        }
        (_, KeyCode::Char('e')) => {
            open_in_editor(app)?;
        }
        _ => {}
    }
    Ok(())
}

fn handle_search_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
            app.search.cursor_pos += 1;
        }
        KeyCode::Backspace => {
            if app.search.query.pop().is_some() {
                app.search.cursor_pos = app.search.cursor_pos.saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            // Execute search
            if !app.search.query.is_empty() {
                let results = app.storage.search(&app.search.query, 50).unwrap_or_default();
                app.search.results = results;
                app.search.selected = 0;
            }

            // If results exist and one is selected, open it
            if !app.search.results.is_empty() {
                let result = &app.search.results[app.search.selected];
                let path = result.file_path.clone();
                if let Ok(content) = app.storage.read_file(&path) {
                    let text = String::from_utf8_lossy(&content).to_string();
                    app.viewer.load_content(&path, &text);
                }
                app.mode = Mode::Normal;
                app.focus = Focus::Viewer;
            }
        }
        KeyCode::Down => {
            if app.search.selected + 1 < app.search.results.len() {
                app.search.selected += 1;
            }
        }
        KeyCode::Up => {
            app.search.selected = app.search.selected.saturating_sub(1);
        }
        _ => {}
    }
    Ok(())
}

fn handle_find_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.find.clear();
        }
        KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE && !app.find.query.is_empty() => {
            // Only treat 'n' as next-match when we have a query
            // Otherwise it's a regular character input
            app.find.next_match();
            // Scroll viewer to current match
            if let Some(&(line, _)) = app.find.matches.get(app.find.current_match) {
                app.viewer.scroll_offset = line.saturating_sub(5);
            }
        }
        KeyCode::Char('N') if !app.find.query.is_empty() => {
            app.find.prev_match();
            if let Some(&(line, _)) = app.find.matches.get(app.find.current_match) {
                app.viewer.scroll_offset = line.saturating_sub(5);
            }
        }
        KeyCode::Enter => {
            app.find.next_match();
            if let Some(&(line, _)) = app.find.matches.get(app.find.current_match) {
                app.viewer.scroll_offset = line.saturating_sub(5);
            }
        }
        KeyCode::Char(c) => {
            app.find.query.push(c);
            app.find.cursor_pos += 1;
            app.find.update_matches(&app.viewer.lines);
        }
        KeyCode::Backspace => {
            if app.find.query.pop().is_some() {
                app.find.cursor_pos = app.find.cursor_pos.saturating_sub(1);
                app.find.update_matches(&app.viewer.lines);
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_mouse(app: &mut TuiApp, mouse: MouseEvent) -> Result<()> {
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            match app.focus {
                Focus::FileTree => app.tree.move_down(),
                Focus::Viewer => app.viewer.scroll_down(3),
            }
        }
        MouseEventKind::ScrollUp => {
            match app.focus {
                Focus::FileTree => app.tree.move_up(),
                Focus::Viewer => app.viewer.scroll_up(3),
            }
        }
        _ => {}
    }
    Ok(())
}

fn open_in_editor(app: &mut TuiApp) -> Result<()> {
    let path = match &app.viewer.file_path {
        Some(p) => app.forge_root.join(p),
        None => return Ok(()),
    };

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    // Leave TUI temporarily
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;

    // Spawn editor
    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status();

    // Re-enter TUI
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;

    // Reload file (may have changed)
    if status.is_ok() {
        if let Some(ref viewer_path) = app.viewer.file_path.clone() {
            if let Ok(content) = app.storage.read_file(viewer_path) {
                let text = String::from_utf8_lossy(&content).to_string();
                app.viewer.load_content(viewer_path, &text);
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Wire input handler into main.rs event loop**

Replace the event handling in `run()`:

```rust
mod input;

fn run(terminal: &mut DefaultTerminal, app: &mut app::TuiApp) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(16))? {
            let evt = event::read()?;
            input::handle_event(app, evt)?;
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add input handler with vim/arrow/mouse navigation and editor launch"
```

---

### Task 8: Wire all UI together

**Files:**
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Complete the render function**

Update `ui/mod.rs` to call all sub-renderers:

```rust
mod file_tree;
mod status_bar;
mod viewer;

use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

use crate::app::TuiApp;

pub fn render(frame: &mut Frame, app: &mut TuiApp) {
    let [body, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [tree_area, viewer_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ])
    .areas(body);

    file_tree::render(frame, app, tree_area);
    viewer::render(frame, app, viewer_area);
    status_bar::render(frame, app, status);
}
```

- [ ] **Step 2: Verify full interaction**

Run against test forge:
- Arrow keys and j/k navigate file tree
- Enter opens files in viewer
- Scroll in viewer works
- Tab switches focus
- q quits
- e opens $EDITOR

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): wire all UI panels together"
```

---

## Phase 5: Search + Find

### Task 9: Add search overlay

**Files:**
- Create: `crates/nexus-tui/src/ui/search.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Create search.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/search.rs`:

```rust
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::TuiApp;

pub fn render(frame: &mut Frame, app: &TuiApp) {
    let popup_area = centered_rect(60, 50, frame.area());

    // Clear background
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Search (Enter to search, Esc to close) ");

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let [query_area, results_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);

    // Query line
    let query_line = Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.search.query, Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(query_line), query_area);

    // Results
    if app.search.results.is_empty() && !app.search.query.is_empty() {
        frame.render_widget(
            Paragraph::new("No results. Press Enter to search.")
                .style(Style::default().fg(Color::DarkGray)),
            results_area,
        );
    } else {
        let items: Vec<ListItem> = app
            .search
            .results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let style = if i == app.search.selected {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(&result.file_path, style),
                    Span::styled(
                        format!("  ({:.2})", result.score),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let count = format!(" {} results ", app.search.results.len());
        let list = List::new(items)
            .block(Block::default().title(count));
        frame.render_widget(list, results_area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, center_y, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, popup, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(center_y);
    popup
}
```

- [ ] **Step 2: Wire into ui/mod.rs**

Add `mod search;` and after the normal panels render:

```rust
if app.mode == Mode::Search {
    search::render(frame, app);
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add search overlay with Tantivy-backed results"
```

---

### Task 10: Add in-file find bar

**Files:**
- Create: `crates/nexus-tui/src/ui/find.rs`
- Modify: `crates/nexus-tui/src/ui/mod.rs`

- [ ] **Step 1: Create find.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-tui/src/ui/find.rs`:

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::TuiApp;

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let match_info = if app.find.matches.is_empty() {
        if app.find.query.is_empty() {
            String::new()
        } else {
            "no matches".to_string()
        }
    } else {
        format!(
            "{}/{}",
            app.find.current_match + 1,
            app.find.matches.len()
        )
    };

    let line = Line::from(vec![
        Span::styled(" Find: ", Style::default().fg(Color::Cyan)),
        Span::styled(&app.find.query, Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(match_info, Style::default().fg(Color::DarkGray)),
    ]);

    let bar = Paragraph::new(line).style(Style::default().bg(Color::Rgb(30, 40, 50)));
    frame.render_widget(bar, area);
}
```

- [ ] **Step 2: Wire into ui/mod.rs**

Add `mod find;` and modify the layout to include a find bar when active:

```rust
if app.mode == Mode::Find {
    // Steal the last line of the viewer area for the find bar
    let [viewer_body, find_bar] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(viewer_area);

    viewer::render(frame, app, viewer_body);
    find::render(frame, app, find_bar);
} else {
    viewer::render(frame, app, viewer_area);
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-tui/
git commit -m "feat(tui): add in-file find bar with match highlighting"
```

---

## Phase 6: Polish + Verification

### Task 11: Ensure search index is built on first search

**Files:**
- Modify: `crates/nexus-tui/src/input.rs`

- [ ] **Step 1: Add search index rebuild before first search**

In `handle_search_key`, before the search call, add:

```rust
// Rebuild search index if needed (first search in session)
app.storage.rebuild_search_index().ok();
```

This ensures the Tantivy index has content on the first `Ctrl+F` search.

- [ ] **Step 2: Commit**

```bash
git add crates/nexus-tui/
git commit -m "fix(tui): rebuild search index before first Tantivy query"
```

---

### Task 12: Final verification

**Files:** (none — verification only)

- [ ] **Step 1: Build**

Run: `cargo build -p nexus-tui`

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p nexus-tui -- -D warnings`
Fix any warnings.

- [ ] **Step 3: Full workspace tests**

Run: `cargo nextest run --workspace`
Expected: all tests still pass (TUI has no tests — it's a visual binary).

- [ ] **Step 4: Manual smoke test**

```bash
TMPFORGE=$(mktemp -d)
./target/debug/nexus --forge-path "$TMPFORGE" forge init
./target/debug/nexus --forge-path "$TMPFORGE" content create notes/readme.md --content "# Readme\n\nThis is a test forge.\n\n## Features\n\n- File browsing\n- Search via [[wikilinks]]\n- #tags support\n\n\`\`\`rust\nfn main() {\n    println!(\"hello\");\n}\n\`\`\`"
./target/debug/nexus --forge-path "$TMPFORGE" content create notes/todo.md --content "# Todo\n\n- Build TUI\n- Add search\n- Ship it"
./target/debug/nexus-tui "$TMPFORGE"
```

Verify:
- File tree shows notes/readme.md and notes/todo.md
- j/k navigate tree, Enter opens file
- Viewer shows syntax-highlighted markdown
- Tab switches focus, j/k scroll viewer
- Ctrl+F opens search, type "todo", Enter searches, Enter opens result
- / opens find bar, type "test", matches highlighted
- e opens $EDITOR
- q quits cleanly

- [ ] **Step 5: Cleanup**

```bash
rm -rf "$TMPFORGE"
```

---

## Summary

12 tasks across 6 phases produce:
- `nexus-tui` binary crate with `nexus-tui` executable
- Two-panel layout: file tree (25%) + viewer (75%) + status bar
- File tree with expand/collapse, vim/arrow/mouse navigation
- Read-only viewer with markdown syntax highlighting (headings, code, wikilinks, tags)
- Line numbers in gutter
- Status bar with mode, file path, scroll position, file count
- Search overlay (Ctrl+F) backed by Tantivy full-text search
- In-file find (/) with match highlighting and n/N navigation
- $EDITOR integration (e key) with terminal suspend/resume
- Input handling: vim keys, arrow keys, mouse (click, scroll)
