use std::path::PathBuf;

use anyhow::Result;

use crate::app::App;

/// Initialise a new forge, optionally at a specific directory.
///
/// If `dir` is `None` the current working directory is used.
pub fn init(app: &App, dir: Option<PathBuf>) -> Result<()> {
    let _ = (app, dir);
    anyhow::bail!("not yet implemented")
}

/// Show the status of the open forge.
pub fn status(app: &mut App) -> Result<()> {
    let _ = app;
    anyhow::bail!("not yet implemented")
}
