use std::path::Path;

use anyhow::Result;

use crate::app::App;

/// Install a plugin from `dir`.
pub fn install(app: &mut App, dir: &Path) -> Result<()> {
    let _ = (app, dir);
    anyhow::bail!("not yet implemented")
}

/// List all installed plugins.
pub fn list(app: &mut App) -> Result<()> {
    let _ = app;
    anyhow::bail!("not yet implemented")
}

/// Call a plugin command identified by `plugin_id` and `command`, passing
/// `args_json` as JSON-encoded arguments.
pub fn call(app: &mut App, plugin_id: &str, command: &str, args_json: &str) -> Result<()> {
    let _ = (app, plugin_id, command, args_json);
    anyhow::bail!("not yet implemented")
}

/// Uninstall the plugin identified by `plugin_id`.
pub fn uninstall(app: &mut App, plugin_id: &str) -> Result<()> {
    let _ = (app, plugin_id);
    anyhow::bail!("not yet implemented")
}

/// Scaffold a new plugin project.
pub fn scaffold(
    type_str: &str,
    id: Option<&str>,
    name: Option<&str>,
    author: Option<&str>,
    output: Option<&Path>,
) -> Result<()> {
    let _ = (type_str, id, name, author, output);
    anyhow::bail!("not yet implemented")
}
