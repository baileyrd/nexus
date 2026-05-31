//! AIG-07 — AI chat panel.
//!
//! Renders the right pane as a transcript + prompt when the panel is
//! visible. Same priority rule as the terminal panel: when active, it
//! takes over the right pane (the viewer / tasks / backlinks step
//! aside). The transcript scrolls; the prompt sits on the bottom row
//! with a one-line status header showing the configured provider.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{AiRole, Mode, TuiApp};

/// Render the AI chat panel into `area`. Mirrors the top-level
/// [`crate::ui::render`] dispatch — the caller decides when this is
/// invoked (right pane, panel-active branch).
pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let title = make_title(app);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Three rows: transcript / status / prompt. Status is one line
    // for "thinking…" / errors; prompt is one line for the input.
    let [transcript_area, status_area, prompt_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    render_transcript(frame, app, transcript_area);
    render_status_line(frame, app, status_area);
    render_prompt(frame, app, prompt_area);
}

fn make_title(app: &TuiApp) -> Line<'static> {
    let provider = app.ai.provider_status.as_deref().unwrap_or("(loading…)");
    let mode_hint = if app.mode == Mode::AiInput {
        " — Esc: leave input · Enter: submit"
    } else {
        " — i: input · a: close"
    };
    Line::from(vec![
        Span::styled(
            " AI ",
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            provider.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(mode_hint, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
    ])
}

fn render_transcript(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if app.ai.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "Press `i` to start typing. The model uses RAG-grounded retrieval (`com.nexus.ai::stream_ask`) with the full transcript, so questions are answered against your forge and prior turns.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for msg in &app.ai.messages {
            let prefix = match msg.role {
                AiRole::User => Span::styled(
                    "you",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                AiRole::Assistant => Span::styled(
                    "nexus",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
            };
            lines.push(Line::from(vec![prefix, Span::raw(":")]));
            for body_line in msg.text.split('\n') {
                lines.push(Line::from(Span::styled(
                    body_line.to_string(),
                    Style::default().fg(Color::White),
                )));
            }
            lines.push(Line::from(""));
        }
    }
    // Auto-pin to the bottom: clamp scroll into a sensible window.
    let total = lines.len() as u16;
    let view_height = area.height;
    let max_scroll = total.saturating_sub(view_height);
    let scroll = app.ai.scroll.min(max_scroll);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, area);
}

fn render_status_line(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let line = if app.ai.in_flight {
        // AIG-07 — once the first chunk lands, swap the indicator
        // from "thinking…" to "streaming…" so the user knows tokens
        // are flowing rather than stalled mid-call.
        let label = match &app.ai.streaming {
            Some(s) if s.started => "  streaming…  ",
            _ => "  thinking…  ",
        };
        Line::from(Span::styled(
            label,
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ))
    } else if let Some(err) = &app.ai.last_error {
        Line::from(Span::styled(
            format!(" error: {err} "),
            Style::default().bg(Color::Red).fg(Color::White),
        ))
    } else {
        Line::from(Span::styled("", Style::default()))
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_prompt(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let prefix = Span::styled(
        " > ",
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );
    let text = Span::styled(app.ai.input.clone(), Style::default().fg(Color::White));
    let cursor = if app.mode == Mode::AiInput {
        Span::styled("█", Style::default().fg(Color::Blue))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![prefix, text, cursor]);
    frame.render_widget(Paragraph::new(line), area);
}
