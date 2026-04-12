# Nexus TUI Design Spec

**Version:** 1.0
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** `nexus-tui` binary crate — a ratatui terminal UI for browsing, searching, and viewing forge content. Two-panel layout (file tree + read-only viewer), forge-wide search, in-file find, vim + arrow + mouse input.

---

## 1. Architecture Overview

New binary crate `nexus-tui` alongside the existing `nexus-cli`. Both share the same library crates (`nexus-kernel`, `nexus-storage`, `nexus-plugins`, `nexus-security`, `nexus-types`). The TUI is a rendering frontend — no business logic lives here.

```
nexus-kernel, nexus-storage, nexus-plugins, nexus-security
                    |
            +-------+-------+
            |       |       |
        nexus-cli  nexus-tui  (nexus-gui, future)
```

Uses `ratatui` 0.28+ with `crossterm` backend. Standard event loop: render → poll → dispatch → update state → repeat.

---

## 2. Crate Structure

```
crates/nexus-tui/
├── Cargo.toml
└── src/
    ├── main.rs          # entry point, terminal setup/teardown, event loop
    ├── app.rs           # TuiApp state: mode, focus, tree state, viewer state
    ├── input.rs         # keyboard + mouse input dispatch
    └── ui/
        ├── mod.rs       # top-level render function
        ├── file_tree.rs # left panel: directory tree widget
        ├── viewer.rs    # right panel: file content with syntax highlighting
        ├── status_bar.rs # bottom line: mode, file, stats, help hint
        ├── search.rs    # Ctrl+F overlay: forge-wide Tantivy search
        └── find.rs      # / overlay: in-file text search with highlighting
```

---

## 3. Dependencies

New workspace dependencies (root `Cargo.toml`):

| Crate | Version | Purpose |
|---|---|---|
| `ratatui` | 0.28+ | TUI framework (widgets, layout, rendering) |
| `crossterm` | 0.28+ | Terminal backend (raw mode, events, alternate screen) |

`nexus-tui` depends on: `nexus-storage`, `nexus-plugins`, `nexus-kernel`, `nexus-types`, `ratatui`, `crossterm`, `serde_json`, `tracing`, `tracing-subscriber`, `anyhow`.

---

## 4. Layout

Two-panel with status bar. File tree takes 25% width, viewer takes 75%.

```
┌─ FILE TREE ──┬─ VIEWER ──────────────────────┐
│ ▼ notes/     │ # Welcome                     │
│   welcome.md │                                │
│   api.md     │ Hello from Nexus.              │
│ ▶ projects/  │                                │
│              │ ## Getting Started             │
│              │                                │
│              │ Create files, search, browse.  │
├──────────────┴────────────────────────────────┤
│ NORMAL │ notes/welcome.md │ 12 files │ Ctrl+? │
└───────────────────────────────────────────────┘
```

Implemented via ratatui's `Layout::horizontal` for the two panels and `Layout::vertical` for the overall structure (panels + status bar).

---

## 5. App State

```rust
pub struct TuiApp {
    pub mode: Mode,
    pub focus: Focus,
    pub tree: TreeState,
    pub viewer: ViewerState,
    pub search: SearchState,
    pub find: FindState,
    pub storage: StorageEngine,
    pub should_quit: bool,
}

pub enum Mode {
    Normal,
    Search,  // Ctrl+F forge-wide search overlay
    Find,    // / in-file find
}

pub enum Focus {
    FileTree,
    Viewer,
}
```

### TreeState

```rust
pub struct TreeState {
    pub entries: Vec<TreeEntry>,
    pub selected: usize,
    pub scroll_offset: usize,
}

pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub depth: usize,
}
```

Built from `StorageEngine::query_files` + filesystem directory listing. Directories expanded/collapsed in-place.

### ViewerState

```rust
pub struct ViewerState {
    pub file_path: Option<String>,
    pub content: String,
    pub lines: Vec<String>,
    pub scroll_offset: usize,
    pub line_count: usize,
}
```

Content loaded via `StorageEngine::read_file`. Lines split for rendering. Scroll position tracked.

### SearchState

```rust
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected: usize,
    pub cursor_pos: usize,
}
```

Results from `StorageEngine::search(query, 50)`. Updated on each keystroke (debounced or on Enter).

### FindState

```rust
pub struct FindState {
    pub query: String,
    pub matches: Vec<(usize, usize)>,  // (line, col) pairs
    pub current_match: usize,
    pub cursor_pos: usize,
}
```

Simple substring search within the current file's content.

---

## 6. Input Handling

### File tree (Focus::FileTree)

| Key | Action |
|---|---|
| `j` / `Down` / mouse scroll | Move cursor down |
| `k` / `Up` / mouse scroll | Move cursor up |
| `Enter` / `l` / `Right` / click | Open file or expand directory |
| `h` / `Left` | Collapse directory |
| `Tab` | Switch focus to viewer |
| `Ctrl+F` | Open search overlay |
| `/` | Open in-file find |
| `q` / `Ctrl+C` | Quit |

