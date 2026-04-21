// src-tauri/src/main.rs
// Tauri application entry point.
// The shell is almost entirely frontend — this file is minimal.
// Add Tauri commands here as the plugin system requires deeper OS integration.

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nexus_shell_lib::run()
}
