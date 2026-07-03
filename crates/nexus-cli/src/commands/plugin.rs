use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use nexus_plugins::{
    scaffold as nexus_scaffold, PluginError, PluginManager, PluginManagerConfig, PluginTemplate,
    ScaffoldConfig,
};

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
        &format!(
            "Plugin '{}' ({}) installed successfully.",
            info.name, info.id
        ),
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
        serde_json::from_str(args_json).map_err(|e| anyhow::anyhow!("invalid JSON args: {e}"))?
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
    // BL-113 Phase 1e/2b/3b — wire the just-enabled plugin's protocol
    // host contributions (if any) so they appear in the DAP / LSP / MCP
    // host's runtime maps. Best-effort: log outcomes, never block enable.
    log_dap_wire_outcomes(app.wire_dap_contributions_for_plugin(plugin_id), "wired");
    log_lsp_wire_outcomes(app.wire_lsp_contributions_for_plugin(plugin_id), "wired");
    log_mcp_wire_outcomes(app.wire_mcp_contributions_for_plugin(plugin_id), "wired");
    log_acp_wire_outcomes(app.wire_acp_contributions_for_plugin(plugin_id), "wired");
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
    // BL-113 Phase 1e/2b/3b — unwire BEFORE the disable lifecycle hook
    // fires so each protocol host removes contributions while the
    // plugin's manifest is still readable.
    log_dap_wire_outcomes(
        app.unwire_dap_contributions_for_plugin(plugin_id),
        "unwired",
    );
    log_lsp_wire_outcomes(
        app.unwire_lsp_contributions_for_plugin(plugin_id),
        "unwired",
    );
    log_mcp_wire_outcomes(
        app.unwire_mcp_contributions_for_plugin(plugin_id),
        "unwired",
    );
    log_acp_wire_outcomes(
        app.unwire_acp_contributions_for_plugin(plugin_id),
        "unwired",
    );
    app.plugins()?.disable(plugin_id)?;
    let format = app.format();
    print_success(
        format,
        &format!("Plugin '{plugin_id}' disabled."),
        &serde_json::json!({ "id": plugin_id, "status": "stopped" }),
    );
    Ok(())
}

