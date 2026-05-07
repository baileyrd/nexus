use std::path::{Path, PathBuf};

use anyhow::Result;
use nexus_bootstrap::{init_forge, storage as ipc};

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
        match name {
            "os" => scaffold_os_template(&target)?,
            other => anyhow::bail!("unknown forge template '{other}' (expected: os)"),
        }
    }

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

/// Import another forge into this one (BL-083).
pub fn import(
    app: &mut App,
    source: &std::path::Path,
    dry_run: bool,
    on_conflict: &str,
) -> Result<()> {
    use std::time::Duration;
    use nexus_kernel::PluginContext;
    let abs = source
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("import source '{}': {e}", source.display()))?;
    let (runtime, rt) = app.runtime()?;
    let resp = rt
        .block_on(runtime.context.ipc_call(
            "com.nexus.storage",
            "import_forge",
            serde_json::json!({
                "source": abs.to_string_lossy(),
                "dry_run": dry_run,
                "on_conflict": on_conflict,
            }),
            Duration::from_secs(600),
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

// ---------------------------------------------------------------------------
// BL-054 Phase 1 — Forge OS template
// ---------------------------------------------------------------------------

/// Top-level directories the OS template lays down. `projects/<name>/`
/// is created lazily by the user when a project starts; the empty
/// `projects/` parent gets a `.gitkeep` so the directory is preserved
/// in version control.
const OS_DIRS: &[&str] = &[
    "raw",
    "wiki",
    "output",
    "projects",
    "ops",
    "personal",
    "archive",
];

/// Files seeded into the forge root. `architecture.md` is intentionally
/// a placeholder — Phase 5's OS Setup skill fills it in by interviewing
/// the user. `CLAUDE.md` documents the memory map so the AI navigates
/// without burning tokens guessing the layout.
const OS_CLAUDE_MD: &str = include_str!("../../templates/os/CLAUDE.md");
const OS_ARCHITECTURE_MD: &str = include_str!("../../templates/os/architecture.md");

fn scaffold_os_template(root: &Path) -> Result<()> {
    for dir in OS_DIRS {
        let path = root.join(dir);
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow::anyhow!("create '{}': {e}", path.display()))?;
        // Empty directories don't survive `git add`; drop a .gitkeep so
        // the OS layout round-trips through version control.
        let keep = path.join(".gitkeep");
        if !keep.exists() {
            std::fs::write(&keep, b"")
                .map_err(|e| anyhow::anyhow!("write '{}': {e}", keep.display()))?;
        }
    }

    // Root files. Don't overwrite a CLAUDE.md the user already wrote
    // (forge::init's outer guard rejects pre-existing forges, but a
    // CLAUDE.md sitting in the directory before init isn't a forge —
    // it's somebody else's content).
    write_if_absent(&root.join("CLAUDE.md"), OS_CLAUDE_MD)?;
    write_if_absent(&root.join("architecture.md"), OS_ARCHITECTURE_MD)?;
    Ok(())
}

fn write_if_absent(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("write '{}': {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_creates_all_directories() {
        let tmp = tempfile::tempdir().unwrap();
        scaffold_os_template(tmp.path()).unwrap();
        for dir in OS_DIRS {
            let path = tmp.path().join(dir);
            assert!(path.is_dir(), "expected directory at {}", path.display());
            assert!(
                path.join(".gitkeep").exists(),
                ".gitkeep missing in {}",
                path.display(),
            );
        }
    }

    #[test]
    fn scaffold_seeds_root_files() {
        let tmp = tempfile::tempdir().unwrap();
        scaffold_os_template(tmp.path()).unwrap();
        let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert!(claude.contains("Memory map"), "CLAUDE.md missing memory map header");
        assert!(claude.contains("raw/"), "CLAUDE.md missing raw/ section");
        let arch = std::fs::read_to_string(tmp.path().join("architecture.md")).unwrap();
        assert!(arch.contains("Architecture"), "architecture.md missing Architecture header");
    }

    #[test]
    fn scaffold_preserves_pre_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("CLAUDE.md");
        std::fs::write(&claude, "user-authored content").unwrap();
        scaffold_os_template(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&claude).unwrap(), "user-authored content");
    }

    #[test]
    fn scaffold_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        scaffold_os_template(tmp.path()).unwrap();
        // Second run must not error or duplicate.
        scaffold_os_template(tmp.path()).unwrap();
        for dir in OS_DIRS {
            assert!(tmp.path().join(dir).is_dir());
        }
    }
}
