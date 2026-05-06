//! CLI commands for git operations (read-only).

use anyhow::Result;

use nexus_git::{DiffLineKind, GitEngine};

use crate::app::App;

fn open_engine(app: &App) -> Result<GitEngine> {
    GitEngine::open(app.forge_root()).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Show repository info (branch, HEAD, dirty state).
pub fn info(app: &App) -> Result<()> {
    let engine = open_engine(app)?;
    let state = engine.state().map_err(|e| anyhow::anyhow!("{e}"))?;

    let branch = state.branch.as_deref().unwrap_or("(detached)");
    println!("Branch : {branch}");
    println!("HEAD   : {}", state.head_oid);
    println!("Dirty  : {}", if state.is_dirty { "yes" } else { "no" });
    println!("State  : {:?}", state.repo_state);
    Ok(())
}

/// Show file statuses (modified, staged, untracked).
pub fn status(app: &App) -> Result<()> {
    let engine = open_engine(app)?;
    let statuses = engine.file_statuses().map_err(|e| anyhow::anyhow!("{e}"))?;

    if statuses.is_empty() {
        println!("Working tree clean.");
        return Ok(());
    }

    for entry in &statuses {
        println!("{} {}", entry.status.marker(), entry.path.display());
    }
    Ok(())
}

/// Show diff for a file (working tree vs HEAD) or all staged changes.
pub fn diff(app: &App, path: Option<&str>) -> Result<()> {
    let engine = open_engine(app)?;

    if let Some(p) = path {
        let hunks = engine
            .diff_file(std::path::Path::new(p))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        print_hunks(&hunks);
    } else {
        let files = engine.diff_staged().map_err(|e| anyhow::anyhow!("{e}"))?;
        if files.is_empty() {
            println!("No staged changes.");
            return Ok(());
        }
        for (file, hunks) in &files {
            println!("--- {file}");
            print_hunks(hunks);
            println!();
        }
    }
    Ok(())
}

/// Show blame annotations for a file.
pub fn blame(app: &App, path: &str) -> Result<()> {
    let engine = open_engine(app)?;
    let entries = engine
        .blame(std::path::Path::new(path))
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    for entry in &entries {
        let lines = if entry.start_line == entry.end_line {
            format!("L{}", entry.start_line)
        } else {
            format!("L{}-{}", entry.start_line, entry.end_line)
        };
        println!(
            "{} {:>12} {} {:>8}  {}",
            entry.commit_hash,
            entry.author,
            entry.date.format("%Y-%m-%d"),
            lines,
            entry.message,
        );
    }
    Ok(())
}

/// Show commit log.
pub fn log(app: &App, limit: usize, file: Option<&str>) -> Result<()> {
    let engine = open_engine(app)?;

    let entries = if let Some(f) = file {
        engine
            .log_file(std::path::Path::new(f), limit)
            .map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        engine.log(limit).map_err(|e| anyhow::anyhow!("{e}"))?
    };

    if entries.is_empty() {
        println!("No commits.");
        return Ok(());
    }

    for entry in &entries {
        let first_line = entry.message.lines().next().unwrap_or("");
        println!(
            "{} {} {} {}",
            entry.hash,
            entry.date.format("%Y-%m-%d"),
            entry.author,
            first_line,
        );
    }
    Ok(())
}

/// Stage a file or all changes.
pub fn stage(app: &App, path: Option<&str>, all: bool) -> Result<()> {
    let engine = open_engine(app)?;
    if all {
        engine.stage_all().map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("Staged all changes.");
    } else if let Some(p) = path {
        engine
            .stage_file(std::path::Path::new(p))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("Staged: {p}");
    } else {
        anyhow::bail!("Specify a file path or use --all");
    }
    Ok(())
}

/// Unstage a file or all changes.
pub fn unstage(app: &App, path: Option<&str>, all: bool) -> Result<()> {
    let engine = open_engine(app)?;
    if all {
        engine.unstage_all().map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("Unstaged all changes.");
    } else if let Some(p) = path {
        engine
            .unstage_file(std::path::Path::new(p))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("Unstaged: {p}");
    } else {
        anyhow::bail!("Specify a file path or use --all");
    }
    Ok(())
}

/// Stage specific hunks within a file.
pub fn stage_hunk(app: &App, path: &str, hunk_indices: &[usize]) -> Result<()> {
    let engine = open_engine(app)?;
    engine
        .stage_hunks(std::path::Path::new(path), hunk_indices)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let n = hunk_indices.len();
    println!("Staged {n} hunk{} in {path}", if n == 1 { "" } else { "s" });
    Ok(())
}

/// Unstage specific hunks within a file.
pub fn unstage_hunk(app: &App, path: &str, hunk_indices: &[usize]) -> Result<()> {
    let engine = open_engine(app)?;
    engine
        .unstage_hunks(std::path::Path::new(path), hunk_indices)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let n = hunk_indices.len();
    println!("Unstaged {n} hunk{} in {path}", if n == 1 { "" } else { "s" });
    Ok(())
}

/// Create a commit from staged changes.
pub fn commit(app: &App, message: &str) -> Result<()> {
    let engine = open_engine(app)?;
    let hash = engine.commit(message).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("[{hash}] {message}");
    Ok(())
}

/// Branch operations: list, create, switch, delete.
pub fn branch(app: &App, command: Option<crate::BranchCommand>) -> Result<()> {
    let engine = open_engine(app)?;

    match command {
        None => {
            // List branches.
            let branches = engine.branches().map_err(|e| anyhow::anyhow!("{e}"))?;
            for b in &branches {
                let marker = if b.is_head { "* " } else { "  " };
                let upstream = b
                    .upstream
                    .as_deref()
                    .map(|u| format!(" -> {u}"))
                    .unwrap_or_default();
                println!("{marker}{}{upstream}", b.name);
            }
        }
        Some(crate::BranchCommand::Create { name }) => {
            engine
                .create_branch(&name)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("Created branch: {name}");
        }
        Some(crate::BranchCommand::Switch { name }) => {
            engine
                .switch_branch(&name)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("Switched to branch: {name}");
        }
        Some(crate::BranchCommand::Delete { name }) => {
            engine
                .delete_branch(&name)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("Deleted branch: {name}");
        }
    }
    Ok(())
}

/// Fetch refs from a remote.
pub fn fetch(app: &App, remote: &str) -> Result<()> {
    let engine = open_engine(app)?;
    engine.fetch(remote).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Fetched from {remote}.");
    Ok(())
}

/// Push a branch to a remote.
pub fn push(app: &App, remote: &str, branch: Option<&str>) -> Result<()> {
    let engine = open_engine(app)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => engine
            .state()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .branch
            .ok_or_else(|| anyhow::anyhow!("detached HEAD — specify a branch"))?,
    };
    engine
        .push(remote, &branch)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Pushed {branch} to {remote}.");
    Ok(())
}

