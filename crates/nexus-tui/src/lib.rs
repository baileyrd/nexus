//! Library entry point for `nexus-tui`.
//!
//! The TUI is usable in two ways:
//!
//! 1. As a standalone binary (`nexus-tui`), which is a thin wrapper around
//!    [`run_tui`]. This preserves back-compat for existing users who invoke
//!    the TUI directly.
//! 2. As a library callable from `nexus-cli`'s `nexus tui` subcommand. The
//!    dispatcher in `nexus-cli` imports [`run_tui`] and calls it on the
//!    subcommand arm.
//!
//! Terminal setup and teardown (raw mode, alternate screen, mouse capture,
//! ratatui init/restore) are fully self-contained within [`run_tui`] so it's
//! safe to call from a dispatcher that also runs unrelated code before and
//! after. A best-effort teardown runs via an RAII guard even if the run loop
//! returns an error.

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
mod streaming;
mod ui;

use app::TuiApp;

/// RAII guard that restores terminal state on drop. Using a guard instead of
/// manual cleanup at the end of `run_tui` ensures teardown runs even if the
/// run loop panics or returns early with an error.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)
            .context("failed to enter alternate screen")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        let _ = crossterm::terminal::disable_raw_mode();
        ratatui::restore();
    }
}

/// Run the TUI event loop. This is the callable library entry point used by
/// both the standalone `nexus-tui` binary and the `nexus tui` subcommand.
///
/// The forge path is resolved via [`resolve_forge_path`] — same precedence as
/// before the refactor: first CLI positional argument, then
/// `NEXUS_FORGE_PATH`, then `~/.nexus/default`.
pub fn run_tui() -> Result<()> {
    let forge_path = resolve_forge_path()?;

    // Write tracing output to a file so we can inspect what events
    // crossterm delivers without polluting the TUI alternate screen.
    // Path is controlled by NEXUS_TUI_LOG; defaults to
    // `/tmp/nexus-tui.log` on Unix, `%TEMP%/nexus-tui.log` on Windows.
    init_file_tracing();

    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    let mut app = TuiApp::new(forge_path)?;
    run(&mut terminal, &mut app)
    // _guard dropped here → raw mode disabled, alt screen left, ratatui restored.
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

        // AIG-07 — drain any pending stream chunks into the active
        // assistant message. No-op when there's no live session;
        // bounded work otherwise (try_recv until empty, plus an
        // is_finished check on the IPC join handle).
        if app.ai.streaming.is_some() {
            app.pump_ai();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
