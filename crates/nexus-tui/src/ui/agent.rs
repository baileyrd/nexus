//! BL-132 — agent panel.
//!
//! Renders the right pane as a transcript + goal-input prompt when
//! `app.agent.active` is set. Mirrors the AI panel's layout
//! (transcript / status line / prompt row); typed `AgentLine` rows
//! drive per-kind styling so goal / round / tool-call / outcome rows
//! are visually distinct. The approval modal sits separately in
//! `ui::agent_approval`.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{AgentLineKind, Mode, TuiApp};

pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let title = make_title(app);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightBlue));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
    let mode_hint = if app.mode == Mode::AgentInput {
        " — Esc: leave input · Enter: submit"
    } else {
        " — i: input · g: close"
    };
    Line::from(vec![
        Span::styled(
            " AGENT ",
            Style::default()
                .bg(Color::LightBlue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "com.nexus.agent · interactive",
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
    if app.agent.lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Press `i` to type a goal. The agent runs in interactive mode — destructive tool \
             calls pause for your approval before dispatch.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for entry in &app.agent.lines {
            match entry.kind {
                AgentLineKind::Goal => {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "goal",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": "),
                        Span::styled(entry.text.clone(), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(""));
                }
                AgentLineKind::Round => {
                    for body in entry.text.split('\n') {
                        lines.push(Line::from(Span::styled(
                            body.to_string(),
                            Style::default().fg(Color::White),
                        )));
                    }
                    lines.push(Line::from(""));
                }
                AgentLineKind::ToolCall => {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", entry.text),
                        Style::default().fg(Color::Cyan),
                    )));
                }
                AgentLineKind::Outcome => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        entry.text.clone(),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                AgentLineKind::Error => {
                    lines.push(Line::from(Span::styled(
                        format!("error: {}", entry.text),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
    }
    let total = lines.len() as u16;
    let view_height = area.height;
    let max_scroll = total.saturating_sub(view_height);
    let scroll = app.agent.scroll.min(max_scroll);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, area);
}

fn render_status_line(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let line = if app.agent.pending.is_some() {
        Line::from(Span::styled(
            "  awaiting approval (see modal)  ",
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ))
    } else if app.agent.in_flight {
        Line::from(Span::styled(
            "  agent running…  ",
            Style::default()
                .bg(Color::LightBlue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ))
    } else if let Some(err) = &app.agent.last_error {
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
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
    );
    let text = Span::styled(app.agent.input.clone(), Style::default().fg(Color::White));
    let cursor = if app.mode == Mode::AgentInput {
        Span::styled("█", Style::default().fg(Color::LightBlue))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![prefix, text, cursor]);
    frame.render_widget(Paragraph::new(line), area);
}
