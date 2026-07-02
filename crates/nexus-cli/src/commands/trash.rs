//! C3 (#356) — `nexus trash list|restore|empty` over the
//! `com.nexus.storage::trash_*` IPC handlers.

use anyhow::Result;
use nexus_bootstrap::storage as ipc;

use crate::app::App;
use crate::output::print_success;

/// List trashed entries, newest first.
pub fn list(app: &mut App) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let entries = rt
        .block_on(ipc::trash_list(&*invoker))
        .map_err(|e| anyhow::anyhow!("failed to list trash: {e}"))?;

    if matches!(format, crate::output::OutputFormat::Json) {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "entries": entries
                    .iter()
                    .map(|e| serde_json::json!({
                        "trash_id": e.trash_id,
                        "original_path": e.original_path,
                        "deleted_at_ms": e.deleted_at_ms,
                        "is_dir": e.is_dir,
                        "size_bytes": e.size_bytes,
                    }))
                    .collect::<Vec<_>>(),
            }))?
        );
        return Ok(());
    }

    if entries.is_empty() {
        println!("Trash is empty.");
        return Ok(());
    }
    println!("{:<18} {:<6} {:>10}  PATH", "TRASH-ID", "KIND", "SIZE");
    for e in entries {
        println!(
            "{:<18} {:<6} {:>10}  {}",
            e.trash_id,
            if e.is_dir { "dir" } else { "file" },
            e.size_bytes,
            e.original_path,
        );
    }
    Ok(())
}

/// Restore a trashed entry to its original path.
pub fn restore(app: &mut App, trash_id: &str) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let restored = rt
        .block_on(ipc::trash_restore(&*invoker, trash_id))
        .map_err(|e| anyhow::anyhow!("failed to restore '{trash_id}': {e}"))?;
    print_success(
        format,
        &format!("restored '{restored}'"),
        &serde_json::json!({ "restored_path": restored }),
    );
    Ok(())
}

/// Permanently delete trashed entries.
pub fn empty(app: &mut App, older_than_days: Option<u64>, force: bool) -> Result<()> {
    if !force {
        let scope = older_than_days
            .map(|d| format!(" older than {d} day(s)"))
            .unwrap_or_default();
        eprint!("Permanently delete trashed entries{scope}? [y/N] ");
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
        let trimmed = answer.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let removed = rt
        .block_on(ipc::trash_empty(&*invoker, older_than_days))
        .map_err(|e| anyhow::anyhow!("failed to empty trash: {e}"))?;
    print_success(
        format,
        &format!("removed {removed} trashed entr{}", if removed == 1 { "y" } else { "ies" }),
        &serde_json::json!({ "removed": removed }),
    );
    Ok(())
}
