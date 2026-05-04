//! `nexus export …` subcommands. Writes forge content out to external
//! formats.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nexus_formats::notion;
use serde_json::json;

use crate::app::App;
use crate::output::print_success;

/// Export a forge subdirectory to a Notion-compatible folder tree.
///
/// `source` is interpreted relative to the forge root if not absolute.
/// Defaults to the forge root when not provided.
pub fn notion_dir(app: &App, source: Option<PathBuf>, dest: &Path) -> Result<()> {
    let source_abs = match source {
        Some(s) if s.is_absolute() => s,
        Some(s) => app.forge_root().join(s),
        None => app.forge_root().to_path_buf(),
    };
    if !source_abs.is_dir() {
        anyhow::bail!("source is not a directory: {}", source_abs.display());
    }

    let report = notion::export_to_notion(&source_abs, dest)
        .context("notion export failed")?;

    let summary = format!(
        "exported {pages} pages, {dbs} databases, {att} attachments → {path}{warn}",
        pages = report.pages_written,
        dbs = report.databases_written,
        att = report.attachments_copied,
        path = dest.display(),
        warn = if report.warnings.is_empty() {
            String::new()
        } else {
            format!(" ({} warning(s))", report.warnings.len())
        }
    );
    let data = json!({
        "pages_written": report.pages_written,
        "databases_written": report.databases_written,
        "attachments_copied": report.attachments_copied,
        "dest": dest.display().to_string(),
        "warnings": report.warnings,
    });
    print_success(app.format(), &summary, &data);

    for w in &report.warnings {
        eprintln!("warning: {w}");
    }

    Ok(())
}
