//! BL-132 — agent approval modal.
//!
//! Renders a centered card when `app.agent.pending` is set. Mirrors
//! the CLI interactive prompt's layout (`──[ approval required ]──`
//! header, per-call rows with safe / DESTRUCTIVE / UNREGISTERED
//! badges, y/n footer hint) so users get the same risk signal
//! whether they're running `nexus agent run --interactive` or
//! driving from the TUI panel.

use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{PendingApproval, ProposedToolCall, TuiApp};

const MODAL_TIMEOUT_SECS: u64 = 1800;

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let Some(pending) = app.agent.pending.as_ref() else {
        return;
    };

    let popup = centered_rect(70, 60, area);
    frame.render_widget(Clear, popup);

    let title = format!(" Approval required · round {} ", pending.round);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // body / footer split
    let [body_area, footer_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(inner);

    let lines = build_body_lines(pending);
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, body_area);

    let footer = build_footer(pending, Instant::now());
    frame.render_widget(Paragraph::new(footer), footer_area);
}

fn build_body_lines(pending: &PendingApproval) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if !pending.text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "model: ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                first_line(&pending.text).to_string(),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(""));
    }
    if pending.calls.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no tool calls in this round)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, call) in pending.calls.iter().enumerate() {
            lines.push(render_call_header(i + 1, call));
            if let (Some(target), Some(cmd)) = (call.target_plugin_id.as_deref(), call.command_id.as_deref()) {
                lines.push(Line::from(Span::styled(
                    format!("       → {target}::{cmd}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    lines
}

fn render_call_header(idx: usize, call: &ProposedToolCall) -> Line<'static> {
    let (badge, badge_style) = match (call.requires_approval, call.registered) {
        (true, true) => (
            " DESTRUCTIVE ",
            Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        (true, false) => (
            " UNREGISTERED ",
            Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD),
        ),
        (false, _) => (
            " safe ",
            Style::default().bg(Color::Green).fg(Color::Black),
        ),
    };
    Line::from(vec![
        Span::styled(
            format!("  {idx}. "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(badge, badge_style),
        Span::raw(" "),
        Span::styled(call.name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ])
}

fn build_footer(pending: &PendingApproval, now: Instant) -> Line<'static> {
    let remaining = remaining_secs(pending.opened_at, MODAL_TIMEOUT_SECS, now);
    let timer = if remaining > 60 {
        format!("auto-reject in {}m", remaining / 60)
    } else {
        format!("auto-reject in {remaining}s")
    };
    Line::from(vec![
        Span::styled(
            "y / Enter ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("approve  ·  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "n / Esc ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled("reject  ·  ", Style::default().fg(Color::DarkGray)),
        Span::styled(timer, Style::default().fg(Color::DarkGray)),
    ])
}

/// Pure helper for the auto-reject countdown shown in the footer.
/// Saturating at 0 keeps the renderer well-defined even after the
/// pump's auto-reject path fires (the modal clears on the same
/// frame so this is mostly defense in depth).
pub fn remaining_secs(opened_at: Instant, timeout_secs: u64, now: Instant) -> u64 {
    let elapsed = now.saturating_duration_since(opened_at).as_secs();
    timeout_secs.saturating_sub(elapsed)
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, vertical_mid, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, centered, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(vertical_mid);
    centered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn remaining_secs_counts_down() {
        let opened = Instant::now();
        let now = opened + Duration::from_secs(120);
        assert_eq!(remaining_secs(opened, 1800, now), 1680);
    }

    #[test]
    fn remaining_secs_saturates_at_zero_past_timeout() {
        let opened = Instant::now();
        let now = opened + Duration::from_secs(3000);
        assert_eq!(remaining_secs(opened, 1800, now), 0);
    }

    #[test]
    fn remaining_secs_full_at_open() {
        let opened = Instant::now();
        assert_eq!(remaining_secs(opened, 1800, opened), 1800);
    }
}
