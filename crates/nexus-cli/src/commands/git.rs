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