/// Pull from a remote (fetch + merge).
pub fn pull(app: &App, remote: &str, branch: Option<&str>) -> Result<()> {
    let engine = open_engine(app)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => engine
            .state()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .branch
            .ok_or_else(|| anyhow::anyhow!("detached HEAD — specify a branch"))?,
    };
    let result = engine
        .pull(remote, &branch)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !result.conflicts.is_empty() {
        println!("Pull completed with {} conflict(s):", result.conflicts.len());
        for f in &result.conflicts {
            println!("  C {f}");
        }
        println!("Resolve conflicts then commit, or run: nexus git merge --abort");
    } else if let Some(hash) = &result.commit_hash {
        let kind = if result.fast_forward { "fast-forward" } else { "merge" };
        println!("Pulled {branch} from {remote} ({kind}, {hash}).");
    } else {
        println!("Already up to date.");
    }
    Ok(())
}

/// Merge a branch or abort an in-progress merge.
pub fn merge(app: &App, branch: Option<&str>, abort: bool) -> Result<()> {
    let engine = open_engine(app)?;

    if abort {
        engine.abort_merge().map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("Merge aborted.");
        return Ok(());
    }

    let branch = branch.ok_or_else(|| anyhow::anyhow!("specify a branch to merge"))?;
    let result = engine
        .merge(branch)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !result.conflicts.is_empty() {
        println!("Merge produced {} conflict(s):", result.conflicts.len());
        for f in &result.conflicts {
            println!("  C {f}");
        }
        println!("Resolve conflicts then commit, or run: nexus git merge --abort");
    } else if let Some(hash) = &result.commit_hash {
        let kind = if result.fast_forward { "Fast-forward" } else { "Merge commit" };
        println!("{kind}: {hash}");
    } else {
        println!("Already up to date.");
    }
    Ok(())
}

