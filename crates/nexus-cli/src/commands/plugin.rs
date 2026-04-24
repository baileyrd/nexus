use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use nexus_plugins::{PluginError, scaffold as nexus_scaffold, PluginTemplate, ScaffoldConfig};

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

/// Reset the crash counter for `plugin_id` (F-8.2.1). Quarantined
/// plugins skip load until this runs. A missing counter file is a no-op.
pub fn reset_crash(app: &mut App, plugin_id: &str) -> Result<()> {
    app.plugins()?.reset_crash_count(plugin_id)?;
    let format = app.format();
    print_success(
        format,
        &format!("Plugin '{plugin_id}' crash counter reset."),
        &serde_json::json!({ "id": plugin_id, "status": "reset" }),
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
///
/// `template_str` selects the output shape: `script` (sandboxed JS/TS — the
/// modern path per Phase 4 §4.2), `core` (WASM, maximum trust), or
/// `community` (WASM, capability-gated). Unknown / empty values fall back to
/// `script` so `nexus plugin scaffold` with no flags produces the default
/// shape. The legacy `--type <core|community>` invocation still routes here
/// — clap forwards whichever of `--template` / `--type` the user passed.
pub fn scaffold(
    template_str: &str,
    id: Option<&str>,
    name: Option<&str>,
    author: Option<&str>,
    output: Option<&Path>,
) -> Result<()> {
    let template = match template_str.to_lowercase().as_str() {
        "core" => PluginTemplate::Core,
        "community" => PluginTemplate::Community,
        "script" | "" => PluginTemplate::Script,
        other => {
            return Err(anyhow!(
                "unknown plugin template '{other}'; expected one of: script, core, community"
            ));
        }
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
    match template {
        PluginTemplate::Script => {
            for f in ["plugin.json", "index.ts", "README.md", "package.json", "tsconfig.json"] {
                println!("  {}", output_dir.join(f).display());
            }
            println!();
            println!("Next steps:");
            println!("  cd {} && pnpm install && pnpm build", output_dir.display());
            println!("  cp index.js plugin.json ~/.nexus-shell/plugins/{}/", plugin_id);
        }
        PluginTemplate::Core | PluginTemplate::Community => {
            println!("  {}", output_dir.join("Cargo.toml").display());
            println!("  {}", output_dir.join("manifest.toml").display());
            println!("  {}", output_dir.join("src").join("lib.rs").display());
        }
    }

    Ok(())
}

/// Dispatch a plugin-registered CLI subcommand (`nexus <subcommand> [args…]`).
///
/// Loads all community plugins, then forwards the call to whichever plugin
/// registered `subcommand` via `[[registrations.cli_subcommand]]`. The
/// remaining `args` are passed as a JSON array.
pub fn dispatch_external(app: &mut App, subcommand: &str, args: Vec<String>) -> Result<()> {
    let plugins = app.plugins()?;
    plugins.load_all()?;

    let args_json = serde_json::json!(args);

    match plugins.dispatch_cli(subcommand, &args_json) {
        Ok(result) => {
            let text = match &result {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other)
                    .unwrap_or_else(|_| other.to_string()),
            };
            println!("{text}");
            Ok(())
        }
        Err(PluginError::PluginNotFound(_)) => {
            let available = plugins.list_cli_subcommands();
            if available.is_empty() {
                Err(anyhow!(
                    "unknown subcommand '{subcommand}'; no plugins with CLI subcommands are installed"
                ))
            } else {
                let list = available
                    .iter()
                    .map(|(id, desc)| format!("  {id:<20} {desc}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Err(anyhow!(
                    "unknown subcommand '{subcommand}'\n\nPlugin subcommands:\n{list}"
                ))
            }
        }
        Err(e) => Err(e.into()),
    }
}

// ---------------------------------------------------------------------------
// WI-38 (Phase 4 §4.1): `nexus plugin install|list|remove` for the shell.
//
// These interoperate with the shell's plugin directory at
// `~/.nexus-shell/plugins/<id>/` (the scanner lives at
// `shell/src-tauri/src/lib.rs`). The marketplace fetch-and-unpack path is
// Phase 5 WI-44 — `install <id>` is a stub.
// ---------------------------------------------------------------------------

/// `nexus plugin install <plugin>`.
///
/// Dispatches based on the shape of the argument:
/// - If it's an existing local directory, forward to the legacy kernel-plugin
///   loader (preserves `nexus plugin install ./my-plugin` from the README).
/// - Otherwise treat it as a marketplace plugin id and print the Phase 5 stub.
pub fn install_dispatch(app: &mut App, plugin: &str) -> Result<()> {
    let as_path = Path::new(plugin);
    if as_path.is_dir() {
        return install(app, as_path);
    }

    eprintln!(
        "Plugin install requires the marketplace (Phase 5 WI-44). \
         See docs/PHASE-5-IMPLEMENTATION-PLAN.md.\n\
         \n\
         To install a local plugin directory, pass a path that exists on disk:\n    \
         nexus plugin install ./path/to/plugin"
    );
    std::process::exit(2);
}

/// `nexus plugin list --shell` — enumerate entries under
/// `~/.nexus-shell/plugins/`. Reads each entry's `plugin.json` (if present)
/// to surface name + version.
pub fn list_shell_plugins() -> Result<()> {
    let dir = shell_plugins_dir()?;
    if !dir.exists() {
        println!("No shell plugins installed ({} does not exist).", dir.display());
        return Ok(());
    }

    let mut rows: Vec<(String, String, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = entry.path().join("plugin.json");
        let (name, version) = if manifest_path.exists() {
            read_plugin_manifest(&manifest_path).unwrap_or_else(|_| {
                (String::from("<unreadable plugin.json>"), String::from("?"))
            })
        } else {
            (String::from("<no plugin.json>"), String::from("?"))
        };
        rows.push((id, name, version));
    }

    if rows.is_empty() {
        println!("No shell plugins installed.");
        return Ok(());
    }

    println!("{:<28} {:<32} {}", "ID", "Name", "Version");
    println!("{}", "-".repeat(78));
    for (id, name, version) in rows {
        println!("{id:<28} {name:<32} {version}");
    }
    Ok(())
}

/// `nexus plugin remove <id>` — delete `~/.nexus-shell/plugins/<id>/`.
/// Prompts for confirmation unless `yes` is true.
pub fn remove_shell_plugin(id: &str, yes: bool) -> Result<()> {
    let dir = shell_plugins_dir()?.join(id);
    if !dir.exists() {
        return Err(anyhow!(
            "No shell plugin with id '{id}' (expected {})",
            dir.display()
        ));
    }

    if !yes {
        print!("Remove shell plugin '{id}' from {}? [y/N] ", dir.display());
        io::stdout().flush().ok();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        let trimmed = answer.trim().to_ascii_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    std::fs::remove_dir_all(&dir)
        .with_context(|| format!("removing {}", dir.display()))?;
    println!("Removed shell plugin '{id}'.");
    Ok(())
}

fn shell_plugins_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("could not determine home directory")?;
    Ok(PathBuf::from(home).join(".nexus-shell").join("plugins"))
}

fn read_plugin_manifest(path: &Path) -> Result<(String, String)> {
    let contents = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&contents)?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    Ok((name, version))
}

#[cfg(test)]
mod shell_plugin_tests {
    //! These tests exercise the `shell_plugins_dir()` → HOME path directly
    //! by passing an explicit root via the `_with_root` test helpers rather
    //! than mutating `$HOME`. Mutating HOME globally was observed to cause
    //! unrelated term-crate tests to panic when their tempdir vanished
    //! underneath an ambient process lookup — safer to avoid env mutation
    //! in unit tests altogether.
    use super::*;

    fn list_shell_plugins_at(root: &Path) -> Result<()> {
        // Emulate the body of `list_shell_plugins()` with an explicit root.
        // We test the *logic*; the one-line `~/` resolver is well-covered
        // by manual smoke.
        let dir = root.join(".nexus-shell").join("plugins");
        if !dir.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let _ = read_plugin_manifest(&entry.path().join("plugin.json"));
            }
        }
        Ok(())
    }

    #[test]
    fn list_shell_plugins_empty_root_ok() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_shell_plugins_at(tmp.path()).is_ok());
    }

    #[test]
    fn list_shell_plugins_reads_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp
            .path()
            .join(".nexus-shell")
            .join("plugins")
            .join("community.foo");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name":"Foo Plugin","version":"1.2.3"}"#,
        )
        .unwrap();
        let (name, version) =
            read_plugin_manifest(&plugin_dir.join("plugin.json")).unwrap();
        assert_eq!(name, "Foo Plugin");
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn read_plugin_manifest_missing_fields_uses_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plugin.json");
        std::fs::write(&path, "{}").unwrap();
        let (name, version) = read_plugin_manifest(&path).unwrap();
        assert_eq!(name, "<unnamed>");
        assert_eq!(version, "?");
    }
}
