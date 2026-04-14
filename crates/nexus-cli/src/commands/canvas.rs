//! CLI commands for canvas file operations.

use anyhow::Result;
use nexus_bootstrap::storage as ipc;
use nexus_bootstrap::{
    parse_canvas, serialize_canvas, CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode,
    CanvasNodeType,
};

use crate::app::App;
use crate::output;

/// Read a canvas file through storage IPC and parse it into a [`CanvasFile`].
fn load_canvas(app: &mut App, path: &str) -> Result<CanvasFile> {
    let (runtime, rt) = app.runtime()?;
    let bytes = ipc::read_file(runtime, rt, path)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| anyhow::anyhow!("canvas '{path}' is not valid UTF-8: {e}"))?;
    Ok(parse_canvas(text)?)
}

/// Serialize `canvas` and write it back to `path` through storage IPC.
fn save_canvas(app: &mut App, path: &str, canvas: &CanvasFile) -> Result<()> {
    let json = serialize_canvas(canvas)?;
    let (runtime, rt) = app.runtime()?;
    ipc::write_file(runtime, rt, path, json.as_bytes())?;
    Ok(())
}

/// Create a new empty canvas file.
pub fn create(app: &mut App, path: &str) -> Result<()> {
    let canvas = CanvasFile {
        nodes: vec![],
        edges: vec![],
    };
    save_canvas(app, path, &canvas)?;
    output::print_success(
        app.format(),
        &format!("Created canvas: {path}"),
        &serde_json::json!(null),
    );
    Ok(())
}

/// Show a summary of a canvas file.
pub fn show(app: &mut App, path: &str) -> Result<()> {
    let canvas = load_canvas(app, path)?;
    println!("Canvas: {path}");
    println!("  Nodes: {}", canvas.nodes.len());
    println!("  Edges: {}", canvas.edges.len());
    for node in &canvas.nodes {
        let detail = match node.node_type {
            CanvasNodeType::File => node.file.as_deref().unwrap_or("").to_string(),
            CanvasNodeType::Text => {
                let t = node.text.as_deref().unwrap_or("");
                if t.len() > 40 {
                    format!("{}...", &t[..40])
                } else {
                    t.to_string()
                }
            }
            CanvasNodeType::Link => node.url.as_deref().unwrap_or("").to_string(),
            CanvasNodeType::Group => node.label.as_deref().unwrap_or("").to_string(),
            CanvasNodeType::Database => node.source.as_deref().unwrap_or("").to_string(),
            CanvasNodeType::Terminal => node.command.as_deref().unwrap_or("").to_string(),
        };
        println!("  [{:>8}] {} — {}", node.node_type.as_str(), node.id, detail);
    }
    for edge in &canvas.edges {
        let label = edge.label.as_deref().unwrap_or("");
        println!(
            "  {} -> {} ({}) {}",
            edge.from_node,
            edge.to_node,
            edge.edge_type.as_str(),
            label
        );
    }
    Ok(())
}

/// Add a node to an existing canvas file.
#[allow(clippy::too_many_arguments)]
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
    let mut canvas = load_canvas(app, path)?;
    let id = format!("n{}", canvas.nodes.len() + 1);
    let nt = match node_type {
        "file" => CanvasNodeType::File,
        "text" => CanvasNodeType::Text,
        "link" => CanvasNodeType::Link,
        "group" => CanvasNodeType::Group,
        "database" => CanvasNodeType::Database,
        "terminal" => CanvasNodeType::Terminal,
        _ => anyhow::bail!(
            "Unknown node type: {node_type}. Valid: file, text, link, group, database, terminal"
        ),
    };
    let node = CanvasNode {
        id: id.clone(),
        node_type: nt.clone(),
        x,
        y,
        width,
        height,
        color: None,
        label: label.map(str::to_string),
        collapsed: false,
        file: if nt == CanvasNodeType::File {
            content.map(str::to_string)
        } else {
            None
        },
        text: if nt == CanvasNodeType::Text {
            content.map(str::to_string)
        } else {
            None
        },
        url: if nt == CanvasNodeType::Link {
            content.map(str::to_string)
        } else {
            None
        },
        source: if nt == CanvasNodeType::Database {
            content.map(str::to_string)
        } else {
            None
        },
        command: if nt == CanvasNodeType::Terminal {
            content.map(str::to_string)
        } else {
            None
        },
    };
    canvas.nodes.push(node);
    save_canvas(app, path, &canvas)?;
    output::print_success(
        app.format(),
        &format!("Added {node_type} node {id} to {path}"),
        &serde_json::json!(null),
    );
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
    let mut canvas = load_canvas(app, path)?;
    let id = format!("e{}", canvas.edges.len() + 1);
    let et = CanvasEdgeType::from_str_lossy(edge_type);
    canvas.edges.push(CanvasEdge {
        id: id.clone(),
        from_node: from.to_string(),
        to_node: to.to_string(),
        edge_type: et,
        label: label.map(str::to_string),
        color: None,
    });
    save_canvas(app, path, &canvas)?;
    output::print_success(
        app.format(),
        &format!("Added edge {id}: {from} -> {to}"),
        &serde_json::json!(null),
    );
    Ok(())
}
