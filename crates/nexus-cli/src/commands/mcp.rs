//! `nexus mcp` — start the MCP server on stdio transport.

use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};

use crate::app::App;

/// Start the MCP stdio server, blocking until the client disconnects.
///
/// Builds a Nexus runtime and hands the resulting plugin context to
/// [`nexus_mcp::NexusMcpServer`], which dispatches every tool call through
/// `ipc_call` rather than holding a storage engine directly.
///
/// # Errors
///
/// Returns an error if the forge cannot be opened or the server fails to start.
pub fn serve(app: &App) -> Result<()> {
    let forge_root = app.forge_root().to_path_buf();
    let runtime = build_cli_runtime(forge_root.clone())
        .with_context(|| format!("failed to build runtime at {}", forge_root.display()))?;

    // Destructure Runtime so we can move `context` into an Arc while keeping
    // the kernel and loader alive for the server's lifetime.
    let Runtime { kernel: _kernel, context, loader: _loader } = runtime;
    let context = Arc::new(context);

    let server = nexus_mcp::NexusMcpServer::new(Arc::clone(&context));

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server.serve_stdio())
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}
