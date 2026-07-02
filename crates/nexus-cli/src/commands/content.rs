use std::io::Read as _;

use anyhow::Result;
use nexus_bootstrap::storage::{self as ipc, TaskFilter};

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

    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let meta = rt
        .block_on(ipc::write_file(&*invoker, path, body.as_bytes()))
        .map_err(|e| anyhow::anyhow!("failed to write file '{path}': {e}"))?;

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

/// Update an existing content node at `path`, overwriting its contents.
///
/// Mirrors the `nexus_update_note` MCP tool: under the hood the kernel IPC
/// (`storage::write_file`) is the same as `create`, but the subcommand is
/// exposed separately so the CLI surface maps 1:1 to the MCP tool set.
pub fn update(app: &mut App, path: &str, content: Option<&str>, stdin: bool) -> Result<()> {
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

    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let meta = rt
        .block_on(ipc::write_file(&*invoker, path, body.as_bytes()))
        .map_err(|e| anyhow::anyhow!("failed to update file '{path}': {e}"))?;

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            print_success(
                format,
                &format!("updated '{path}'"),
                &serde_json::json!({
                    "path": meta.path,
                    "size_bytes": meta.size_bytes,
                    "content_hash": meta.content_hash,
                    "modified_at": meta.modified_at,
                }),
            );
        }
        _ => {
            println!("Updated: {}", meta.path);
            println!("Size   : {} bytes", meta.size_bytes);
            println!("Hash   : {}", meta.content_hash);
        }
    }

    Ok(())
}

/// List every content node, optionally filtered by a path prefix.
///
/// Mirrors the `nexus_list_notes` MCP tool (kernel IPC `storage::query_files`).
pub fn list(app: &mut App, prefix: Option<&str>) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let records = rt
        .block_on(ipc::query_files_with_prefix(
            &*invoker,
            prefix.unwrap_or(""),
        ))
        .map_err(|e| anyhow::anyhow!("failed to list files: {e}"))?;

    if records.is_empty() {
        if matches!(format, OutputFormat::Text) {
            println!("No files found.");
        } else {
            print_list(format, &["Path", "Size", "Modified"], &[]);
        }
        return Ok(());
    }

    match format {
        OutputFormat::Text => {
            // One path per line is the most script-friendly representation for
            // the default output mode (matches e.g. `ls` style listings).
            for r in &records {
                println!("{}", r.path);
            }
        }
        _ => {
            let headers = &["Path", "Size", "Modified"];
            let rows: Vec<Vec<String>> = records
                .iter()
                .map(|r| {
                    vec![
                        r.path.clone(),
                        r.size_bytes.to_string(),
                        r.modified_at.to_string(),
                    ]
                })
                .collect();
            print_list(format, headers, &rows);
        }
    }

    Ok(())
}

