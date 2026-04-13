//! CLI commands for configuration management.

use anyhow::Result;

use crate::app::App;
use crate::output;

/// Show the current configuration for a config file.
pub fn show(app: &App, file: &str) -> Result<()> {
    let root = app.forge_root();
    match file {
        "app" => {
            let cfg = nexus_storage::config::load_app_config(root)?;
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
        "workspace" => {
            let state = nexus_storage::config::load_workspace_state(root)?;
            println!("{}", serde_json::to_string_pretty(&state)?);
        }
        "mcp" => {
            let cfg = nexus_storage::config::load_mcp_config(root)?;
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
        "ai" => {
            let cfg = nexus_storage::config::load_ai_config(root)?;
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
        "all" => {
            println!("=== app.toml ===");
            let app_cfg = nexus_storage::config::load_app_config(root)?;
            println!("{}", toml::to_string_pretty(&app_cfg)?);
            println!("\n=== workspace.json ===");
            let ws = nexus_storage::config::load_workspace_state(root)?;
            println!("{}", serde_json::to_string_pretty(&ws)?);
            println!("\n=== mcp.toml ===");
            let mcp = nexus_storage::config::load_mcp_config(root)?;
            println!("{}", toml::to_string_pretty(&mcp)?);
            println!("\n=== ai.toml ===");
            let ai = nexus_storage::config::load_ai_config(root)?;
            println!("{}", toml::to_string_pretty(&ai)?);
        }
        _ => anyhow::bail!("Unknown config file: {file}. Valid: app, workspace, mcp, ai, all"),
    }
    Ok(())
}

/// Reset a config file to defaults.
pub fn reset(app: &App, file: &str) -> Result<()> {
    let root = app.forge_root();
    let null = serde_json::json!(null);
    match file {
        "app" => {
            nexus_storage::config::save_app_config(root, &Default::default())?;
            output::print_success(app.format(), "Reset app.toml to defaults", &null);
        }
        "workspace" => {
            nexus_storage::config::save_workspace_state(root, &Default::default())?;
            output::print_success(app.format(), "Reset workspace.json to defaults", &null);
        }
        "mcp" => {
            nexus_storage::config::save_mcp_config(root, &Default::default())?;
            output::print_success(app.format(), "Reset mcp.toml to defaults", &null);
        }
        "ai" => {
            nexus_storage::config::save_ai_config(root, &Default::default())?;
            output::print_success(app.format(), "Reset ai.toml to defaults", &null);
        }
        _ => anyhow::bail!("Unknown config file: {file}. Valid: app, workspace, mcp, ai"),
    }
    Ok(())
}
