use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::{Mode, TuiApp};

mod agent;
mod agent_approval;
mod ai;
mod backlinks;
mod file_tree;
mod kernel_stats;
mod status_bar;
mod tasks;
mod terminal;
mod viewer;

/// Render the full TUI layout.
pub fn render(frame: &mut Frame, app: &mut TuiApp) {
    let [body, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [tree_area, right_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ])
    .areas(body);

    file_tree::render(frame, app, tree_area);

    // Right pane priority: agent panel > AI panel > terminal >
    // tasks > viewer. The agent panel sits above AI so the
    // approval-modal flow isn't visually nested under a chat
    // surface; both are blue but the agent panel's lighter shade +
    // header label tell them apart.
    if app.agent.active {
        agent::render(frame, app, right_area);
    } else if app.ai.active {
        ai::render(frame, app, right_area);
    } else if app.terminal.active {
        terminal::render(frame, app, right_area);
    } else if app.task_view.active {
        tasks::render(frame, app, right_area);
    } else {
        // Split right pane for backlinks when visible.
        let (viewer_area, backlinks_area) = if app.backlinks.visible {
            let [va, ba] = Layout::vertical([
                Constraint::Percentage(70),
                Constraint::Percentage(30),
            ])
            .areas(right_area);
            (va, Some(ba))
        } else {
            (right_area, None)
        };

        // Viewer area: split off a find bar when in Find mode.
        if app.mode == Mode::Find {
            let [viewer_body, find_bar] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(viewer_area);
            viewer::render(frame, app, viewer_body);
            render_find_bar(frame, app, find_bar);
        } else {
            viewer::render(frame, app, viewer_area);
        }

        // Render backlinks panel if visible.
        if let Some(bl_area) = backlinks_area {
            backlinks::render(frame, app, bl_area);
        }
    }

    status_bar::render(frame, app, status);

    // Search overlay rendered on top of everything.
    if app.mode == Mode::Search {
        render_search_overlay(frame, app);
    }

    // BL-137 — kernel-stats overlay layered above the search popup
    // so the close keystroke (Shift+K) always resolves before the
    // user toggles modes underneath.
    if app.kernel_stats.visible {
        kernel_stats::render(frame, app, frame.area());
    }

    // BL-132 — approval modal layered above every other overlay.
    // The user must answer (y / n / Esc) or wait for the auto-reject
    // timer; until then keystrokes do not reach the panels beneath.
    if app.agent.pending.is_some() {
        agent_approval::render(frame, app, frame.area());
    }
}

// ── Find bar ──────────────────────────────────────────────────────────────────

fn render_find_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let match_count = app.find.matches.len();
    let current = if match_count == 0 {
        0
    } else {
        app.find.current_match + 1
    };
    let match_info = if match_count == 0 {
        "no matches".to_owned()
    } else {
        format!("{current}/{match_count}")
    };

    let find_line = Line::from(vec![
        Span::styled(" Find: ", Style::default().fg(Color::Cyan)),
        Span::styled(app.find.query.clone(), Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(match_info, Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(
        Paragraph::new(find_line).style(Style::default().bg(Color::Rgb(30, 40, 50))),
        area,
    );
}

// ── Search overlay ────────────────────────────────────────────────────────────

fn render_search_overlay(frame: &mut Frame, app: &TuiApp) {
    let popup_area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, popup_area);

    // Outer block with border.
    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split the inner area: query line at top, then results list.
    let [query_area, results_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);

    // Query line.
    let query_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Yellow)),
        Span::styled(app.search.query.clone(), Style::default().fg(Color::White)),
        Span::styled("█", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(query_line), query_area);

    // Results list.
    let items: Vec<ListItem> = app
        .search
        .results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = i == app.search.selected;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let label = if r.excerpt.is_empty() {
                r.file_path.clone()
            } else {
                format!("{}: {}", r.file_path, r.excerpt)
            };
            ListItem::new(Span::styled(label, style))
        })
        .collect();

    if items.is_empty() && !app.search.query.is_empty() {
        let no_results = Paragraph::new(Span::styled(
            "No results",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(no_results, results_area);
    } else {
        let list = List::new(items);
        frame.render_widget(list, results_area);
    }
}

// ── Helper: compute a centred popup rect ─────────────────────────────────────

/// Return a [`Rect`] that is `percent_x`% wide and `percent_y`% tall,
/// centred inside `area`.
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
