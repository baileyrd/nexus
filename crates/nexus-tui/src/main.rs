use std::io;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::DefaultTerminal;

mod app;
mod input;
mod ui;

use app::TuiApp;

fn main() -> Result<()> {
    let forge_path = resolve_forge_path()?;

    crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = ratatui::init();

    let mut app = TuiApp::new(forge_path)?;
    let result = run(&mut terminal, &mut app);

    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    crossterm::terminal::disable_raw_mode().ok();
    ratatui::restore();

    result
}

/// Resolve the forge root path from (in order):
/// 1. First command-line argument
/// 2. `NEXUS_FORGE_PATH` environment variable
/// 3. `~/.nexus/default`
fn resolve_forge_path() -> Result<PathBuf> {
    // 1. argv[1]
    if let Some(arg) = std::env::args().nth(1) {
        return Ok(PathBuf::from(arg));
    }

    // 2. Environment variable
    if let Ok(env_path) = std::env::var("NEXUS_FORGE_PATH") {
        return Ok(PathBuf::from(env_path));
    }

    // 3. ~/.nexus/default
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("could not determine home directory")?;
    Ok(PathBuf::from(home).join(".nexus").join("default"))
}

fn run(terminal: &mut DefaultTerminal, app: &mut TuiApp) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(16))? {
            let evt = event::read()?;
            input::handle_event(app, evt)?;
        }

        // Pump the PTY whenever the terminal panel is visible so
        // long-running commands surface new output between keystrokes.
        // The call uses a 50 ms internal timeout and returns
        // immediately when there's nothing to read, so an idle
        // session doesn't slow the render loop.
        if app.terminal.active {
            app.pump_terminal();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
