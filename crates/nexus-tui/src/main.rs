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

    // Write tracing output to a file so we can inspect what events
    // crossterm delivers without polluting the TUI alternate screen.
    // Path is controlled by NEXUS_TUI_LOG; defaults to
    // `/tmp/nexus-tui.log` on Unix, `%TEMP%/nexus-tui.log` on Windows.
    init_file_tracing();

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

fn init_file_tracing() {
    use tracing_subscriber::EnvFilter;

    let path = std::env::var("NEXUS_TUI_LOG").unwrap_or_else(|_| {
        if cfg!(windows) {
            std::env::var("TEMP")
                .ok()
                .map(|t| format!("{t}\\nexus-tui.log"))
                .unwrap_or_else(|| "nexus-tui.log".into())
        } else {
            "/tmp/nexus-tui.log".into()
        }
    });
    let Ok(file) = std::fs::File::create(&path) else {
        // Log init failures are silent by design — we don't want to
        // break the TUI because a temp-file write wasn't possible.
        return;
    };
    // Default to `nexus_tui=debug` so every input-handler breadcrumb
    // lands without the user having to set RUST_LOG manually. Any
    // existing RUST_LOG value still takes precedence.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("nexus_tui=debug"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .try_init();
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
