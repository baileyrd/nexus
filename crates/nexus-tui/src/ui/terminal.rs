//! Terminal panel renderer — PRD-09 §14.
//!
//! The panel replaces the viewer when `app.terminal.active` is true
//! (same pattern as `TaskViewState`). Layout is a vertical split:
//! the upper region shows the line-buffered output, the lower
//! single-line region shows the user's in-progress input with a
//! cursor block.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{Mode, TuiApp};

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let [output_area, input_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    render_output(frame, app, output_area);
    render_input(frame, app, input_area);
}

fn render_output(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // Title carries the session id (shortened) + line count so users
    // can sanity-check the PTY is producing output. No ANSI colour
    // rendering in this slice — lines arrive ANSI-stripped via the
    // `OutputLine.content` field, which is the right default for a
    // shell-style interaction view.
    let title = match &app.terminal.session_id {
        Some(id) => {
            let short = id.get(..8).unwrap_or(id);
            // While debugging input routing, show the last few key
            // events the handler saw so we can tell at a glance
            // whether crossterm is delivering them to us.
            let keys = if app.terminal.key_log.is_empty() {
                String::new()
            } else {
                format!("  keys: [{}]", app.terminal.key_log.join(" → "))
            };
            format!(
                " Terminal [{short}]  {} lines{keys} ",
                app.terminal.lines.len(),
            )
        }
        None => " Terminal (starting…) ".into(),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Fit the most recent lines into the visible area. Oldest drops
    // first — matches the way shells scroll.
    let visible_rows = inner.height as usize;
    let total = app.terminal.lines.len();
    let start = total.saturating_sub(visible_rows);
    let items: Vec<ListItem> = app
        .terminal
        .lines
        .iter()
        .skip(start)
        .map(|l| ListItem::new(Line::from(l.content.as_str())))
        .collect();
    let list = List::new(items).style(Style::default().fg(Color::Gray));
    frame.render_widget(list, inner);
}

fn render_input(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let cursor = if app.mode == Mode::Terminal {
        Span::styled("█", Style::default().fg(Color::Cyan))
    } else {
        // When we're not focused on the terminal, render a static
        // placeholder so the user knows how to re-enter input mode.
        Span::styled(" ", Style::default())
    };
    let hint = if app.mode == Mode::Terminal {
        ""
    } else {
        "  (press T to type, Esc to leave)"
    };
    let line = Line::from(vec![
        Span::styled(
            " $ ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.terminal.input.as_str(),
            Style::default().fg(Color::White),
        ),
        cursor,
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 25, 35))),
        area,
    );
}
