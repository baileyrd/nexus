use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::app::TuiApp;

mod file_tree;
mod status_bar;
mod viewer;

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
    viewer::render(frame, app, viewer_area);
    status_bar::render(frame, app, status);
}
