//! CLI commands for canvas file operations.

use anyhow::Result;

use crate::app::App;
use crate::output;

/// Create a new empty canvas file.
pub fn create(app: &mut App, path: &str) -> Result<()> {
    let canvas = nexus_storage::CanvasFile {
        nodes: vec![],
        edges: vec![],
    };
    let json = nexus_storage::serialize_canvas(&canvas)?;
    app.storage_mut()?.write_file(path, json.as_bytes())?;
    output::print_success(app.format(), &format!("Created canvas: {path}"), &serde_json::json!(null));
    Ok(())
}

/// Show a summary of a canvas file.
pub fn show(app: &mut App, path: &str) -> Result<()> {
    let canvas = app.storage_mut()?.read_canvas(path)?;
    println!("Canvas: {path}");
    println!("  Nodes: {}", canvas.nodes.len());
    println!("  Edges: {}", canvas.edges.len());
    for node in &canvas.nodes {
        let detail = match node.node_type {
            nexus_storage::CanvasNodeType::File => node.file.as_deref().unwrap_or("").to_string(),
            nexus_storage::CanvasNodeType::Text => {
                let t = node.text.as_deref().unwrap_or("");
                if t.len() > 40 { format!("{}...", &t[..40]) } else { t.to_string() }
            }
            nexus_storage::CanvasNodeType::Link => node.url.as_deref().unwrap_or("").to_string(),
            nexus_storage::CanvasNodeType::Group => node.label.as_deref().unwrap_or("").to_string(),
            nexus_storage::CanvasNodeType::Database => node.source.as_deref().unwrap_or("").to_string(),
            nexus_storage::CanvasNodeType::Terminal => node.command.as_deref().unwrap_or("").to_string(),
        };
        println!("  [{:>8}] {} — {}", node.node_type.as_str(), node.id, detail);
    }
    for edge in &canvas.edges {
        let label = edge.label.as_deref().unwrap_or("");
        println!("  {} -> {} ({}) {}", edge.from_node, edge.to_node, edge.edge_type.as_str(), label);
    }
    Ok(())
}

/// Add a node to an existing canvas file.
pub fn add_node(
    app: &mut App,
    path: &str,
    node_type: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    content: Option<&str>,
    label: Option<&str>,
) -> Result<()> {
    let mut canvas = app.storage_mut()?.read_canvas(path)?;
    let id = format!("n{}", canvas.nodes.len() + 1);
    let nt = match node_type {
        "file" => nexus_storage::CanvasNodeType::File,
        "text" => nexus_storage::CanvasNodeType::Text,
        "link" => nexus_storage::CanvasNodeType::Link,
        "group" => nexus_storage::CanvasNodeType::Group,
        "database" => nexus_storage::CanvasNodeType::Database,
        "terminal" => nexus_storage::CanvasNodeType::Terminal,
        _ => anyhow::bail!("Unknown node type: {node_type}. Valid: file, text, link, group, database, terminal"),
    };
    let node = nexus_storage::CanvasNode {
        id: id.clone(),
        node_type: nt.clone(),
        x, y, width, height,
        color: None,
        label: label.map(str::to_string),
        collapsed: false,
        file: if nt == nexus_storage::CanvasNodeType::File { content.map(str::to_string) } else { None },
        text: if nt == nexus_storage::CanvasNodeType::Text { content.map(str::to_string) } else { None },
        url: if nt == nexus_storage::CanvasNodeType::Link { content.map(str::to_string) } else { None },
        source: if nt == nexus_storage::CanvasNodeType::Database { content.map(str::to_string) } else { None },
        command: if nt == nexus_storage::CanvasNodeType::Terminal { content.map(str::to_string) } else { None },
    };
    canvas.nodes.push(node);
    let json = nexus_storage::serialize_canvas(&canvas)?;
    app.storage_mut()?.write_file(path, json.as_bytes())?;
    output::print_success(app.format(), &format!("Added {node_type} node {id} to {path}"), &serde_json::json!(null));
    Ok(())
}

/// Add an edge between two nodes.
pub fn add_edge(
    app: &mut App,
    path: &str,
    from: &str,
    to: &str,
    edge_type: &str,
    label: Option<&str>,
) -> Result<()> {
    let mut canvas = app.storage_mut()?.read_canvas(path)?;
    let id = format!("e{}", canvas.edges.len() + 1);
    let et = nexus_storage::CanvasEdgeType::from_str_lossy(edge_type);
    canvas.edges.push(nexus_storage::CanvasEdge {
        id: id.clone(),
        from_node: from.to_string(),
        to_node: to.to_string(),
        edge_type: et,
        label: label.map(str::to_string),
        color: None,
    });
    let json = nexus_storage::serialize_canvas(&canvas)?;
    app.storage_mut()?.write_file(path, json.as_bytes())?;
    output::print_success(app.format(), &format!("Added edge {id}: {from} -> {to}"), &serde_json::json!(null));
    Ok(())
}
