use std::io::Read as _;

use anyhow::Result;

use crate::app::App;
use crate::output::{print_list, print_success, OutputFormat};

/// Create a new content node at `path`.
pub fn create(app: &mut App, path: &str, content: Option<&str>, stdin: bool) -> Result<()> {
    let body: String = if let Some(text) = content {
        // Interpret escape sequences so users can pass newlines via --content "line1\nline2"
        text.replace("\\n", "\n").replace("\\t", "\t")
    } else if stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
        buf
    } else {
        String::new()
    };

    let storage = app.storage_mut()?;
    let meta = storage
        .write_file(path, body.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to write file '{path}': {e}"))?;

    let format = app.format();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                format,
                &format!("created '{path}'"),
                &serde_json::json!({
                    "path": meta.path,
                    "size_bytes": meta.size_bytes,
                    "content_hash": meta.content_hash,
                    "modified_at": meta.modified_at,
                }),
            );
        }
        _ => {
            println!("Created: {}", meta.path);
            println!("Size   : {} bytes", meta.size_bytes);
            println!("Hash   : {}", meta.content_hash);
        }
    }

    Ok(())
}

/// Read the content node at `path`.
pub fn read(app: &mut App, path: &str, raw: bool) -> Result<()> {
    let storage = app.storage()?;
    let bytes = storage
        .read_file(path)
        .map_err(|e| anyhow::anyhow!("failed to read file '{path}': {e}"))?;

    let text = String::from_utf8_lossy(&bytes);

    if raw {
        print!("{text}");
    } else {
        println!("Path : {path}");
        println!("Size : {} bytes", bytes.len());
        println!("---");
        print!("{text}");
    }

    Ok(())
}

/// Delete the content node at `path`.
pub fn delete(app: &mut App, path: &str, force: bool) -> Result<()> {
    if !force {
        eprint!("Delete '{path}'? [y/N] ");
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

    let storage = app.storage_mut()?;
    storage
        .delete_file(path)
        .map_err(|e| anyhow::anyhow!("failed to delete file '{path}': {e}"))?;

    let format = app.format();

    print_success(
        format,
        &format!("deleted '{path}'"),
        &serde_json::json!({ "path": path }),
    );

    Ok(())
}

/// Search content nodes with `query`, returning up to `limit` results.
pub fn search(app: &mut App, query: &str, limit: usize) -> Result<()> {
    let storage = app.storage_mut()?;

    storage
        .rebuild_search_index()
        .map_err(|e| anyhow::anyhow!("failed to rebuild search index: {e}"))?;

    let results = storage
        .search(query, limit)
        .map_err(|e| anyhow::anyhow!("search failed: {e}"))?;

    let format = app.format();

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    let headers = &["Path", "Score", "Type"];
    let rows: Vec<Vec<String>> = results
        .iter()
        .map(|r| {
            vec![
                r.file_path.clone(),
                format!("{:.4}", r.score),
                r.block_type.clone(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// List tasks across the forge.
pub fn tasks(app: &mut App, completed: bool, all: bool, file: Option<&str>) -> Result<()> {
    let storage = app.storage()?;

    let filter = nexus_storage::TaskFilter {
        completed: if all {
            None
        } else if completed {
            Some(true)
        } else {
            Some(false)
        },
        file_path: file.map(String::from),
    };

    let tasks = storage
        .query_tasks(&filter)
        .map_err(|e| anyhow::anyhow!("failed to query tasks: {e}"))?;

    let format = app.format();

    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }

    let headers = &["ID", "Status", "Content", "File"];
    let rows: Vec<Vec<String>> = tasks
        .iter()
        .map(|t| {
            vec![
                t.id.to_string(),
                if t.completed { "[x]".to_string() } else { "[ ]".to_string() },
                t.content.clone(),
                format!("{}:{}", t.file_path, t.line_number),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Toggle a task's completion state.
pub fn task_toggle(app: &mut App, task_id: u64) -> Result<()> {
    let storage = app.storage_mut()?;

    let record = storage
        .toggle_task(task_id)
        .map_err(|e| anyhow::anyhow!("failed to toggle task {task_id}: {e}"))?;

    let status = if record.completed { "completed" } else { "pending" };
    println!(
        "Task {} toggled to {}: {} ({}:{})",
        record.id, status, record.content, record.file_path, record.line_number
    );

    Ok(())
}

/// Show outgoing links from a file.
pub fn links(app: &mut App, path: &str) -> Result<()> {
    let storage = app.storage()?;
    let outgoing = storage
        .outgoing_links(path)
        .map_err(|e| anyhow::anyhow!("failed to get links: {e}"))?;

    let format = app.format();

    if outgoing.is_empty() {
        println!("No outgoing links.");
        return Ok(());
    }

    let headers = &["Target", "Type", "Text", "Resolved", "Fragment"];
    let rows: Vec<Vec<String>> = outgoing
        .iter()
        .map(|l| {
            vec![
                l.target_path.clone(),
                l.link_type.clone(),
                l.link_text.clone(),
                if l.is_resolved { "yes".to_string() } else { "no".to_string() },
                l.fragment.clone().unwrap_or_default(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show all files that link to the given file.
pub fn backlinks(app: &mut App, path: &str) -> Result<()> {
    let storage = app.storage()?;
    let bl = storage
        .backlinks(path)
        .map_err(|e| anyhow::anyhow!("failed to get backlinks: {e}"))?;

    let format = app.format();

    if bl.is_empty() {
        println!("No backlinks found.");
        return Ok(());
    }

    let headers = &["Source", "Type", "Text"];
    let rows: Vec<Vec<String>> = bl
        .iter()
        .map(|b| {
            vec![
                b.source_path.clone(),
                b.link_type.clone(),
                b.link_text.clone(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Create or open a daily note.
pub fn daily(app: &mut App, date: Option<&str>) -> Result<()> {
    use chrono::{Local, NaiveDate};

    let date = if let Some(d) = date {
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("invalid date format (expected YYYY-MM-DD): {e}"))?
    } else {
        Local::now().date_naive()
    };

    let path = format!("notes/daily/{}.md", date.format("%Y-%m-%d"));

    let storage = app.storage_mut()?;

    // Check if already exists
    if storage.file_exists(&path).unwrap_or(false) {
        println!("Daily note already exists: {path}");
        return Ok(());
    }

    let title = date.format("%B %d, %Y");
    let date_str = date.format("%Y-%m-%d");

    let content = format!(
        "---\ndate: {date_str}\ntags: [daily]\n---\n# {title}\n\n## Tasks\n\n## Notes\n"
    );

    let meta = storage
        .write_file(&path, content.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to create daily note: {e}"))?;

    println!("Created: {}", meta.path);

    Ok(())
}
