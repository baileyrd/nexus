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
    nexus_shell_lib::run()
}
