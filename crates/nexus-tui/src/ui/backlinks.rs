//! Backlinks panel widget for nexus-tui.
//!
//! Renders a toggleable panel below the viewer showing files that link
//! to the currently viewed file.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::TuiApp;

/// Render the backlinks panel into `area`.
///
/// Each entry shows the source path in cyan and the link text in gray.
/// The title includes the total number of backlinks.
pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let count = app.backlinks.entries.len();
    let title = format!(" Backlinks ({count}) ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::DarkGray));

    let items: Vec<ListItem> = app
        .backlinks
        .entries
        .iter()
        .map(|(source_path, link_text)| {
            let line = Line::from(vec![
                Span::styled(
                    source_path.clone(),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("  {link_text}"),
                    Style::default().fg(Color::DarkGray),
                ),
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

    frame.render_stateful_widget(list, area, &mut app.backlinks.list_state);
}