### Viewer (Focus::Viewer)

| Key | Action |
|---|---|
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `g` / `Home` | Jump to top |
| `G` / `End` | Jump to bottom |
| `Page Down` / `Ctrl+D` | Page down |
| `Page Up` / `Ctrl+U` | Page up |
| `e` | Open in `$EDITOR` (suspend TUI) |
| `Tab` | Switch focus to file tree |
| `Ctrl+F` | Open search overlay |
| `/` | Open in-file find |
| `q` / `Ctrl+C` | Quit |

### Search overlay (Mode::Search)

| Key | Action |
|---|---|
| Characters | Append to query |
| `Backspace` | Delete last character |
| `Down` / `Ctrl+N` | Move selection down |
| `Up` / `Ctrl+P` | Move selection up |
| `Enter` | Open selected result in viewer, close search |
| `Esc` | Close search overlay |

### In-file find (Mode::Find)

| Key | Action |
|---|---|
| Characters | Append to query, update highlights |
| `Backspace` | Delete last character |
| `Enter` / `n` | Jump to next match |
| `N` | Jump to previous match |
| `Esc` | Close find bar |

### Mouse

| Action | Effect |
|---|---|
| Click on tree entry | Select entry, open if file |
| Click on panel | Switch focus to that panel |
| Scroll wheel | Scroll the focused panel |

---

## 7. Viewer Features

### Markdown syntax highlighting

Simple pattern-based highlighting applied per line (not comrak AST — too heavy for display):

| Pattern | Style |
|---|---|
| `# Heading` (1-6 `#`) | Bold + blue, scaled by level |
| `` `inline code` `` | Gray background |
| ```` ``` ```` code fence | Dim + different background |
| `[[wikilink]]` | Cyan underline |
| `#tag` | Green |
| `> blockquote` | Italic + dim |
| `- list item` | Normal with bullet |
| `---` | Horizontal rule (dim line) |

### Line numbers

Gutter on the left side of the viewer showing line numbers in dim style.

### Open in $EDITOR

When `e` is pressed:
1. Restore terminal from raw mode
2. Spawn `$EDITOR <file_path>` (or `$VISUAL`, fallback to `vi`)
3. Wait for editor to exit
4. Re-enter raw mode, redraw
5. Reload file content (may have changed)

---

## 8. Search Overlay (Ctrl+F)

Centered overlay covering ~60% of the screen:

```
┌─ Search ────────────────────────────┐
│ Query: welco█                       │
│─────────────────────────────────────│
│ > notes/welcome.md     (0.95)       │
│   notes/api-design.md  (0.42)       │
│   projects/readme.md   (0.31)       │
│                                     │
│                    3 results         │
└─────────────────────────────────────┘
```

Results from `StorageEngine::search()`. Rebuilds the Tantivy index on first search if needed. Search triggered on Enter (not live — Tantivy queries are not instant enough for keystroke-level updates).

---

## 9. In-File Find (/)

Find bar at the bottom of the viewer panel:

```
│ Create files, search, browse.  │
│                                │
│ Find: welco█  3/12 matches     │
└────────────────────────────────┘
```

All matches highlighted in the viewer content. Current match has a distinct highlight. `n`/`N` cycle through matches. Match count updates live as you type.

---

## 10. Event Loop

```rust
fn run(terminal: &mut Terminal, app: &mut TuiApp) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if crossterm::event::poll(Duration::from_millis(16))? {
            let event = crossterm::event::read()?;
            input::handle_event(app, event)?;
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
```

Tick rate: ~60fps (16ms poll timeout). Terminal setup: crossterm raw mode + alternate screen + mouse capture. Teardown restores terminal on exit (including on panic via a drop guard).

---

## 11. main.rs Flow

1. Parse CLI args (minimal: `--forge-path`, `--verbose`)
2. Set up tracing
3. Open `StorageEngine` for the forge
4. Build initial `TuiApp` (scan file tree, no file open)
5. Enter alternate screen, enable raw mode, enable mouse
6. Run event loop
7. On exit: disable mouse, leave alternate screen, disable raw mode

---

## 12. Deferred

| Item | Rationale | Revisit |
|---|---|---|
| Text editing | Read-only viewer first; `$EDITOR` for edits | v2 |
| Split panes / multiple tabs | Single file viewer first | v2 |
| Plugin status panel | No visual need yet | When plugins have UI |
| Markdown preview (rendered) | Highlighted source is sufficient | v2 |
| File create/delete from TUI | Use CLI commands | v2 |
| Wikilink navigation (click to follow) | Requires link resolution in viewer | v2 |
| Configurable keybindings | Hardcoded defaults first | v2 |
| Theme customization | Hardcoded dark theme first | v2 |
