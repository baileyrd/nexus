use anyhow::Result;

use crate::app::App;

/// Watch the forge for filesystem changes matching `glob`.
pub fn run(app: &mut App, glob: &str) -> Result<()> {
    let _ = (app, glob);
    anyhow::bail!("not yet implemented")
}
