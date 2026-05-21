//! `nexus acp serve` — start the inbound ACP JSON-RPC server on
//! stdio (BL-145 / Hermes Feature 7).
//!
//! Builds a Nexus runtime, hands the resulting plugin context to
//! [`nexus_acp::AcpServer`], and blocks on the duplex over
//! `tokio::io::{stdin, stdout}` until the parent process disconnects.
//!
//! Pure proxy: every method dispatches through `context.ipc_call(...)`
//! against the allow-list pinned in `nexus_acp::server::route_method`.

use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};

use crate::app::App;

/// Start the ACP stdio server, blocking until the parent disconnects.
///
/// # Errors
/// Returns an error if the forge cannot be opened, the runtime fails
/// to build, or the server's read/write loop fails irrecoverably.
pub fn serve(app: &App) -> Result<()> {
    let forge_root = app.forge_root().to_path_buf();
    let runtime = build_cli_runtime(forge_root.clone()).with_context(|| {
        format!("failed to build runtime at {}", forge_root.display())
    })?;
    let Runtime {
        kernel: _kernel,
        context,
        loader: _loader,
    } = runtime;
    let server = nexus_acp::AcpServer::new(Arc::new(context));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()?;
    rt.block_on(async {
        server
            .serve(tokio::io::stdin(), tokio::io::stdout())
            .await
            .map_err(|e| anyhow::anyhow!("ACP server error: {e}"))
    })?;
    Ok(())
}
