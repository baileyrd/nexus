//! `com.nexus.workflow::templates_*` handlers — list, fetch, and
//! instantiate built-in workflow templates. Lifted out of
//! `core_plugin.rs` by the BL-137 oversized-file decomposition.

use std::path::Path;

use nexus_plugins::PluginError;

use crate::core_plugin::{GetTemplateArgs, InitTemplateArgs};
use crate::templates;

use super::shared::{exec_err, parse};

#[allow(clippy::unnecessary_wraps)] // dispatcher contract returns Result
pub(crate) fn handle_list() -> Result<serde_json::Value, PluginError> {
    let entries: Vec<_> = templates::CATALOG
        .iter()
        .map(|t| {
            serde_json::json!({
                "slug": t.slug,
                "description": t.description,
                "tags": t.tags,
                "filename": t.filename,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(entries))
}

pub(crate) fn handle_get(
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GetTemplateArgs = parse(args, "templates_get")?;
    let t = templates::find(&a.slug)
        .ok_or_else(|| exec_err(format!("no template named '{}'", a.slug)))?;
    Ok(serde_json::json!({
        "slug": t.slug,
        "description": t.description,
        "tags": t.tags,
        "filename": t.filename,
        "body": t.body,
    }))
}

pub(crate) fn handle_init(
    root: &Path,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: InitTemplateArgs = parse(args, "templates_init")?;
    let t = templates::find(&a.slug)
        .ok_or_else(|| exec_err(format!("no template named '{}'", a.slug)))?;
    let filename = a
        .filename
        .as_deref()
        .map(sanitize_filename)
        .transpose()
        .map_err(exec_err)?
        .unwrap_or_else(|| t.filename.to_string());
    let target = root.join(&filename);
    if target.exists() && !a.overwrite {
        return Err(exec_err(format!(
            "templates_init: '{}' already exists (pass overwrite=true to replace)",
            target.display()
        )));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| exec_err(format!("templates_init: create_dir_all: {e}")))?;
    }
    std::fs::write(&target, t.body)
        .map_err(|e| exec_err(format!("templates_init: write: {e}")))?;
    Ok(serde_json::json!({
        "written": true,
        "path": target.to_string_lossy(),
        "slug": t.slug,
    }))
}

/// Defensive filename check for `templates_init`. Rejects path
/// separators and parent-dir hops so a malicious caller can't write
/// outside `<forge>/.workflows/`. Empty / whitespace-only names also
/// fail. Allowed: `<basename>` or `<basename>.workflow.toml`.
fn sanitize_filename(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("templates_init: filename cannot be empty".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err(format!(
            "templates_init: filename '{trimmed}' must be a bare basename (no path separators)"
        ));
    }
    Ok(trimmed.to_string())
}
