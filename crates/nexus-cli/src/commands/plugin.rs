use std::path::Path;

use anyhow::Result;
use nexus_plugins::{scaffold as nexus_scaffold, PluginTemplate, ScaffoldConfig};

use crate::app::App;
use crate::output::{print_list, print_success, print_value};

/// Install a plugin from `dir`.
pub fn install(app: &mut App, dir: &Path) -> Result<()> {
    let info = app.plugins()?.load(dir)?;
    let format = app.format();
    let data = serde_json::json!({
        "id": info.id,
        "name": info.name,
        "version": info.version,
        "status": format!("{:?}", info.status),
    });
    print_success(
        format,
        &format!("Plugin '{}' ({}) installed successfully.", info.name, info.id),
        &data,
    );
    Ok(())
}

/// List all installed plugins.
pub fn list(app: &mut App) -> Result<()> {
    let format = app.format();
    let plugins = app.plugins()?.list();
    if plugins.is_empty() {
        println!("No plugins loaded.");
        return Ok(());
    }
    let headers = &["ID", "Name", "Version", "Status", "Trust"];
    let rows: Vec<Vec<String>> = plugins
        .into_iter()
        .map(|p| {
            vec![
                p.id,
                p.name,
                p.version,
                format!("{:?}", p.status),
                format!("{:?}", p.trust_level),
            ]
        })
        .collect();
    print_list(format, headers, &rows);
    Ok(())
}

/// Call a plugin command identified by `plugin_id` and `command`, passing
/// `args_json` as JSON-encoded arguments.
pub fn call(app: &mut App, plugin_id: &str, command: &str, args_json: &str) -> Result<()> {
    let args: serde_json::Value = if args_json.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(args_json)
            .map_err(|e| anyhow::anyhow!("invalid JSON args: {e}"))?
    };
    let format = app.format();
    let result = app.plugins()?.dispatch_ipc(plugin_id, command, &args)?;
    print_value(format, &result);
    Ok(())
}

/// Uninstall the plugin identified by `plugin_id`.
pub fn uninstall(app: &mut App, plugin_id: &str) -> Result<()> {
    app.plugins()?.unload(plugin_id)?;
    let format = app.format();
    print_success(
        format,
        &format!("Plugin '{plugin_id}' uninstalled."),
        &serde_json::Value::Null,
    );
    Ok(())
}

/// Enable the plugin identified by `plugin_id`.
pub fn enable(app: &mut App, plugin_id: &str) -> Result<()> {
    app.plugins()?.enable(plugin_id)?;
    let format = app.format();
    print_success(
        format,
        &format!("Plugin '{plugin_id}' enabled."),
        &serde_json::json!({ "id": plugin_id, "status": "running" }),
    );
    Ok(())
}

/// Disable the plugin identified by `plugin_id`.
pub fn disable(app: &mut App, plugin_id: &str) -> Result<()> {
    app.plugins()?.disable(plugin_id)?;
    let format = app.format();
    print_success(
        format,
        &format!("Plugin '{plugin_id}' disabled."),
        &serde_json::json!({ "id": plugin_id, "status": "stopped" }),
    );
    Ok(())
}

/// View or update settings for the plugin identified by `plugin_id`.
///
/// If `set_json` is `None`, the current settings are printed.
/// If `set_json` is `Some`, the settings are updated from the JSON string.
pub fn settings(app: &mut App, plugin_id: &str, set_json: Option<&str>) -> Result<()> {
    let format = app.format();
    if let Some(json_str) = set_json {
        let new_settings: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("invalid JSON: {e}"))?;
        app.plugins()?.set_settings(plugin_id, &new_settings)?;
        print_success(
            format,
            &format!("Settings updated for '{plugin_id}'."),
            &new_settings,
        );
    } else {
        let current = app.plugins()?.get_settings(plugin_id)?;
        print_value(format, &current);
    }
    Ok(())
}

/// Scaffold a new plugin project.
pub fn scaffold(
    type_str: &str,
    id: Option<&str>,
    name: Option<&str>,
    author: Option<&str>,
    output: Option<&Path>,
) -> Result<()> {
    let template = match type_str.to_lowercase().as_str() {
        "core" => PluginTemplate::Core,
        _ => PluginTemplate::Community,
    };

    let plugin_id = id.unwrap_or("com.example.my-plugin").to_string();
    let plugin_name = name.unwrap_or("My Plugin").to_string();
    let author_str = author.unwrap_or("Unknown").to_string();

    let config = ScaffoldConfig {
        plugin_id: plugin_id.clone(),
        plugin_name: plugin_name.clone(),
        author: author_str,
        description: format!("{plugin_name} — Nexus plugin."),
    };

    let output_dir = match output {
        Some(p) => p.to_path_buf(),
        None => std::path::PathBuf::from(&plugin_id),
    };

    nexus_scaffold(&output_dir, template, &config)?;

    println!("Scaffolded plugin '{plugin_name}' at '{}':", output_dir.display());
    println!("  {}", output_dir.join("Cargo.toml").display());
    println!("  {}", output_dir.join("manifest.toml").display());
    println!("  {}", output_dir.join("src").join("lib.rs").display());

    Ok(())
}
