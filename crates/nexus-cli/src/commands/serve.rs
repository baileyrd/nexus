//! `nexus serve --stdio` — start the remote-forge JSON-RPC server on
//! stdio (BL-140 Phase 1).
//!
//! Builds a full Nexus runtime (same path as `nexus tui` / `nexus acp
//! serve`), then hands the kernel + plugin context to
//! [`nexus_remote::RemoteServer`] and blocks on a duplex over
//! `tokio::io::{stdin, stdout}` until the parent disconnects.
//!
//! Pure proxy — every `ipc_call` request dispatches through the
//! kernel's `ipc_call` boundary; every `event_subscribe` request opens
//! a kernel bus subscription that forwards matching events as
//! server-pushed JSON-RPC notifications.

use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};

use crate::app::App;

/// Start the remote-forge stdio server, blocking until the parent
/// disconnects.
///
/// # Errors
/// Returns an error if the forge cannot be opened, the runtime fails to
/// build, or the server's read/write loop fails irrecoverably.
pub fn serve(app: &App) -> Result<()> {
    let forge_root = app.forge_root().to_path_buf();
    let runtime = build_cli_runtime(forge_root.clone()).with_context(|| {
        format!("failed to build runtime at {}", forge_root.display())
    })?;
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = runtime;

    let event_bus = kernel.event_bus();
    let server = nexus_remote::RemoteServer::new(Arc::new(context), event_bus);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()?;
    rt.block_on(async {
        server
            .serve(tokio::io::stdin(), tokio::io::stdout())
            .await
            .map_err(|e| anyhow::anyhow!("remote server error: {e}"))
    })?;

    // Hold the kernel alive for the duration of the server loop. Drop
    // happens here at scope exit so the plugins shut down cleanly.
    drop(kernel);
    Ok(())
}
