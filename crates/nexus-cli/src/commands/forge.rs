use std::path::PathBuf;

use anyhow::Result;
use nexus_storage::{StorageConfig, StorageEngine};

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

    // Initialise the forge via App::init_forge, which wraps StorageEngine::init.
    let tmp_app = App::new(target.clone(), app.format());
    tmp_app.init_forge()?;

    // Open the freshly-created forge and reconcile any pre-existing files.
    let engine = StorageEngine::open(&target, &StorageConfig::default())
        .map_err(|e| anyhow::anyhow!("failed to open new forge for indexing: {e}"))?;
    let stats = engine
        .rebuild_index()
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
    let storage = app.storage()?;

    let records = storage
        .query_files(&nexus_storage::FileFilter::default())
        .map_err(|e| anyhow::anyhow!("failed to query files: {e}"))?;

    let file_count = records.len();
    let total_size: u64 = records.iter().map(|r| r.size_bytes).sum();
    let location = app.forge_root().display().to_string();

    let format = app.format();

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
    let storage = app.storage()?;
    let stats = storage
        .rebuild_index()
        .map_err(|e| anyhow::anyhow!("reindex failed: {e}"))?;

    match app.format() {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                app.format(),
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
