use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::app::App;

/// Return the path to the logs directory.
fn logs_dir(app: &App) -> PathBuf {
    app.forge_root().join(".forge").join("logs")
}

/// Stream the most recent log entries, optionally filtered by `level`.
pub fn tail(app: &App, level: Option<&str>, lines: usize) -> Result<()> {
    let dir = logs_dir(app);
    if !dir.exists() {
        println!("No log files found.");
        return Ok(());
    }

    // Collect .log files and sort by name (lexicographic == date order for
    // nexus-YYYY-MM-DD.log naming).
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("log"))
        .collect();

    if entries.is_empty() {
        println!("No log files found.");
        return Ok(());
    }

    entries.sort();
    let most_recent = entries.last().expect("non-empty");

    let content = fs::read_to_string(most_recent)?;
    let all_lines: Vec<&str> = content.lines().collect();

    let tail_lines: Vec<&&str> = all_lines.iter().rev().take(lines).collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let level_upper = level.map(|l| l.to_uppercase());

    for line in tail_lines {
        if let Some(ref lvl) = level_upper
            && !line.to_uppercase().contains(lvl.as_str())
        {
            continue;
        }
        println!("{line}");
    }

    Ok(())
}

/// Show logs for the given `date` (YYYY-MM-DD format).
pub fn show(app: &App, date: &str) -> Result<()> {
    let log_file = logs_dir(app).join(format!("nexus-{date}.log"));
    if !log_file.exists() {
        anyhow::bail!("no log file for date: {date}");
    }
    let content = fs::read_to_string(&log_file)?;
    print!("{content}");
    Ok(())
}

/// Print the path to the log directory.
pub fn path(app: &App) -> Result<()> {
    println!("{}", logs_dir(app).display());
    Ok(())
}
