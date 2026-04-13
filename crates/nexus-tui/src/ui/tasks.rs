//! Task list view widget for nexus-tui.
//!
//! Renders a full-pane task list that replaces the viewer when toggled.
//! Shows all tasks with completion status, content, and file location.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::TuiApp;

/// Render the task list view into `area`.
///
/// Each entry shows a checkbox (`[x]` or `[ ]`), the task content, and
/// the file path with line number in gray. The title includes pending
/// and total counts.
pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let total = app.task_view.entries.len();
    let pending = app.task_view.entries.iter().filter(|t| !t.completed).count();
    let title = format!(" Tasks ({pending} pending, {total} total) ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Yellow));

    let items: Vec<ListItem> = app
        .task_view
        .entries
        .iter()
        .map(|task| {
            let checkbox = if task.completed { "[x] " } else { "[ ] " };
            let content_style = if task.completed {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            let location = format!("  {}:{}", task.file_path, task.line_number);

            let line = Line::from(vec![
                Span::styled(
                    checkbox,
                    if task.completed {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                ),
                Span::styled(task.content.clone(), content_style),
                Span::styled(location, Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED),
        );

    frame.render_stateful_widget(list, area, &mut app.task_view.list_state);
}
