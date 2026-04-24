// src-tauri/src/main.rs
// Tauri application entry point.
// The shell is almost entirely frontend — this file is minimal.
// Add Tauri commands here as the plugin system requires deeper OS integration.

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Install the local panic hook first so a crash during Tauri init
    // still lands in ~/.nexus-shell/logs/panic.log. No network, no opt-in —
    // see docs/planning/PHASE-5-IMPLEMENTATION-PLAN.md §4 (WI-47).
    nexus_panic_log::install("nexus-shell");

    // Initialise tracing subscriber so kernel + shell log records surface
    // on stderr. `RUST_LOG` still overrides via env-filter; default is INFO.
    // Mirrors the pattern used by `nexus-cli` and `nexus-tui`.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(""))
        .add_directive(tracing::Level::INFO.into());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    nexus_shell_lib::run()
}
