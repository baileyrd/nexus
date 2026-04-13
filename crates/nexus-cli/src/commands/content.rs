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
