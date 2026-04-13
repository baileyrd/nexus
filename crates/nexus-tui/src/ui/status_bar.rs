//! Status bar widget for nexus-tui.
//!
//! Renders a single line at the bottom of the screen showing the current mode,
//! open file, scroll position, file count, and a help hint.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{Mode, TuiApp};

/// Render the status bar into `area`.
pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // ── Mode badge ────────────────────────────────────────────────────────────
    let (mode_label, mode_bg) = match app.mode {
        Mode::Normal => (" NORMAL ", Color::Green),
        Mode::Search => (" SEARCH ", Color::Yellow),
        Mode::Find => (" FIND ", Color::Cyan),
    };
    let mode_span = Span::styled(
        mode_label,
        Style::default()
            .fg(Color::Black)
            .bg(mode_bg)
            .add_modifier(Modifier::BOLD),
    );

    let sep = Span::styled(" │ ", Style::default().fg(Color::DarkGray));

    // ── File path ─────────────────────────────────────────────────────────────
    let file_span = match app.viewer.file_path.as_deref() {
        Some(path) => Span::styled(path.to_owned(), Style::default().fg(Color::White)),
        None => Span::styled("no file", Style::default().fg(Color::DarkGray)),
    };

    // ── Scroll position ───────────────────────────────────────────────────────
    let scroll_span = if app.viewer.file_path.is_some() {
        let line = app.viewer.scroll_offset + 1;
        let total = app.viewer.lines.len();
        Span::styled(
            format!(" {line}/{total}"),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::raw("")
    };

    // ── Stats ─────────────────────────────────────────────────────────────────
    let file_count = app.status_info.file_count;
    let link_count = app.status_info.link_count;
    let pending_count = app.status_info.pending_task_count;
    let stats_span = Span::styled(
        format!("{file_count} files | {link_count} links | {pending_count} tasks"),
        Style::default().fg(Color::DarkGray),
    );

    // ── Help hint ─────────────────────────────────────────────────────────────
    let help_span = Span::styled(
        "Ctrl+? help",
        Style::default().fg(Color::DarkGray),
    );

    let line = Line::from(vec![
        mode_span,
        sep.clone(),
        file_span,
        scroll_span,
        sep.clone(),
        stats_span,
        sep,
        help_span,
    ]);

    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));
    frame.render_widget(bar, area);
}
