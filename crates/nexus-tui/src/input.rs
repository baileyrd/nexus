//! Input event handling for nexus-tui.
//!
//! Dispatches crossterm [`Event`]s to the appropriate handler based on the
//! current [`Mode`] and [`Focus`].

use std::io;
use std::process::Command;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::app::{Focus, Mode, TuiApp};

/// Top-level event dispatcher.
///
/// Only handles `Event::Key` with `kind == Press` and `Event::Mouse`.
/// All other event variants (resize, focus, paste …) are silently ignored.
pub fn handle_event(app: &mut TuiApp, event: Event) -> Result<()> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match app.mode {
            Mode::Normal => handle_normal_key(app, key)?,
            Mode::Search => handle_search_key(app, key)?,
            Mode::Find => handle_find_key(app, key)?,
        },
        Event::Mouse(mouse) => handle_mouse(app, mouse)?,
        _ => {}
    }
    Ok(())
}

// ── Normal mode ───────────────────────────────────────────────────────────────

fn handle_normal_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    // ── Global keys (regardless of focus) ────────────────────────────────────
    match (key.modifiers, key.code) {
        // Quit
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (KeyModifiers::NONE, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        // Enter search overlay
        (KeyModifiers::CONTROL, KeyCode::Char('f')) => {
            app.search.clear();
            app.mode = Mode::Search;
            return Ok(());
        }
        // Enter find bar
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.find.clear();
            app.mode = Mode::Find;
            return Ok(());
        }
        // Toggle focus between tree and viewer
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.focus = match app.focus {
                Focus::FileTree => Focus::Viewer,
                Focus::Viewer => Focus::FileTree,
            };
            return Ok(());
        }
        // Toggle backlinks panel
        (KeyModifiers::NONE, KeyCode::Char('b')) => {
            app.backlinks.toggle();
            if app.backlinks.visible {
                app.load_backlinks();
            }
            return Ok(());
        }
        _ => {}
    }

    // ── Focus-specific keys ───────────────────────────────────────────────────
    match app.focus {
        Focus::FileTree => handle_tree_key(app, key)?,
        Focus::Viewer => handle_viewer_key(app, key)?,
    }

    Ok(())
}

fn handle_tree_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.tree.move_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.tree.move_up();
        }
        (KeyModifiers::NONE, KeyCode::Enter)
        | (KeyModifiers::NONE, KeyCode::Char('l'))
        | (KeyModifiers::NONE, KeyCode::Right) => {
            let visible = app.visible_entries();
            let is_dir = visible
                .get(app.tree.selected)
                .map(|e| e.is_dir)
                .unwrap_or(false);
            if is_dir {
                app.toggle_dir();
            } else {
                app.open_selected_file()?;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            app.toggle_dir();
        }
        _ => {}
    }
    Ok(())
}

fn handle_viewer_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.viewer.scroll_down(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.viewer.scroll_up(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) | (KeyModifiers::NONE, KeyCode::Home) => {
            app.viewer.scroll_to_top();
        }
        // Shift+G → crossterm sends 'G' with SHIFT modifier (or sometimes NONE for uppercase)
        (KeyModifiers::SHIFT, KeyCode::Char('G'))
        | (KeyModifiers::NONE, KeyCode::Char('G'))
        | (KeyModifiers::NONE, KeyCode::End) => {
            app.viewer.scroll_to_bottom();
        }
        (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.viewer.scroll_down(20);
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.viewer.scroll_up(20);
        }
        (KeyModifiers::NONE, KeyCode::Char('e')) => {
            open_in_editor(app)?;
        }
        _ => {}
    }
    Ok(())
}

// ── Search mode ───────────────────────────────────────────────────────────────

fn handle_search_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.mode = Mode::Normal;
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.search.query.pop();
            app.search.cursor_pos = app.search.cursor_pos.saturating_sub(1);
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Execute the search.
            let query = app.search.query.clone();
            if !query.is_empty() {
                match app.storage.search(&query, 50) {
                    Ok(results) => {
                        app.search.results = results;
                        app.search.selected = 0;
                        // If there are results, open the top one in the viewer.
                        if let Some(top) = app.search.results.first() {
                            let path = top.file_path.clone();
                            let bytes = app.storage.read_file(&path);
                            if let Ok(bytes) = bytes {
                                let text = String::from_utf8_lossy(&bytes).into_owned();
                                app.viewer.load_content(path, text);
                                app.focus = Focus::Viewer;
                            }
                        }
                    }
                    Err(_) => {
                        app.search.results.clear();
                    }
                }
            }
            app.mode = Mode::Normal;
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            let max = app.search.results.len().saturating_sub(1);
            if app.search.selected < max {
                app.search.selected += 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            if app.search.selected > 0 {
                app.search.selected -= 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(c))
        | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.search.query.push(c);
            app.search.cursor_pos += 1;
        }
        _ => {}
    }
    Ok(())
}

// ── Find mode ─────────────────────────────────────────────────────────────────

fn handle_find_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.find.clear();
            app.mode = Mode::Normal;
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.find.query.pop();
            app.find.cursor_pos = app.find.cursor_pos.saturating_sub(1);
            let lines = app.viewer.lines.clone();
            app.find.update_matches(&lines);
        }
        (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Char('n'))
            if !app.find.query.is_empty() =>
        {
            app.find.next_match();
            scroll_to_match(app);
        }
        // Shift+N → prev match
        (KeyModifiers::SHIFT, KeyCode::Char('N'))
        | (KeyModifiers::NONE, KeyCode::Char('N'))
            if !app.find.query.is_empty() =>
        {
            app.find.prev_match();
            scroll_to_match(app);
        }
        (KeyModifiers::NONE, KeyCode::Char(c))
        | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.find.query.push(c);
            app.find.cursor_pos += 1;
            let lines = app.viewer.lines.clone();
            app.find.update_matches(&lines);
        }
        _ => {}
    }
    Ok(())
}

/// Scroll the viewer so the current find match is visible.
fn scroll_to_match(app: &mut TuiApp) {
    if let Some(&(line_idx, _col)) = app.find.matches.get(app.find.current_match) {
        app.viewer.scroll_offset = line_idx;
    }
}

// ── Mouse handling ────────────────────────────────────────────────────────────

fn handle_mouse(app: &mut TuiApp, mouse: MouseEvent) -> Result<()> {
    match mouse.kind {
        MouseEventKind::ScrollDown => match app.focus {
            Focus::FileTree => app.tree.move_down(),
            Focus::Viewer => app.viewer.scroll_down(3),
        },
        MouseEventKind::ScrollUp => match app.focus {
            Focus::FileTree => app.tree.move_up(),
            Focus::Viewer => app.viewer.scroll_up(3),
        },
        _ => {}
    }
    Ok(())
}

// ── Editor launch ─────────────────────────────────────────────────────────────

/// Suspend the TUI, open the current file in an external editor, then resume.
fn open_in_editor(app: &mut TuiApp) -> Result<()> {
    let path = match app.viewer.file_path.clone() {
        Some(p) => p,
        None => return Ok(()),
    };

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    // Build the full path on disk.
    let full_path = app.forge_root.join(&path);

    // Leave the TUI.
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    // Spawn editor and wait.
    let _ = Command::new(&editor).arg(&full_path).status();

    // Re-enter the TUI.
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;

    // Reload file content from storage (it may have changed).
    if let Ok(bytes) = app.storage.read_file(&path) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        app.viewer.load_content(path, text);
    }

    Ok(())
}
