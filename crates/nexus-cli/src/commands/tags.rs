//! Tag listing — MCP-parity subcommand for `nexus_list_tags`.
//!
//! Wraps the kernel IPC `storage::query_tags` so the CLI exposes the same
//! tag-introspection surface as the MCP server.

use anyhow::Result;
use nexus_bootstrap::storage as ipc;

use crate::app::App;
use crate::output::print_list;

/// List every occurrence of a tag across the forge.
///
/// When `name` is `None` (or empty), all tags are returned; otherwise only
/// occurrences of the named tag are listed. Mirrors the `nexus_list_tags`
/// MCP tool (kernel IPC `storage::query_tags`).
pub fn list(app: &mut App, name: Option<&str>) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;

    let query_name = name.unwrap_or("");
    let tags = ipc::query_tags(runtime, rt, query_name)
        .map_err(|e| anyhow::anyhow!("failed to query tags: {e}"))?;

    if tags.is_empty() {
        println!("No tags found.");
        return Ok(());
    }

    let headers = &["Tag", "File", "Source"];
    let rows: Vec<Vec<String>> = tags
        .iter()
        .map(|t| vec![t.name.clone(), t.file_path.clone(), t.source.clone()])
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}
