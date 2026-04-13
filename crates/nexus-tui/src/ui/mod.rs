use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(frame: &mut Frame) {
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

    let tree = Paragraph::new("File tree here")
        .block(Block::default().borders(Borders::ALL).title(" Files "));
    frame.render_widget(tree, tree_area);

    let viewer = Paragraph::new("Select a file")
        .block(Block::default().borders(Borders::ALL).title(" Preview "));
    frame.render_widget(viewer, viewer_area);

    let bar = Paragraph::new(" NORMAL │ no file │ q to quit");
    frame.render_widget(bar, status);
}
