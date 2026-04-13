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
