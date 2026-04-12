use anyhow::Result;

use crate::app::App;

/// Stream the most recent log entries, optionally filtered by `level`.
pub fn tail(app: &App, level: Option<&str>, lines: usize) -> Result<()> {
    let _ = (app, level, lines);
    anyhow::bail!("not yet implemented")
}

/// Show logs for the given `date` (YYYY-MM-DD format).
pub fn show(app: &App, date: &str) -> Result<()> {
    let _ = (app, date);
    anyhow::bail!("not yet implemented")
}

/// Print the path to the log directory.
pub fn path(app: &App) -> Result<()> {
    let _ = app;
    anyhow::bail!("not yet implemented")
}
