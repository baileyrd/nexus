use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::TuiApp;

mod file_tree;

/// Render the full TUI layout.
pub fn render(frame: &mut Frame, app: &mut TuiApp) {
    let [body, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [tree_area, viewer_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(75),
    ])
    .areas(body);

    file_tree::render(frame, app, tree_area);

    let viewer_title = " Preview ";
    let viewer_content = app
        .viewer
        .file_path
        .as_deref()
        .unwrap_or("Select a file");
    let viewer = Paragraph::new(viewer_content)
        .block(Block::default().borders(Borders::ALL).title(viewer_title));
    frame.render_widget(viewer, viewer_area);

    let mode_str = match app.mode {
        crate::app::Mode::Normal => "NORMAL",
        crate::app::Mode::Search => "SEARCH",
        crate::app::Mode::Find => "FIND",
    };
    let file_str = app
        .viewer
        .file_path
        .as_deref()
        .unwrap_or("no file");
    let bar = Paragraph::new(format!(" {mode_str} │ {file_str} │ q to quit"));
    frame.render_widget(bar, status);
}
