use std::io;
use std::time::Duration;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::DefaultTerminal;

mod ui;

fn main() -> Result<()> {
    crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = ratatui::init();

    let result = run(&mut terminal);

    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    crossterm::terminal::disable_raw_mode().ok();
    ratatui::restore();

    result
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame))?;
        if event::poll(Duration::from_millis(16))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == event::KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }
    }
}
