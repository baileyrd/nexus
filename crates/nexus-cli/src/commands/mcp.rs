//! `nexus mcp` — start the MCP server on stdio transport.

use anyhow::Result;

use crate::app::App;

/// Start the MCP stdio server, blocking until the client disconnects.
///
/// Opens a fresh [`nexus_storage::StorageEngine`] for the MCP server (it takes
/// ownership via [`nexus_mcp::NexusMcpServer::new`]) and runs the async
/// transport loop on a new Tokio runtime.
///
/// # Errors
///
/// Returns an error if the forge cannot be opened or the server fails to start.
pub fn serve(app: &App) -> Result<()> {
    let forge_root = app.forge_root().to_path_buf();

    let storage = nexus_storage::StorageEngine::open(
        &forge_root,
        &nexus_storage::StorageConfig::default(),
    )
    .map_err(|e| anyhow::anyhow!("failed to open forge at '{}': {e}", forge_root.display()))?;

    let server = nexus_mcp::NexusMcpServer::new(storage);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server.serve_stdio())
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}
