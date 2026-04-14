use std::path::PathBuf;

use anyhow::Result;
use nexus_bootstrap::{init_forge, storage as ipc};

use crate::app::App;
use crate::output::{print_success, OutputFormat};

/// Initialise a new forge, optionally at a specific directory.
///
/// If `dir` is `None` the forge root configured in `app` is used.
/// After creating the `.forge/` structure, runs a full index rebuild
/// so that pre-existing files on disk are immediately visible.
pub fn init(app: &App, dir: Option<PathBuf>) -> Result<()> {
    let target = dir.unwrap_or_else(|| app.forge_root().to_path_buf());

    // Check if the forge already exists.
    if target.join(".forge").exists() {
        anyhow::bail!("forge already exists at '{}'", target.display());
    }

    // Create the `.forge/` directory structure and initial schema. This is
    // the one storage operation that has to precede runtime startup because
    // the storage plugin's `on_init` opens an existing forge.
    init_forge(&target)?;

    // Build a runtime anchored at the new forge and reindex any pre-existing
    // files through the plugin boundary.
    let mut staging = App::new(target.clone(), app.format());
    let (runtime, rt) = staging.runtime()?;
    let stats = ipc::rebuild_index(runtime, rt)
        .map_err(|e| anyhow::anyhow!("failed to index existing files: {e}"))?;

    let location = target.display().to_string();

    match app.format() {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                app.format(),
                &format!("forge initialised at '{location}'"),
                &serde_json::json!({
                    "location": location,
                    "files_indexed": stats.files_processed,
                    "blocks_indexed": stats.blocks_indexed,
                    "links_found": stats.links_found,
                    "tags_found": stats.tags_found,
                }),
            );
        }
        _ => {
            println!("Forge initialised at '{location}'");
            if stats.files_processed > 0 {
                println!("Indexed {} existing files ({} blocks, {} links, {} tags)",
                    stats.files_processed, stats.blocks_indexed,
                    stats.links_found, stats.tags_found);
            }
        }
    }

    Ok(())
}

/// Show the status of the open forge.
pub fn status(app: &mut App) -> Result<()> {
    let format = app.format();
    let location = app.forge_root().display().to_string();
    let (runtime, rt) = app.runtime()?;

    let records = ipc::query_files(runtime, rt)
        .map_err(|e| anyhow::anyhow!("failed to query files: {e}"))?;

    let file_count = records.len();
    let total_size: u64 = records.iter().map(|r| r.size_bytes).sum();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                format,
                "forge status",
                &serde_json::json!({
                    "location": location,
                    "file_count": file_count,
                    "total_size_bytes": total_size,
                }),
            );
        }
        _ => {
            println!("Forge location : {location}");
            println!("Files          : {file_count}");
            println!("Total size     : {total_size} bytes");
        }
    }

    Ok(())
}

/// Rebuild the index from files on disk.
///
/// Clears the existing index and re-indexes every file in the forge,
/// updating blocks, links, tags, and tasks.
pub fn reindex(app: &mut App) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    let stats = ipc::rebuild_index(runtime, rt)
        .map_err(|e| anyhow::anyhow!("reindex failed: {e}"))?;

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                format,
                "reindex complete",
                &serde_json::json!({
                    "files_processed": stats.files_processed,
                    "blocks_indexed": stats.blocks_indexed,
                    "links_found": stats.links_found,
                    "tags_found": stats.tags_found,
                    "duration_ms": stats.duration_ms,
                }),
            );
        }
        _ => {
            println!("Reindex complete in {}ms", stats.duration_ms);
            println!("  Files  : {}", stats.files_processed);
            println!("  Blocks : {}", stats.blocks_indexed);
            println!("  Links  : {}", stats.links_found);
            println!("  Tags   : {}", stats.tags_found);
        }
    }

    Ok(())
}
