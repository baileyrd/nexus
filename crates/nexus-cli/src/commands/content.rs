use anyhow::Result;

use crate::app::App;

/// Create a new content node at `path`.
pub fn create(app: &mut App, path: &str, content: Option<&str>, stdin: bool) -> Result<()> {
    let _ = (app, path, content, stdin);
    anyhow::bail!("not yet implemented")
}

/// Read the content node at `path`.
pub fn read(app: &mut App, path: &str, raw: bool) -> Result<()> {
    let _ = (app, path, raw);
    anyhow::bail!("not yet implemented")
}

/// Delete the content node at `path`.
pub fn delete(app: &mut App, path: &str, force: bool) -> Result<()> {
    let _ = (app, path, force);
    anyhow::bail!("not yet implemented")
}

/// Search content nodes with `query`, returning up to `limit` results.
pub fn search(app: &mut App, query: &str, limit: usize) -> Result<()> {
    let _ = (app, query, limit);
    anyhow::bail!("not yet implemented")
}
