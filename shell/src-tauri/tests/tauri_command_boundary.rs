//! BL-137 — snapshot guard for the Tauri command surface.
//!
//! Per [`ADR 0011`](../../../../docs/adr/0011-shell-decision-tauri.md) and the
//! CLAUDE.md "shell host stays thin" invariant, the only `#[tauri::command]`s
//! that belong here are those intrinsic to hosting the shell (kernel boot,
//! plugin lifecycle, popout windows, shell-state persistence). Feature work
//! must route through `kernel_invoke` → `ipc_call` and live in a service
//! crate, not as a new bespoke command here.
//!
//! This test pins the current 26-command set. If you add or remove a command
//! and this test fails, that's the system asking: **is this a host concern,
//! or did this belong behind an IPC handler?** If it really is a host
//! concern, update both the [`EXPECTED`] list below and add an ADR (or
//! ADR addendum) explaining why.
//!
//! Recent additions:
//! - `bridge::boot_remote` (BL-140 Phase 3a) — top-level boot command for
//!   the `ssh://` remote-forge transport. Host concern under the same
//!   rationale as `boot_kernel`: the choice of *which* runtime to build
//!   has to happen before any plugin IPC, so it can't itself live behind
//!   `kernel_invoke`. See `docs/developer/remote-forge.md`.
//! - `bridge::kernel_connection_state` (BL-140 Phase 3c) — sync read of
//!   the current `ConnectionState`. Host concern because the state lives
//!   in the kernel-runtime managed-state slot; surfacing it via an IPC
//!   verb would require a kernel-side plugin that doesn't exist (and
//!   doesn't make sense — state is shell-side, observed from the
//!   reconnecting wrapper).

use std::fs;
use std::path::PathBuf;

/// The pinned set of `#[tauri::command]` handlers registered by
/// `invoke_handler!` in `shell/src-tauri/src/lib.rs`.
///
/// Grouped by intent (mirrors the CLAUDE.md grouping):
///   - **kernel**: init/boot/shutdown/invoke/subscribe/unsubscribe/is_booted
///   - **plugin-management**: scan, enable, granted-caps read/write, revoke
///   - **persistence**: shell-state + last-forge-path
///   - **utility**: path_exists, append_shell_log
///   - **popout windows**: ADR 0020
const EXPECTED: &[&str] = &[
    "scan_plugin_directory",
    "scan_plugin_directory_at",
    "set_plugin_enabled",
    "get_plugin_granted_capabilities",
    "set_plugin_granted_capabilities",
    "path_exists",
    "append_shell_log",
    "persistence::get_shell_state",
    "persistence::save_shell_state",
    "persistence::write_last_forge_path",
    "persistence::forget_forge_path",
    "bridge::init_forge",
    "bridge::boot_kernel",
    "bridge::boot_remote",
    "bridge::shutdown_kernel",
    "bridge::revoke_plugin_capability",
    "bridge::kernel_invoke",
    "bridge::kernel_subscribe",
    "bridge::kernel_unsubscribe",
    "bridge::kernel_is_booted",
    "bridge::kernel_connection_state",
    "windows::popout_window",
    "windows::close_popout_window",
    "windows::list_popout_windows",
    "windows::get_popout_window_bounds",
    "windows::set_popout_window_bounds",
];

#[test]
fn invoke_handler_matches_pinned_command_set() {
    let lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let source = fs::read_to_string(&lib_path)
        .unwrap_or_else(|e| panic!("read {}: {}", lib_path.display(), e));

    let handlers = extract_invoke_handlers(&source).unwrap_or_else(|| {
        panic!(
            "could not locate `.invoke_handler(tauri::generate_handler![ … ])` block in {}",
            lib_path.display()
        )
    });

    let actual: Vec<String> = handlers
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();

    let expected: Vec<String> = EXPECTED.iter().map(ToString::to_string).collect();

    if actual != expected {
        let added: Vec<&String> = actual.iter().filter(|c| !expected.contains(c)).collect();
        let removed: Vec<&String> = expected.iter().filter(|c| !actual.contains(c)).collect();

        panic!(
            "Tauri command surface drifted from the pinned snapshot.\n\
             \n\
             Added: {added:?}\n\
             Removed: {removed:?}\n\
             \n\
             Per CLAUDE.md ('shell host stays thin') and ADR 0011, the shell must \
             not grow new feature-specific `#[tauri::command]`s — route those \
             through `kernel_invoke` -> `ipc_call` instead. If a new command is \
             genuinely a host concern (popout, shell persistence, plugin \
             lifecycle), update the EXPECTED list in this test AND add or extend \
             an ADR (see ADR 0020 for the popout pattern) describing why.\n\
             \n\
             Expected (pinned): {expected:#?}\n\
             Actual   (lib.rs): {actual:#?}\n"
        );
    }
}

/// Extract the comma-separated body of the first
/// `.invoke_handler(tauri::generate_handler![ … ])` call in `source`.
///
/// Returns `None` if the pattern isn't present.
fn extract_invoke_handlers(source: &str) -> Option<&str> {
    let start_marker = ".invoke_handler(tauri::generate_handler![";
    let start = source.find(start_marker)? + start_marker.len();
    let rest = &source[start..];
    let end = rest.find("])")?;
    Some(&rest[..end])
}

#[test]
fn extractor_handles_multiline_block() {
    let sample = r#"
        .on_window_event(...)
        .invoke_handler(tauri::generate_handler![
            foo,
            bar::baz,
            quux,
        ])
        .run(...)
    "#;
    let body = extract_invoke_handlers(sample).expect("block present");
    let names: Vec<&str> = body
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(names, vec!["foo", "bar::baz", "quux"]);
}

#[test]
fn extractor_returns_none_when_block_absent() {
    assert!(extract_invoke_handlers("fn main() {}").is_none());
}
