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

    // C76 — build the tokio runtime *before* the CLI runtime and enter
    // it first. `build_cli_runtime` synchronously wires every core
    // plugin, including the workflow plugin's cron / file_event /
    // git_event / mcp_event / digest trigger engines, each of which
    // checks `tokio::runtime::Handle::try_current()` and silently
    // disables itself if no runtime is entered
    // (crates/nexus-workflow/src/core_plugin.rs). `serve` blocks until
    // the parent disconnects, so it's long-lived exactly like the TUI —
    // safe and correct for it to arm triggers, unlike a CLI one-shot
    // that would exit before any trigger could matter.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()?;
    let guard = rt.enter();
    let runtime = build_cli_runtime(forge_root.clone())
        .with_context(|| format!("failed to build runtime at {}", forge_root.display()))?;
    drop(guard);
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = runtime;

    let event_bus = kernel.event_bus();
    let server = nexus_remote::RemoteServer::new(Arc::new(context), event_bus);

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