/// Read the content node at `path`.
pub fn read(app: &mut App, path: &str, raw: bool) -> Result<()> {
    let (invoker, rt) = app.invoker()?;
    let bytes = rt
        .block_on(ipc::read_file(&*invoker, path))
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

/// Delete the content node at `path`. Moves to the forge trash by
/// default (C3 / #356 — restorable via `nexus trash restore`); pass
/// `permanent` for the unrecoverable pre-C3 behaviour.
pub fn delete(app: &mut App, path: &str, force: bool, permanent: bool) -> Result<()> {
    if !force {
        let verb = if permanent {
            "Permanently delete"
        } else {
            "Move to trash"
        };
        eprint!("{verb} '{path}'? [y/N] ");
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
    if permanent {
        rt.block_on(ipc::delete_file(&*invoker, path))
            .map_err(|e| anyhow::anyhow!("failed to delete file '{path}': {e}"))?;
        print_success(
            format,
            &format!("deleted '{path}'"),
            &serde_json::json!({ "path": path }),
        );
    } else {
        let trash_id = rt
            .block_on(ipc::trash_entry(&*invoker, path, "forge"))
            .map_err(|e| anyhow::anyhow!("failed to trash '{path}': {e}"))?;
        print_success(
            format,
            &format!(
                "moved '{path}' to trash{}",
                trash_id
                    .as_deref()
                    .map(|id| format!(" (restore with `nexus trash restore {id}`)"))
                    .unwrap_or_default()
            ),
            &serde_json::json!({ "path": path, "trash_id": trash_id }),
        );
    }

    Ok(())
}

/// Search content nodes with `query`, returning up to `limit` results.
pub fn search(app: &mut App, query: &str, limit: usize) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;

    rt.block_on(ipc::rebuild_search_index(&*invoker))
        .map_err(|e| anyhow::anyhow!("failed to rebuild search index: {e}"))?;

    let results = rt
        .block_on(ipc::search(&*invoker, query, limit))
        .map_err(|e| anyhow::anyhow!("search failed: {e}"))?;

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
    let format = app.format();
    let filter = TaskFilter {
        completed: if all {
            None
        } else if completed {
            Some(true)
        } else {
            Some(false)
        },
        file_path: file.map(String::from),
    };

    let (invoker, rt) = app.invoker()?;
    let tasks = rt
        .block_on(ipc::query_tasks(&*invoker, &filter))
        .map_err(|e| anyhow::anyhow!("failed to query tasks: {e}"))?;

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
                if t.completed {
                    "[x]".to_string()
                } else {
                    "[ ]".to_string()
                },
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
    let (invoker, rt) = app.invoker()?;
    let record = rt
        .block_on(ipc::toggle_task(&*invoker, task_id))
        .map_err(|e| anyhow::anyhow!("failed to toggle task {task_id}: {e}"))?;

    let status = if record.completed {
        "completed"
    } else {
        "pending"
    };
    println!(
        "Task {} toggled to {}: {} ({}:{})",
        record.id, status, record.content, record.file_path, record.line_number
    );

    Ok(())
}

/// Show outgoing links from a file.
pub fn links(app: &mut App, path: &str) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let outgoing = rt
        .block_on(ipc::outgoing_links(&*invoker, path))
        .map_err(|e| anyhow::anyhow!("failed to get links: {e}"))?;

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
                if l.is_resolved {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
                l.fragment.clone().unwrap_or_default(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show all files that link to the given file.
pub fn backlinks(app: &mut App, path: &str) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let bl = rt
        .block_on(ipc::backlinks(&*invoker, path))
        .map_err(|e| anyhow::anyhow!("failed to get backlinks: {e}"))?;

    if bl.is_empty() {
        println!("No backlinks found.");
        return Ok(());
    }

    let headers = &["Source", "Text"];
    let rows: Vec<Vec<String>> = bl
        .iter()
        .map(|b| vec![b.source_path.clone(), b.link_text.clone()])
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Export a note to HTML.
pub fn export(app: &mut App, path: &str, output: Option<&str>) -> Result<()> {
    let (invoker, rt) = app.invoker()?;
    let bytes = rt
        .block_on(ipc::read_file(&*invoker, path))
        .map_err(|e| anyhow::anyhow!("failed to read file '{path}': {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let title = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let html = nexus_bootstrap::export_to_html(&text, title);
    if let Some(out_path) = output {
        std::fs::write(out_path, &html)
            .map_err(|e| anyhow::anyhow!("failed to write '{out_path}': {e}"))?;
        println!("Exported to {out_path}");
    } else {
        print!("{html}");
    }
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

    let (invoker, rt) = app.invoker()?;

    // Check if already exists
    if rt
        .block_on(ipc::file_exists(&*invoker, &path))
        .unwrap_or(false)
    {
        println!("Daily note already exists: {path}");
        return Ok(());
    }

    let title = date.format("%B %d, %Y");
    let date_str = date.format("%Y-%m-%d");

    let content =
        format!("---\ndate: {date_str}\ntags: [daily]\n---\n# {title}\n\n## Tasks\n\n## Notes\n");

    let meta = rt
        .block_on(ipc::write_file(&*invoker, &path, content.as_bytes()))
        .map_err(|e| anyhow::anyhow!("failed to create daily note: {e}"))?;

    println!("Created: {}", meta.path);

    Ok(())
}
