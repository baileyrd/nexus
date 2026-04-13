use anyhow::Result;

use crate::app::App;
use crate::output::{print_list, OutputFormat};

/// Show knowledge graph statistics.
pub fn status(app: &mut App) -> Result<()> {
    let storage = app.storage()?;
    let stats = storage
        .graph_stats()
        .map_err(|e| anyhow::anyhow!("failed to get graph stats: {e}"))?;

    let format = app.format();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::json!({
                    "nodes": stats.node_count,
                    "edges": stats.edge_count,
                    "unresolved": stats.unresolved_count,
                })
            );
        }
        _ => {
            println!("Nodes      : {}", stats.node_count);
            println!("Edges      : {}", stats.edge_count);
            println!("Unresolved : {}", stats.unresolved_count);
        }
    }

    Ok(())
}

/// List all unresolved (broken) links.
pub fn unresolved(app: &mut App) -> Result<()> {
    let storage = app.storage()?;
    let links = storage
        .unresolved_links()
        .map_err(|e| anyhow::anyhow!("failed to get unresolved links: {e}"))?;

    let format = app.format();

    if links.is_empty() {
        println!("No unresolved links.");
        return Ok(());
    }

    let headers = &["Target", "Referenced By"];
    let rows: Vec<Vec<String>> = links
        .iter()
        .map(|u| {
            vec![
                u.target_path.clone(),
                u.referenced_by.join(", "),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show neighbors of a file within N hops.
pub fn neighbors(app: &mut App, path: &str, depth: usize) -> Result<()> {
    let storage = app.storage()?;
    let paths = storage
        .graph_neighbors(path, depth)
        .map_err(|e| anyhow::anyhow!("failed to get neighbors: {e}"))?;

    let format = app.format();

    if paths.is_empty() {
        println!("No neighbors found.");
        return Ok(());
    }

    let headers = &["Path"];
    let rows: Vec<Vec<String>> = paths.iter().map(|p| vec![p.clone()]).collect();

    print_list(format, headers, &rows);

    Ok(())
}
