//! `nexus import …` subcommands. Imports external knowledge-tool exports
//! into the active forge.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nexus_formats::notion;

use crate::app::App;
use crate::output::print_success;
use serde_json::json;

/// Import a Notion zip-export into the active forge.
///
/// `dest` is interpreted relative to the forge root if not absolute. The
/// destination directory is created if missing; existing files are kept and
/// new files are renamed with a `(N)` suffix on collision.
pub fn notion_zip(app: &App, source: &Path, dest: Option<PathBuf>) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("source zip not found: {}", source.display());
    }

    let dest_abs = match dest {
        Some(d) if d.is_absolute() => d,
        Some(d) => app.forge_root().join(d),
        None => app.forge_root().join("Imported from Notion"),
    };

    let report = notion::import_notion_zip(source, &dest_abs).context("notion import failed")?;

    let summary = format!(
        "imported {pages} pages, {bases} databases, {attach} attachments → {path}{warn}",
        pages = report.pages_written,
        bases = report.bases_written,
        attach = report.attachments_copied,
        path = dest_abs.display(),
        warn = if report.warnings.is_empty() {
            String::new()
        } else {
            format!(" ({} warning(s))", report.warnings.len())
        }
    );
    let data = json!({
        "pages_written": report.pages_written,
        "bases_written": report.bases_written,
        "attachments_copied": report.attachments_copied,
        "dest": dest_abs.display().to_string(),
        "warnings": report.warnings,
    });
    print_success(app.format(), &summary, &data);

    for w in &report.warnings {
        eprintln!("warning: {w}");
    }

    Ok(())
}