fn log_dap_wire_outcomes(
    result: Result<Vec<nexus_bootstrap::dap_contribution_wiring::DapWireOutcome>>,
    verb_past: &'static str,
) {
    use nexus_bootstrap::dap_contribution_wiring::DapWireStatus;
    match result {
        Ok(outcomes) => {
            for outcome in &outcomes {
                if matches!(outcome.status, DapWireStatus::Ok) {
                    tracing::info!(
                        plugin_id = %outcome.plugin_id,
                        adapter = %outcome.adapter_name,
                        "{verb_past} DAP adapter contribution",
                    );
                } else {
                    tracing::warn!(
                        plugin_id = %outcome.plugin_id,
                        adapter = %outcome.adapter_name,
                        status = ?outcome.status,
                        "DAP adapter contribution skipped",
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "DAP contribution {verb_past} pass failed; plugin state change still applied",
            );
        }
    }
}

fn log_lsp_wire_outcomes(
    result: Result<Vec<nexus_bootstrap::lsp_contribution_wiring::LspWireOutcome>>,
    verb_past: &'static str,
) {
    use nexus_bootstrap::lsp_contribution_wiring::LspWireStatus;
    match result {
        Ok(outcomes) => {
            for outcome in &outcomes {
                if matches!(outcome.status, LspWireStatus::Ok) {
                    tracing::info!(
                        plugin_id = %outcome.plugin_id,
                        server = %outcome.server_name,
                        "{verb_past} LSP server contribution",
                    );
                } else {
                    tracing::warn!(
                        plugin_id = %outcome.plugin_id,
                        server = %outcome.server_name,
                        status = ?outcome.status,
                        "LSP server contribution skipped",
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "LSP contribution {verb_past} pass failed; plugin state change still applied",
            );
        }
    }
}

fn log_acp_wire_outcomes(
    result: Result<Vec<nexus_bootstrap::acp_contribution_wiring::AcpWireOutcome>>,
    verb_past: &'static str,
) {
    use nexus_bootstrap::acp_contribution_wiring::AcpWireStatus;
    match result {
        Ok(outcomes) => {
            for outcome in &outcomes {
                if matches!(outcome.status, AcpWireStatus::Ok) {
                    tracing::info!(
                        plugin_id = %outcome.plugin_id,
                        agent = %outcome.agent_name,
                        "{verb_past} ACP agent contribution",
                    );
                } else {
                    tracing::warn!(
                        plugin_id = %outcome.plugin_id,
                        agent = %outcome.agent_name,
                        status = ?outcome.status,
                        "ACP agent contribution skipped",
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "ACP contribution {verb_past} pass failed; plugin state change still applied",
            );
        }
    }
}

fn log_mcp_wire_outcomes(
    result: Result<Vec<nexus_bootstrap::mcp_contribution_wiring::McpWireOutcome>>,
    verb_past: &'static str,
) {
    use nexus_bootstrap::mcp_contribution_wiring::McpWireStatus;
    match result {
        Ok(outcomes) => {
            for outcome in &outcomes {
                if matches!(outcome.status, McpWireStatus::Ok) {
                    tracing::info!(
                        plugin_id = %outcome.plugin_id,
                        server = %outcome.server_name,
                        "{verb_past} MCP server contribution",
                    );
                } else {
                    tracing::warn!(
                        plugin_id = %outcome.plugin_id,
                        server = %outcome.server_name,
                        status = ?outcome.status,
                        "MCP server contribution skipped",
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "MCP contribution {verb_past} pass failed; plugin state change still applied",
            );
        }
    }
}

/// Revoke a HIGH-risk capability previously granted to `plugin_id`
/// (BL-096). Live-effective on the running plugin and persisted to
/// `granted_caps.json` for restart.
pub fn revoke(app: &mut App, plugin_id: &str, capability: &str) -> Result<()> {
    let cap = nexus_kernel::Capability::from_str(capability)
        .map_err(|e| anyhow!("invalid capability '{capability}': {e}"))?;
    if !cap.is_high_risk() {
        anyhow::bail!(
            "'{capability}' is not a HIGH-risk capability — only HIGH-risk \
             grants are revocable; manifest-declared caps cannot be revoked \
             at runtime."
        );
    }
    app.plugins()?.revoke_capability(plugin_id, cap)?;
    let format = app.format();
    print_success(
        format,
        &format!("Revoked '{capability}' from '{plugin_id}'."),
        &serde_json::json!({
            "plugin_id": plugin_id,
            "capability": capability,
            "revoked": true,
        }),
    );
    Ok(())
}

/// Verify a plugin manifest's `[signature]` block against the
/// trusted-key ring (BL-099). Used by operators to spot-check a
/// downloaded plugin before installing.
pub fn verify(plugin_dir: &Path, keys_dir: Option<&Path>) -> Result<()> {
    let manifest_path = plugin_dir.join("manifest.toml");
    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("read manifest at {}", manifest_path.display()))?;
    let manifest = nexus_plugins::parse_manifest(&raw, manifest_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow!("parse manifest: {e}"))?;
    let Some(sig) = manifest.signature.as_ref() else {
        anyhow::bail!(
            "{} has no [signature] block — nothing to verify",
            manifest_path.display()
        );
    };
    let verifier = match keys_dir {
        Some(dir) => nexus_plugins::signing::PluginSignatureVerifier::with_keys_dir(dir)
            .map_err(|e| anyhow!("load keyring at {}: {e}", dir.display()))?,
        None => nexus_plugins::signing::PluginSignatureVerifier::from_user_home(),
    };
    let canonical = nexus_plugins::signing::canonicalize_manifest_for_signing(&raw);
    verifier
        .verify(canonical.as_bytes(), sig)
        .map_err(|e| anyhow!("signature verification failed: {e}"))?;
    println!(
        "OK — {} signed by '{}' ({})",
        manifest.id, sig.signer_key_id, sig.algorithm
    );
    Ok(())
}

/// Build a `hot_reload: true` `PluginManager` rooted at `dir` and run its
/// initial `load_all` scan. Split out of [`dev`] so the setup half —
/// "does dev mode actually turn hot_reload on and load what's there" — is
/// unit-testable without also driving the infinite Ctrl+C loop.
///
/// # Errors
/// Returns an error if the plugin manager cannot be created at `dir`
/// (see [`PluginManager::new`]) or if the initial `load_all` scan fails.
fn start_dev_manager(dir: &Path) -> Result<(PluginManager, Vec<nexus_kernel::PluginInfo>)> {
    // `PluginManagerConfig::default()` already carries `hot_reload: true`
    // — it's `App::plugins()`'s override to `false` (app.rs:258) that
    // silences it for every other command. Spelled out here so the
    // C80 fix reads as a config choice, not an accident of the default.
    let config = PluginManagerConfig {
        hot_reload: true,
        ..Default::default()
    };
    let mut manager = PluginManager::new(dir, &config)
        .with_context(|| format!("failed to start plugin dev mode at '{}'", dir.display()))?;
    let loaded = manager
        .load_all()
        .with_context(|| format!("failed to load plugins from '{}'", dir.display()))?;
    Ok((manager, loaded))
}

/// C80 — run a long-lived plugin-development session rooted at `dir`.
///
/// `dir` follows the same layout `.forge/plugins/` uses: one subdirectory
/// per plugin, each with its own `manifest.toml` (exactly what
/// `PluginLoader::load_all` already scans for). Point it at a scratch
/// directory containing the one plugin you're iterating on, or at
/// `.forge/plugins/` itself to live-reload everything installed there.
///
/// Loads every plugin found, then polls the existing (already fully
/// tested — `crates/nexus-plugins/src/hot_reload.rs`) `HotReloader` /
/// `PluginManager::poll_reloads` machinery every 250ms and hot-swaps the
/// sandbox for any plugin whose `.wasm` changed on disk, printing a line
/// per reload, until Ctrl+C. This is a standalone session — deliberately
/// does not touch a live forge's kernel/storage runtime (dev mode has
/// nothing to do with a specific forge), so it works the same whether or
/// not `--forge-path` even resolves to something real.
///
/// # Errors
/// Returns an error if the plugin manager cannot be created at `dir`
/// (see [`PluginManager::new`]) or if the initial `load_all` scan fails.
pub fn dev(dir: &Path) -> Result<()> {
    let (mut manager, loaded) = start_dev_manager(dir)?;
    if loaded.is_empty() {
        println!("No plugins found under '{}'.", dir.display());
    } else {
        for info in &loaded {
            println!("loaded   {} ({}) [{:?}]", info.id, info.name, info.status);
        }
    }
    println!(
        "Watching '{}' for .wasm changes. Press Ctrl+C to stop.",
        dir.display()
    );

    // A dedicated single-thread runtime, not `App::runtime()` — dev mode
    // doesn't need (and shouldn't require) a live forge's kernel/storage
    // boot just to watch a directory and poll a debounced file-change
    // queue.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to start tokio runtime")?;
    rt.block_on(async {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                    match manager.poll_reloads() {
                        Ok(reloaded) => {
                            for id in reloaded {
                                println!("reloaded {id}");
                            }
                        }
                        Err(e) => println!("reload error: {e}"),
                    }
                }
            }
        }
    });

    println!("Stopped.");
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
        let new_settings: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| anyhow::anyhow!("invalid JSON: {e}"))?;
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

    println!(
        "Scaffolded plugin '{plugin_name}' at '{}':",
        output_dir.display()
    );
    match template {
        PluginTemplate::Script => {
            for f in [
                "plugin.json",
                "index.ts",
                "README.md",
                "package.json",
                "tsconfig.json",
            ] {
                println!("  {}", output_dir.join(f).display());
            }
            println!();
            println!("Next steps:");
            println!(
                "  cd {} && pnpm install && pnpm build",
                output_dir.display()
            );
            println!(
                "  cp index.js plugin.json ~/.nexus-shell/plugins/{}/",
                plugin_id
            );
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
    app.plugins()?.load_all()?;

    // BL-113 Phase 1d/2b/3b — wire any DAP / LSP / MCP contributions
    // the loaded plugins declared so a subcommand that ends up hitting
    // one of those protocol hosts sees the merged set.
    // Best-effort: failures log but do not block the subcommand.
    log_dap_wire_outcomes(app.wire_dap_contributions(), "wired");
    log_lsp_wire_outcomes(app.wire_lsp_contributions(), "wired");
    log_mcp_wire_outcomes(app.wire_mcp_contributions(), "wired");
    log_acp_wire_outcomes(app.wire_acp_contributions(), "wired");

    let plugins = app.plugins()?;
    let args_json = serde_json::json!(args);

    match plugins.dispatch_cli(subcommand, &args_json) {
        Ok(result) => {
            let text = match &result {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
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
                // Plugin manifests are operator-controlled but not
                // necessarily operator-authored — a malicious manifest
                // can embed terminal escape sequences in `id` /
                // `description` to repaint the user's prompt or move
                // the cursor. Strip ANSI before printing into the
                // user's terminal. See issue #85.
                let list = available
                    .iter()
                    .map(|(id, desc)| format!("  {:<20} {}", strip_ansi(id), strip_ansi(desc),))
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
         See docs/planning/PHASE-5-IMPLEMENTATION-PLAN.md.\n\
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
        println!(
            "No shell plugins installed ({} does not exist).",
            dir.display()
        );
        return Ok(());
    }

    let mut rows: Vec<(String, String, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = entry.path().join("plugin.json");
        let (name, version) = if manifest_path.exists() {
            read_plugin_manifest(&manifest_path)
                .unwrap_or_else(|_| (String::from("<unreadable plugin.json>"), String::from("?")))
        } else {
            (String::from("<no plugin.json>"), String::from("?"))
        };
        rows.push((id, name, version));
    }

    if rows.is_empty() {
        println!("No shell plugins installed.");
        return Ok(());
    }

    println!("{:<28} {:<32} Version", "ID", "Name");
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

    std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
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

/// Strip ANSI escape sequences (CSI `\x1b[...`) and other C0/C1
/// control characters from `s`. Plugin metadata flows from
/// untrusted manifests into the user's terminal — embedded escapes
/// could otherwise repaint the prompt, move the cursor, or mask
/// other output. This is intentionally simple (no full ECMA-48
/// state machine): drop anything in the C0 range except tab/newline,
/// drop anything in the C1 range, and drop CSI-style escape
/// sequences via a tiny state machine. See issue #85.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // ESC starts an escape sequence — drop until a final
            // byte (any 0x40-0x7E in CSI, or just one byte in
            // simple escapes like ESC c).
            '\x1b' => {
                if let Some(&next) = chars.peek() {
                    if next == '[' {
                        chars.next();
                        // Consume the parameter bytes (0x30-0x3f) and
                        // intermediate bytes (0x20-0x2f), then the
                        // final byte (0x40-0x7e).
                        for fc in chars.by_ref() {
                            if matches!(fc, '\x40'..='\x7e') {
                                break;
                            }
                        }
                    } else {
                        // Non-CSI escape — drop the next byte too.
                        chars.next();
                    }
                }
            }
            // C0 control codes (drop everything except common
            // whitespace).
            '\x00'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1f' | '\x7f' => {}
            // C1 control codes.
            '\u{80}'..='\u{9f}' => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod strip_ansi_tests {
    use super::strip_ansi;

    #[test]
    fn passes_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strips_csi_color_sequences() {
        // \x1b[31m...\x1b[0m — red text with reset.
        let dirty = "\x1b[31mhello\x1b[0m";
        assert_eq!(strip_ansi(dirty), "hello");
    }

    #[test]
    fn strips_cursor_movement_sequences() {
        // CSI K (erase in line) + CSI 2J (clear screen) + payload.
        let dirty = "before\x1b[K\x1b[2Jafter";
        assert_eq!(strip_ansi(dirty), "beforeafter");
    }

    #[test]
    fn strips_bare_escape_and_simple_escapes() {
        // ESC 7 (DECSC, save cursor).
        assert_eq!(strip_ansi("a\x1b7b"), "ab");
        // Bare ESC followed by nothing — dropped.
        assert_eq!(strip_ansi("a\x1b"), "a");
    }

    #[test]
    fn strips_c0_controls_except_tab_and_newline() {
        // BEL (\x07) and DEL (\x7f) dropped; \t and \n preserved.
        assert_eq!(strip_ansi("a\x07\tb\nc\x7fd"), "a\tb\ncd");
    }

    #[test]
    fn issue_85_malicious_plugin_id_payload() {
        // Realistic shape: a plugin manifest declaring an `id` that
        // claims to be `safe-id` but uses ANSI to overwrite the
        // separator and inject content into help output.
        let dirty = "evil-id\x1b[1A\x1b[2K\x1b[31mPWNED\x1b[0m";
        let cleaned = strip_ansi(dirty);
        assert!(
            !cleaned.contains('\x1b'),
            "no escape bytes must survive; got: {cleaned:?}"
        );
        assert_eq!(cleaned, "evil-idPWNED");
    }
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
        let (name, version) = read_plugin_manifest(&plugin_dir.join("plugin.json")).unwrap();
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

/// C80 — `dev`'s setup half: does it actually turn `hot_reload` on and
/// load what's there. The infinite Ctrl+C loop itself isn't unit-tested
/// here (it drives already-tested `PluginManager::poll_reloads` /
/// `HotReloader` machinery per `crates/nexus-plugins/tests/
/// hot_reload_caps.rs` — see `start_dev_manager`'s doc comment); this
/// covers the wiring that's actually new: the CLI command turning
/// hot-reload on and reporting what got loaded.
#[cfg(test)]
mod dev_tests {
    use super::*;

    fn minimal_plugin_wasm() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../nexus-plugins/tests/fixtures/minimal-plugin.wasm")
    }

    fn write_plugin(dir: &Path, id: &str) {
        let plugin_dir = dir.join(id);
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::copy(minimal_plugin_wasm(), plugin_dir.join("test.wasm")).unwrap();
        std::fs::write(
            plugin_dir.join("manifest.toml"),
            format!(
                r#"
[plugin]
id = "{id}"
name = "Dev Mode Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = []

[wasm]
module = "test.wasm"

[lifecycle]
on_init = false
on_start = false
on_stop = false
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn start_dev_manager_loads_the_one_plugin_under_dir() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "com.test.dev_hot_reload_on");
        let (_manager, loaded) = start_dev_manager(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "com.test.dev_hot_reload_on");
        // Whether hot_reload is actually live (not just that construction
        // and load succeeded, which would pass even with the pre-C80
        // `hot_reload: false` default) is the property
        // `dev_manager_poll_reloads_picks_up_a_touched_wasm` below proves
        // end-to-end.
    }

    #[test]
    fn start_dev_manager_loads_every_plugin_under_dir() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "com.test.dev_multi_a");
        write_plugin(tmp.path(), "com.test.dev_multi_b");
        let (_manager, loaded) = start_dev_manager(tmp.path()).unwrap();
        let mut ids: Vec<&str> = loaded.iter().map(|i| i.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["com.test.dev_multi_a", "com.test.dev_multi_b"]);
    }

    #[test]
    fn start_dev_manager_reports_empty_dir_as_no_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        let (_manager, loaded) = start_dev_manager(tmp.path()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn dev_manager_poll_reloads_picks_up_a_touched_wasm() {
        // End-to-end proof (not just construction): touching the plugin's
        // .wasm after load is actually observed by poll_reloads() through
        // a manager built the same way `dev` builds one. This is the
        // property C80 exists to deliver — reusing the debounced
        // HotReloader that `hot_reload: false` (the pre-C80 CLI default)
        // silently discarded.
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "com.test.dev_reload_probe");
        let (mut manager, loaded) = start_dev_manager(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 1);

        let wasm_path = tmp.path().join("com.test.dev_reload_probe").join("test.wasm");
        // Debounced watcher needs the mtime to actually move and a beat
        // to notice — matches the debounce window `PluginManagerConfig::
        // default().debounce_ms` (500ms) already exercises in
        // `hot_reload_caps.rs`.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::copy(minimal_plugin_wasm(), &wasm_path).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut reloaded_ids = Vec::new();
        while std::time::Instant::now() < deadline && reloaded_ids.is_empty() {
            reloaded_ids = manager.poll_reloads().unwrap();
            if reloaded_ids.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        assert_eq!(reloaded_ids, ["com.test.dev_reload_probe"]);
    }
}
