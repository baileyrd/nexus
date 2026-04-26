# Phase 5: Polish & Secondary Features Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** HTML export, enhanced TUI, watcher reconcile

---

## A. HTML Export

New file `crates/nexus-storage/src/export.rs`.

`export_to_html(content: &str, title: &str) -> String` — renders markdown to a complete HTML document:
- Uses comrak's HTML rendering mode with safe mode enabled (no raw HTML passthrough)
- Wraps output in `<!DOCTYPE html>` with embedded CSS for callouts, code blocks, tasks, wikilinks
- Wikilinks rendered as `<a data-wikilink="true" href="target.html">text</a>`
- Callouts rendered as `<div class="callout callout-{type}">` blocks
- Tasks rendered as `<input type="checkbox" disabled>` elements

CLI: `nexus content export <path> [--output file.html]` — export a single note. Prints to stdout if no `--output`.

No new dependencies — comrak already supports HTML rendering.

### Files
- Create: `crates/nexus-storage/src/export.rs`
- Modify: `crates/nexus-storage/src/lib.rs` — register module, pub use
- Modify: `crates/nexus-cli/src/main.rs` — add Export variant
- Modify: `crates/nexus-cli/src/commands/content.rs` — add export handler

---

## B. Enhanced TUI

Modifications to `crates/nexus-tui/src/`:

### B1. Backlinks Panel
When a file is selected, show a toggleable bottom panel listing backlinks. Toggle with `b` key. Uses `StorageEngine::backlinks()`. Shows source path and link text. Enter on a backlink navigates to that file.

### B2. Task List View
New view mode toggled with `t` key. Shows all tasks across the forge using `StorageEngine::query_tasks()`. Displays checkbox state, content, and file:line. Tasks replace the viewer content when active.

### B3. Fuzzy File Search
Enhance the existing search overlay to filter the file tree as the user types. Simple case-insensitive substring match on file paths. No new dependencies.

### B4. Status Bar Enhancements
Show file count, link count (from graph stats), and pending task count in the status bar. Queried on startup and refreshed on file changes.

### Files
- Modify: `crates/nexus-tui/src/app.rs` — add backlinks state, task view mode, stats
- Modify: `crates/nexus-tui/src/ui/` — backlinks panel widget, task list widget
- Modify: `crates/nexus-tui/src/input.rs` — handle `b` and `t` key bindings

The TUI crate needs `nexus-storage` as a dependency (check if already present).

---

## C. Watcher Reconcile

Modify `crates/nexus-storage/src/lib.rs`.

Add `process_watcher_events(&self) -> Result<usize, StorageError>` to `StorageEngine`:
- Drains all pending events from the watcher receiver (non-blocking `try_recv` loop)
- For each `FileCreated`/`FileModified`: read file from disk, re-parse, re-index, update graph
- For each `FileDeleted`: remove from index and graph
- For each `FileRenamed`: delete old path, create new path
- Returns number of events processed

The existing `nexus watch` CLI command calls this in its event loop. The TUI can call it on a periodic tick.

### Files
- Modify: `crates/nexus-storage/src/lib.rs` — add `process_watcher_events` method

---

## Testing

- **HTML Export**: unit test rendering a markdown string with headings, callouts, tasks, wikilinks → verify HTML output contains expected elements
- **TUI**: no automated tests (TUI is visual — test manually)
- **Watcher Reconcile**: integration test — init forge, write file externally (bypass StorageEngine), call `process_watcher_events`, verify file is indexed

---

## Out of Scope

- Canvas support (.canvas JSON format)
- Graph visualization in TUI
- Multi-file HTML export with navigation sidebar
- Event-driven embedding updates (manual `nexus ai embed` for now)
