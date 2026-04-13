use std::io;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::DefaultTerminal;

mod app;
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
            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        event::KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        event::KeyCode::Up | event::KeyCode::Char('k') => {
                            app.tree.move_up();
                        }
                        event::KeyCode::Down | event::KeyCode::Char('j') => {
                            app.tree.move_down();
                        }
                        event::KeyCode::Enter => {
                            let visible = app.visible_entries();
                            let is_dir = visible
                                .get(app.tree.selected)
                                .map(|e| e.is_dir)
                                .unwrap_or(false);
                            if is_dir {
                                app.toggle_dir();
                            } else {
                                let _ = app.open_selected_file();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
