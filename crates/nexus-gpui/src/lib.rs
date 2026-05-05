//! Nexus gpui desktop shell — Phase 0 spike.
//!
//! Validates three core assumptions before production work begins:
//!
//! 1. gpui compiles and opens a window on the target platform.
//! 2. [`nexus_bootstrap::build_gpui_runtime`] boots the kernel from within
//!    the gpui event loop.
//! 3. Async `ipc_call`s work via a [`tokio::runtime::Runtime`] bridge from
//!    gpui's background executor.

mod ai;
mod editor;
mod graph;
mod kernel_bridge;
mod pane;
mod theme;
mod workbench;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use gpui::{App, AppContext, Application, TitlebarOptions, Window, WindowOptions};
use gpui_platform;

use nexus_bootstrap::build_gpui_runtime;

pub use kernel_bridge::KernelBridge;

/// Entry point called from `main`. Boots the Nexus kernel, then hands control
/// to the gpui event loop for the lifetime of the process.
///
/// # Errors
/// Returns an error if the forge cannot be opened or kernel bootstrap fails.
/// Errors inside the event loop (IPC failures, render panics) are logged and
/// recovered where possible.
pub fn run_app(forge_path: PathBuf) -> Result<()> {
    // Boot the kernel before gpui starts — `build_gpui_runtime` is sync and
    // must not run inside gpui's executor.
    let runtime = build_gpui_runtime(forge_path).context("failed to boot Nexus kernel")?;

    // Shared runtime + dedicated tokio runtime for async IPC calls.
    let bridge = Arc::new(KernelBridge::new(runtime)?);

    Application::with_platform(gpui_platform::current_platform(false)).run(move |cx: &mut App| {
        let bridge = Arc::clone(&bridge);
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Nexus".into()),
                    appears_transparent: false,
                    ..TitlebarOptions::default()
                }),
                ..WindowOptions::default()
            },
            |_window: &mut Window, cx: &mut App| {
                cx.new(|cx| workbench::WorkbenchView::new(bridge, cx))
            },
        )
        .expect("failed to open Nexus window");
    });

    Ok(())
}
