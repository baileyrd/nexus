//! BL-137 — kernel-stats overlay.
//!
//! Renders the latest `com.nexus.security::metrics_snapshot` reply as
//! a four-section modal: IPC calls (top 10 by total count), event-bus
//! publishes (top 10), capability-check tallies, and the queue-depth
//! gauge + `metrics_dropped_total` sentinel. The shape mirrors the
//! shell's health panel; both read the same IPC handler.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::TuiApp;

pub fn render(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let popup = centered_rect(80, 80, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Kernel metrics (Shift+K to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let Some(snap) = app.kernel_stats.snapshot.as_ref() else {
        let msg = match app.kernel_stats.last_error.as_ref() {
            Some(e) => format!("metrics unavailable: {e}"),
            None => "metrics snapshot unavailable (kernel metrics not installed)".to_string(),
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    // Four-row layout: gauge line, IPC top-N, event-bus top-N, caps.
    let [gauge_area, ipc_area, bus_area, caps_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(45),
        Constraint::Percentage(25),
        Constraint::Percentage(30),
    ])
    .areas(inner);

    // ── Gauge line ────────────────────────────────────────────────
    let queue_depth = snap.get("event_bus_queue_depth").and_then(|v| v.as_u64()).unwrap_or(0);
    let dropped = snap.get("metrics_dropped_total").and_then(|v| v.as_u64()).unwrap_or(0);
    let gauge_line = Line::from(vec![
        Span::styled("event_bus_queue_depth: ", Style::default().fg(Color::Yellow)),
        Span::styled(format!("{queue_depth}"), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("    "),
        Span::styled("metrics_dropped_total: ", Style::default().fg(Color::Yellow)),
        Span::styled(format!("{dropped}"), Style::default().add_modifier(Modifier::BOLD)),
    ]);
    frame.render_widget(
        Paragraph::new(gauge_line)
            .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray))),
        gauge_area,
    );

    // ── IPC top-N ─────────────────────────────────────────────────
    let mut ipc_rows: Vec<(String, u64)> = snap
        .get("ipc_calls_total")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    ipc_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let ipc_total: u64 = ipc_rows.iter().map(|(_, n)| n).sum();
    let ipc_top: Vec<Line> = std::iter::once(Line::from(Span::styled(
        format!("IPC calls (total {ipc_total}, top 10):"),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .chain(ipc_rows.iter().take(10).map(|(k, n)| {
        Line::from(vec![
            Span::styled(format!("{n:>8}  "), Style::default().fg(Color::Green)),
            Span::raw(k.clone()),
        ])
    }))
    .collect();
    frame.render_widget(
        Paragraph::new(ipc_top).wrap(Wrap { trim: false }),
        ipc_area,
    );

    // ── Event-bus top-N ───────────────────────────────────────────
    let mut bus_rows: Vec<(String, u64)> = snap
        .get("event_bus_published_total")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    bus_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let bus_total: u64 = bus_rows.iter().map(|(_, n)| n).sum();
    let bus_top: Vec<Line> = std::iter::once(Line::from(Span::styled(
        format!("event_bus publishes (total {bus_total}, top 10):"),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .chain(bus_rows.iter().take(10).map(|(k, n)| {
        Line::from(vec![
            Span::styled(format!("{n:>8}  "), Style::default().fg(Color::Green)),
            Span::raw(k.clone()),
        ])
    }))
    .collect();
    frame.render_widget(
        Paragraph::new(bus_top).wrap(Wrap { trim: false }),
        bus_area,
    );

    // ── Capability checks ─────────────────────────────────────────
    let mut cap_rows: Vec<(String, u64)> = snap
        .get("capability_checks_total")
        .and_then(|v| v.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    cap_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let cap_lines: Vec<Line> = std::iter::once(Line::from(Span::styled(
        "capability checks (top 10):",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .chain(cap_rows.iter().take(10).map(|(k, n)| {
        let denied = k.ends_with("::denied");
        Line::from(vec![
            Span::styled(
                format!("{n:>8}  "),
                Style::default().fg(if denied { Color::Red } else { Color::Green }),
            ),
            Span::raw(k.clone()),
        ])
    }))
    .collect();
    frame.render_widget(
        Paragraph::new(cap_lines).wrap(Wrap { trim: false }),
        caps_area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, mid, _] = Layout::vertical([
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
    .areas(mid);
    centered
}
