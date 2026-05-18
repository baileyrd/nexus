use std::path::PathBuf;

use anyhow::Result;
use nexus_bootstrap::{
    forge_template::{apply as apply_template, ForgeTemplate},
    init_forge, storage as ipc,
};

use crate::app::App;
use crate::output::{print_success, OutputFormat};

/// Initialise a new forge, optionally at a specific directory.
///
/// If `dir` is `None` the forge root configured in `app` is used.
/// `template` selects an optional scaffold (currently only `"os"`,
/// which lays out the BL-054 Phase 1 directory structure + memory-map
/// `CLAUDE.md` before the index rebuild runs).
///
/// After creating the `.forge/` structure, runs a full index rebuild
/// so that pre-existing files on disk are immediately visible — when
/// the OS template seeded files, those land in the index in the same
/// pass.
pub fn init(app: &App, dir: Option<PathBuf>, template: Option<&str>) -> Result<()> {
    let target = dir.unwrap_or_else(|| app.forge_root().to_path_buf());

    // Check if the forge already exists.
    if target.join(".forge").exists() {
        anyhow::bail!("forge already exists at '{}'", target.display());
    }

    // Create the `.forge/` directory structure and initial schema. This is
    // the one storage operation that has to precede runtime startup because
    // the storage plugin's `on_init` opens an existing forge.
    init_forge(&target)?;

    // Apply the requested scaffold template, if any. Done before the
    // reindex so the seeded files end up in the index automatically.
    if let Some(name) = template {
        let kind = ForgeTemplate::from_str(name)
            .ok_or_else(|| anyhow::anyhow!("unknown forge template '{name}' (expected: os)"))?;
        apply_template(&target, kind)?;
    }

    // Build a runtime anchored at the new forge and reindex any pre-existing
    // files through the plugin boundary.
    let mut staging = App::new(target.clone(), app.format());
    let (invoker, rt) = staging.invoker()?;
    let stats = rt
        .block_on(ipc::rebuild_index(&*invoker))
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
    let (invoker, rt) = app.invoker()?;

    let records = rt
        .block_on(ipc::query_files(&*invoker))
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

/// Import another forge into this one (BL-083).
pub fn import(
    app: &mut App,
    source: &std::path::Path,
    dry_run: bool,
    on_conflict: &str,
) -> Result<()> {
    use nexus_types::constants::IPC_TIMEOUT_EXTENDED;
    use nexus_types::plugin_ids;
    let abs = source
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("import source '{}': {e}", source.display()))?;
    let (invoker, rt) = app.invoker()?;
    let resp = rt
        .block_on(invoker.ipc_call(
            plugin_ids::STORAGE,
            "import_forge",
            serde_json::json!({
                "source": abs.to_string_lossy(),
                "dry_run": dry_run,
                "on_conflict": on_conflict,
            }),
            IPC_TIMEOUT_EXTENDED,
        ))
        .map_err(|e| anyhow::anyhow!("import_forge ipc: {e}"))?;

    if dry_run {
        let copies = resp
            .get("copies")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let skips = resp
            .get("skips_identical")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let conflicts = resp
            .get("conflicts")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        println!("Import plan ({}):", abs.display());
        println!(
            "  copies:    {} new file(s)",
            copies.len()
        );
        println!("  identical: {} file(s) (no action)", skips.len());
        println!("  conflicts: {} (strategy = {on_conflict})", conflicts.len());
        if !conflicts.is_empty() {
            println!("\nConflicting paths:");
            for c in &conflicts {
                if let Some(p) = c.get("relpath").and_then(serde_json::Value::as_str) {
                    println!("  {p}");
                }
            }
        }
        return Ok(());
    }

    let copied = resp.get("copied").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
    let overwritten = resp
        .get("overwritten")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let renamed = resp
        .get("renamed")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let skipped_conflicts = resp
        .get("skipped_conflicts")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let skipped_identical = resp
        .get("skipped_identical")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    println!("Import complete:");
    println!("  copied:            {copied}");
    println!("  overwritten:       {overwritten}");
    println!("  renamed:           {renamed}");
    println!("  skipped (identical): {skipped_identical}");
    println!("  skipped (conflict):  {skipped_conflicts}");
    Ok(())
}

