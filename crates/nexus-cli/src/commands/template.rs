//! `nexus template …` subcommands. Lists and applies page templates.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nexus_templates::TemplateRegistry;
use serde_json::json;

use crate::app::App;
use crate::output::{print_success, OutputFormat};

/// `nexus template list` — show every template available in the active forge.
pub fn list(app: &App) -> Result<()> {
    let registry = TemplateRegistry::load(app.forge_root())
        .context("failed to load template registry")?;

    let entries = registry.list();

    match app.format() {
        OutputFormat::Json | OutputFormat::Jsonl => {
            let data = json!({
                "templates": entries
                    .iter()
                    .map(|(n, d)| json!({ "name": n, "description": d }))
                    .collect::<Vec<_>>(),
            });
            print_success(app.format(), "templates", &data);
        }
        _ => {
            for (name, desc) in &entries {
                match desc {
                    Some(d) => println!("{name:<30} {d}"),
                    None => println!("{name}"),
                }
            }
        }
    }
    Ok(())
}

/// `nexus template apply <name> --arg key=val …` — render and write a template
/// to disk under the forge root.
pub fn apply(
    app: &App,
    name: &str,
    args: Vec<String>,
    target: Option<PathBuf>,
    overwrite: bool,
    dry_run: bool,
) -> Result<()> {
    let registry = TemplateRegistry::load(app.forge_root())
        .context("failed to load template registry")?;

    let tpl = registry
        .get(name)
        .with_context(|| format!("unknown template '{name}'"))?;

    // Parse `key=value` args.
    let mut kv: BTreeMap<String, String> = BTreeMap::new();
    for raw in args {
        let (k, v) = raw
            .split_once('=')
            .with_context(|| format!("--arg expects key=value, got '{raw}'"))?;
        kv.insert(k.to_string(), v.to_string());
    }

    if let Some(t) = target {
        // CLI override of the template's target_path. Implemented as another
        // value so the template can reference it as `{{__target}}`; for
        // simplicity we just override the path pattern.
        kv.insert("__cli_target".to_string(), t.display().to_string());
    }

    let dest_root = app.forge_root().to_path_buf();

    if dry_run {
        let values = tpl
            .resolve_values(&kv, &dest_root)
            .context("resolving template parameters")?;
        let (body, target_path) = tpl.render(&values).context("rendering template")?;
        println!("--- target path ---");
        println!("{target_path}");
        println!("--- body ---");
        println!("{body}");
        return Ok(());
    }

    let written = tpl
        .apply(&kv, &dest_root, overwrite)
        .with_context(|| format!("applying template '{name}'"))?;

    let summary = format!("created {}", written.display());
    let data = json!({
        "template": name,
        "path": written.display().to_string(),
    });
    print_success(app.format(), &summary, &data);

    Ok(())
}