/// List files with unresolved conflicts.
pub fn conflicts(app: &App) -> Result<()> {
    let engine = open_engine(app)?;
    let files = engine
        .conflict_files()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if files.is_empty() {
        println!("No conflicts.");
    } else {
        for f in &files {
            println!("  C {f}");
        }
    }
    Ok(())
}

/// List configured remotes.
pub fn remotes(app: &App) -> Result<()> {
    let engine = open_engine(app)?;
    let remotes = engine
        .remotes()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if remotes.is_empty() {
        println!("No remotes configured.");
    } else {
        for r in &remotes {
            println!("  {r}");
        }
    }
    Ok(())
}

/// Auto-commit dirty changes (one-shot or watch mode).
pub fn auto_commit(
    app: &App,
    enable: bool,
    disable: bool,
    watch: bool,
    interval: u64,
    debounce: u64,
) -> Result<()> {
    if enable || disable {
        return toggle_auto_commit(app.forge_root(), enable);
    }

    let mut committer = nexus_git::AutoCommitter::new(app.forge_root(), debounce);

    if watch {
        println!("Auto-commit watch mode (interval: {interval}s, debounce: {debounce}s). Ctrl+C to stop.");
        loop {
            match committer.check_and_commit() {
                Ok(result) => {
                    if let Some(hash) = &result.commit_hash {
                        println!(
                            "[{hash}] {} ({} file(s))",
                            result.message.as_deref().unwrap_or("auto-commit"),
                            result.files_changed,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("auto-commit error: {e}");
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(interval));
        }
    } else {
        let result = committer
            .check_and_commit()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(hash) = &result.commit_hash {
            println!(
                "[{hash}] {} ({} file(s))",
                result.message.as_deref().unwrap_or("auto-commit"),
                result.files_changed,
            );
        } else if result.debounced {
            println!("Skipped (debounce window).");
        } else {
            println!("Working tree clean — nothing to commit.");
        }
    }
    Ok(())
}

/// Write `[git] auto_commit = <enable>` to `.forge/app.toml`.
///
/// Reads the existing file as a raw TOML document, updates only the
/// `git.auto_commit` key, and writes it back so other settings are preserved.
fn toggle_auto_commit(forge_root: &std::path::Path, enable: bool) -> Result<()> {
    let dir = forge_root.join(".forge");
    let path = dir.join("app.toml");

    // Load existing content (tolerates missing file).
    let mut table: toml::Table = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).unwrap_or_default()
    } else {
        toml::Table::new()
    };

    // Navigate to [git] section, creating it if absent.
    let git = table
        .entry("git")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(git_table) = git {
        git_table.insert("auto_commit".to_string(), toml::Value::Boolean(enable));
    }

    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, toml::to_string_pretty(&table)?)?;
    println!(
        "Auto-commit {} for this forge. Restart the Nexus kernel to apply.",
        if enable { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn print_hunks(hunks: &[nexus_git::HunkDiff]) {
    for hunk in hunks {
        println!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        );
        for line in &hunk.lines {
            let prefix = match line.kind {
                DiffLineKind::Added => "+",
                DiffLineKind::Removed => "-",
                DiffLineKind::Context => " ",
            };
            print!("{prefix}{}", line.content);
        }
    }
}
