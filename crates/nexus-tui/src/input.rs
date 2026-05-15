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
    // Log every event we receive before any mode filter, so the file
    // log captures cases where we're dropping events (e.g. because
    // `kind != Press` or `app.mode` flipped unexpectedly). Does not
    // fire for `_ => {}` branches below, but the breadcrumb for
    // `Event::Key` above catches the case we actually care about.
    if let Event::Key(key) = &event {
        tracing::debug!(
            kind = ?key.kind,
            modifiers = ?key.modifiers,
            code = ?key.code,
            mode = ?app.mode,
            "raw key event",
        );
    }
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match app.mode {
            Mode::Normal => handle_normal_key(app, key)?,
            Mode::Search => handle_search_key(app, key)?,
            Mode::Find => handle_find_key(app, key)?,
            Mode::Terminal => handle_terminal_key(app, key)?,
            Mode::AiInput => handle_ai_input_key(app, key)?,
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
        // Toggle task list view
        (KeyModifiers::NONE, KeyCode::Char('t')) => {
            app.task_view.toggle();
            if app.task_view.active {
                app.load_tasks();
            }
            return Ok(());
        }
        // BL-137 — toggle the kernel-stats overlay (Shift+K). Read-only
        // snapshot of the BL-093 metrics registry; refetches each open
        // so the panel reflects current point-in-time state rather
        // than the last view.
        (KeyModifiers::SHIFT, KeyCode::Char('K')) | (_, KeyCode::Char('K')) => {
            app.toggle_kernel_stats();
            return Ok(());
        }
        // Open terminal panel + switch to Terminal mode so keystrokes
        // start flowing into the PTY. Bound on a capital `T` so it
        // doesn't collide with the task-view toggle.
        (KeyModifiers::SHIFT, KeyCode::Char('T')) | (_, KeyCode::Char('T')) => {
            app.open_terminal();
            if app.terminal.active {
                app.mode = Mode::Terminal;
            }
            return Ok(());
        }
        // AIG-07 — toggle the AI chat panel + drop straight into
        // input mode on first activation so the user can start
        // typing immediately. `a` is unused elsewhere.
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            app.toggle_ai_panel();
            if app.ai.active {
                app.mode = Mode::AiInput;
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
                match nexus_bootstrap::storage::search(&app.runtime, &app.rt, &query, 50) {
                    Ok(results) => {
                        app.search.results = results;
                        app.search.selected = 0;
                        // If there are results, open the top one in the viewer.
                        if let Some(top) = app.search.results.first() {
                            let path = top.file_path.clone();
                            let bytes = nexus_bootstrap::storage::read_file(&app.runtime, &app.rt, &path);
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

// ── Terminal mode ─────────────────────────────────────────────────────────────

fn handle_terminal_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    // Diagnostic: every key event the Terminal-mode handler sees is
    // logged to a small ring shown in the panel title. If typed
    // characters don't reach the buffer, this tells us whether the
    // keystrokes arrived here at all and what crossterm labelled
    // them. The ring is capped at 5 entries so the title stays tidy.
    app.terminal
        .log_key(format!("{:?}+{:?}", key.modifiers, key.code));
    tracing::debug!(
        modifiers = ?key.modifiers,
        code = ?key.code,
        kind = ?key.kind,
        "terminal key",
    );

    match (key.modifiers, key.code) {
        // Exit Terminal mode back to Normal. Leaves the session alive
        // so users can come back to it. Use `Ctrl+D` to actually kill.
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.mode = Mode::Normal;
            app.hide_terminal();
        }
        // Send Ctrl+C as SIGINT to the child process group. We do this
        // via raw input (0x03) rather than the IDE quit path because
        // Ctrl+C in a terminal panel must target the running command,
        // not the TUI itself.
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.send_terminal_raw(&[0x03]);
        }
        // Ctrl+D — EOF / session close. Kills the session and drops
        // back to Normal mode.
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            app.kill_terminal();
            app.mode = Mode::Normal;
        }
        // Enter flushes the input buffer as one command.
        (_, KeyCode::Enter) => {
            app.submit_terminal_input();
        }
        // Backspace edits the input buffer.
        (_, KeyCode::Backspace) => {
            app.terminal.input.pop();
        }
        // Any printable character: append to the buffer. We do NOT
        // forward keystrokes live to the PTY — we line-buffer on the
        // TUI side so the panel is a shell-prompt shape, not a raw
        // xterm. Full PTY pass-through is a future slice once we've
        // solved in-app terminfo rendering.
        //
        // The broad `(_, KeyCode::Char(c))` matches *every* modifier
        // combination that still produces a printable char —
        // including `SHIFT+a` → `Char('A')` with `modifiers = SHIFT`.
        // On terminals that set modifier flags (CapsLock, some kitty
        // keyboard protocols) plain letters would otherwise slip
        // past a `(NONE, Char(_))` arm.
        (_, KeyCode::Char(c)) => {
            app.terminal.input.push(c);
        }
        _ => {}
    }
    Ok(())
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
    if let Ok(bytes) = nexus_bootstrap::storage::read_file(&app.runtime, &app.rt, &path) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        app.viewer.load_content(path, text);
    }

    Ok(())
}

// ── AIG-07 — AI input mode ───────────────────────────────────────────────────

fn handle_ai_input_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};

    match (key.modifiers, key.code) {
        // Esc closes input mode but keeps the panel open. Hitting
        // Esc again from Normal mode (with focus on the AI panel)
        // hides the panel; that's the FileTree/Viewer-style escape
        // hatch.
        (_, KeyCode::Esc) => {
            app.mode = Mode::Normal;
            return Ok(());
        }
        // AIG-07 — non-blocking submit. `submit_ai` spawns the IPC
        // call as a tokio task, subscribes to the chunk topic, and
        // returns immediately. `pump_ai` (in the render loop) drains
        // chunks between frames so the user sees tokens stream in.
        (KeyModifiers::NONE, KeyCode::Enter) => {
            app.submit_ai();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Backspace)
        | (KeyModifiers::SHIFT, KeyCode::Backspace) => {
            app.ai.backspace();
            return Ok(());
        }
        // Plain printable input. Excludes the modifier-prefixed
        // arrow keys etc. that crossterm reports as Char-with-
        // -modifier; those fall through to the catch-all below.
        (KeyModifiers::NONE, KeyCode::Char(c))
        | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.ai.insert_char(c);
            return Ok(());
        }
        _ => {}
    }
    Ok(())
}
