//! CLI commands for configuration management.

use anyhow::Result;
use nexus_bootstrap::storage as storage_ipc;

use crate::app::App;
use crate::output;

/// Show the current configuration for a config file.
pub fn show(app: &mut App, file: &str) -> Result<()> {
    match file {
        "app" | "workspace" | "mcp" | "ai" => {
            let payload = read_kind(app, file)?;
            println!("{}", payload.content);
        }
        "all" => {
            for (kind, header) in [
                ("app", "=== app.toml ==="),
                ("workspace", "=== workspace.json ==="),
                ("mcp", "=== mcp.toml ==="),
                ("ai", "=== ai.toml ==="),
            ] {
                let payload = read_kind(app, kind)?;
                println!("{header}");
                println!("{}", payload.content);
            }
        }
        _ => anyhow::bail!("Unknown config file: {file}. Valid: app, workspace, mcp, ai, all"),
    }
    Ok(())
}

/// Reset a config file to defaults.
pub fn reset(app: &mut App, file: &str) -> Result<()> {
    let label = match file {
        "app" => "Reset app.toml to defaults",
        "workspace" => "Reset workspace.json to defaults",
        "mcp" => "Reset mcp.toml to defaults",
        "ai" => "Reset ai.toml to defaults",
        _ => anyhow::bail!("Unknown config file: {file}. Valid: app, workspace, mcp, ai"),
    };
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    storage_ipc::config_reset(runtime, rt, file)?;
    output::print_success(format, label, &serde_json::json!(null));
    Ok(())
}

fn read_kind(app: &mut App, kind: &str) -> Result<storage_ipc::ConfigPayload> {
    let (runtime, rt) = app.runtime()?;
    storage_ipc::config_read(runtime, rt, kind)
}