/// BL-137 — diagnostic walk of `<forge>` vs. the SQLite index. Reports
/// files on disk but not indexed (`missing`), files in the index but
/// missing on disk (`stale`), and entries where the on-disk `mtime`
/// disagrees with the indexed `modified_at` (`mtime_drift`).
///
/// Read-only by default. Pass `fix = true` to invoke
/// `com.nexus.storage::rebuild_index` after the report when any drift
/// is found.
pub fn doctor(app: &mut App, fix: bool) -> Result<()> {
    let format = app.format();
    let forge_root = app.forge_root().to_path_buf();
    let (invoker, rt) = app.invoker()?;

    let indexed = rt
        .block_on(ipc::query_files(&*invoker))
        .map_err(|e| anyhow::anyhow!("doctor: query_files: {e}"))?;
    let indexed_by_path: std::collections::HashMap<&str, &ipc::FileRecord> = indexed
        .iter()
        .map(|r| (r.path.as_str(), r))
        .collect();

    let on_disk = walk_markdown_files(&forge_root)?;
    let on_disk_set: std::collections::HashSet<&str> =
        on_disk.iter().map(|(rel, _)| rel.as_str()).collect();

    let mut missing: Vec<String> = on_disk
        .iter()
        .filter(|(rel, _)| !indexed_by_path.contains_key(rel.as_str()))
        .map(|(rel, _)| rel.clone())
        .collect();
    missing.sort();

    let mut stale: Vec<String> = indexed
        .iter()
        .filter(|r| !on_disk_set.contains(r.path.as_str()))
        .map(|r| r.path.clone())
        .collect();
    stale.sort();

    let mut mtime_drift: Vec<(String, i64, i64)> = Vec::new();
    for (rel, disk_mtime) in &on_disk {
        if let Some(rec) = indexed_by_path.get(rel.as_str()) {
            // Skip if the index has no recorded mtime (defaults to 0).
            if rec.modified_at != 0 && rec.modified_at != *disk_mtime {
                mtime_drift.push((rel.clone(), *disk_mtime, rec.modified_at));
            }
        }
    }
    mtime_drift.sort_by(|a, b| a.0.cmp(&b.0));

    let drifted = !missing.is_empty() || !stale.is_empty() || !mtime_drift.is_empty();

    // Report ---------------------------------------------------------------
    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                format,
                "forge doctor",
                &serde_json::json!({
                    "location": forge_root.display().to_string(),
                    "files_on_disk":   on_disk.len(),
                    "files_indexed":   indexed.len(),
                    "missing":         missing,
                    "stale":           stale,
                    "mtime_drift":     mtime_drift
                        .iter()
                        .map(|(p, d, i)| serde_json::json!({
                            "path": p, "disk_mtime": d, "index_mtime": i
                        }))
                        .collect::<Vec<_>>(),
                    "drift": drifted,
                }),
            );
        }
        _ => {
            println!("Forge location : {}", forge_root.display());
            println!("Files on disk  : {}", on_disk.len());
            println!("Files indexed  : {}", indexed.len());
            if missing.is_empty() && stale.is_empty() && mtime_drift.is_empty() {
                println!("No drift detected.");
            } else {
                if !missing.is_empty() {
                    println!("\nMissing from index ({}):", missing.len());
                    for p in &missing {
                        println!("  {p}");
                    }
                }
                if !stale.is_empty() {
                    println!("\nStale in index ({}):", stale.len());
                    for p in &stale {
                        println!("  {p}");
                    }
                }
                if !mtime_drift.is_empty() {
                    println!("\nmtime drift ({}):", mtime_drift.len());
                    for (p, disk, idx) in &mtime_drift {
                        println!("  {p}  disk={disk} index={idx}");
                    }
                }
            }
        }
    }

    if fix && drifted {
        let stats = rt
            .block_on(ipc::rebuild_index(&*invoker))
            .map_err(|e| anyhow::anyhow!("doctor --fix: rebuild_index: {e}"))?;
        match format {
            OutputFormat::Json | OutputFormat::Jsonl => {
                print_success(
                    format,
                    "forge doctor --fix",
                    &serde_json::json!({
                        "files_processed": stats.files_processed,
                        "blocks_indexed":  stats.blocks_indexed,
                        "duration_ms":     stats.duration_ms,
                    }),
                );
            }
            _ => {
                println!(
                    "\nReindexed {} file(s) in {}ms.",
                    stats.files_processed, stats.duration_ms
                );
            }
        }
    }

    Ok(())
}

/// Walk `forge_root` for `*.md` files, returning `(forge-relative path,
/// mtime-seconds)` tuples. Skips `.forge/`, `.git/`, and any other
/// dot-prefixed directory at any depth — mirrors the storage engine's
/// ignore rules well enough for the doctor's drift report without
/// pulling in `nexus-storage` directly.
fn walk_markdown_files(forge_root: &std::path::Path) -> Result<Vec<(String, i64)>> {
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![forge_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for ent in entries.flatten() {
            let path = ent.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            // Skip hidden directories (`.forge`, `.git`, etc.) at every level.
            if name.starts_with('.') {
                continue;
            }
            let file_type = match ent.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let rel = match path.strip_prefix(forge_root) {
                Ok(p) => p
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
                Err(_) => continue,
            };
            let mtime = ent
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            out.push((rel, mtime));
        }
    }
    Ok(out)
}

/// Rebuild the index from files on disk.
///
/// Clears the existing index and re-indexes every file in the forge,
/// updating blocks, links, tags, and tasks.
pub fn reindex(app: &mut App) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let stats = rt
        .block_on(ipc::rebuild_index(&*invoker))
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

