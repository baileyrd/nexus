//! File tree widget for nexus-tui.
//!
//! Renders the left-hand pane showing the forge file tree with
//! expand/collapse indicators and per-entry icons.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::{Focus, Mode, TuiApp};

/// Render the file tree into `area`.
///
/// The block border is blue when focused, dark-gray otherwise.
/// Directories are shown in bold blue with a `▼`/`▶` icon; files are shown in
/// white with a leading space.
///
/// When the search overlay is active and the query is non-empty, file
/// entries are filtered to show only those whose path contains the query
/// (case-insensitive substring match).
pub fn render(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let focused = app.focus == Focus::FileTree;
    let border_color = if focused {
        Color::Blue
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Files ")
        .border_style(Style::default().fg(border_color));

    let visible = app.visible_entries();

    // Apply fuzzy filtering when search overlay is active with a query.
    let fuzzy_query = if app.mode == Mode::Search && !app.search.query.is_empty() {
        Some(app.search.query.to_lowercase())
    } else {
        None
    };

    let filtered: Vec<_> = if let Some(ref needle) = fuzzy_query {
        visible
            .into_iter()
            .filter(|e| !e.is_dir && e.path.to_lowercase().contains(needle.as_str()))
            .collect()
    } else {
        visible
    };

    // Find the visible index that corresponds to app.tree.selected so the
    // ListState scrolls to the right item.
    let visible_selected = if app.tree.selected < app.tree.entries.len() {
        filtered
            .iter()
            .position(|e| std::ptr::eq(*e, &app.tree.entries[app.tree.selected]))
            .unwrap_or(0)
    } else {
        0
    };

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|entry| {
            let indent = "  ".repeat(entry.depth);
            let icon = if entry.is_dir {
                if entry.is_expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };
            let label = format!("{}{}{}", indent, icon, entry.name);
            let style = if entry.is_dir {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Point ListState at the visible index.
    app.tree.list_state.select(Some(visible_selected));
    frame.render_stateful_widget(list, area, &mut app.tree.list_state);
}
