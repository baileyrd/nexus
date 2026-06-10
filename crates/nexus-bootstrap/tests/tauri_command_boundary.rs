//! BL-137 — snapshot guard for the Tauri command surface.
//!
//! Per [`ADR 0011`](../../../../docs/adr/0011-shell-decision-tauri.md) and the
//! CLAUDE.md "shell host stays thin" invariant, the only `#[tauri::command]`s
//! that belong here are those intrinsic to hosting the shell (kernel boot,
//! plugin lifecycle, popout windows, shell-state persistence). Feature work
//! must route through `kernel_invoke` → `ipc_call` and live in a service
//! crate, not as a new bespoke command here.
//!
//! This test pins the current 29-command set. If you add or remove a command
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
//! - `persistence::write_remote_recent` / `persistence::forget_remote_recent`
//!   (BL-148) — saved-recents list for `ssh://` launcher entries. Host
//!   concern because shell-state persistence already lives here; the
//!   remote-recents list extends the same on-disk shell-state file
//!   handled by `write_last_forge_path` / `forget_forge_path`.
//!
//! V5 (`repo-review-2026-06-10.md`): this test lives in
//! `nexus-bootstrap/tests/` rather than `shell/src-tauri/tests/` because
//! `nexus-shell` sits outside the cargo workspace — no CI job compiles it
//! on Linux (it needs webkit2gtk system deps), so a test there never ran.
//! It only reads the shell sources as text (the `dep_invariants.rs`
//! pattern), so hosting it here puts it inside `cargo test --workspace`
//! on every PR.

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
    "notify_desktop",
    "persistence::get_shell_state",
    "persistence::save_shell_state",
    "persistence::write_last_forge_path",
    "persistence::forget_forge_path",
    "persistence::write_remote_recent",
    "persistence::forget_remote_recent",
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

/// Walk up from `CARGO_MANIFEST_DIR` to the workspace root (the directory
/// holding the top-level `Cargo.toml` with `[workspace]`).
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        assert!(
            dir.pop(),
            "could not locate workspace root starting from CARGO_MANIFEST_DIR"
        );
    }
}

/// `shell/src-tauri/src` relative to the workspace root.
fn shell_src_dir() -> PathBuf {
    workspace_root().join("shell").join("src-tauri").join("src")
}

#[test]
fn invoke_handler_matches_pinned_command_set() {
    let lib_path = shell_src_dir().join("lib.rs");
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

#[test]
fn every_declared_tauri_command_is_registered() {
    // V5 — a `#[tauri::command]` fn that never reaches
    // `generate_handler![]` is dead code at best and a forgotten
    // registration at worst; either way the command surface drifted
    // from the pinned snapshot without tripping the test above.
    let mut declared = Vec::new();
    collect_declared_commands(&shell_src_dir(), &mut declared);
    declared.sort();
    declared.dedup();

    let mut registered: Vec<String> = EXPECTED
        .iter()
        .map(|c| {
            c.rsplit("::")
                .next()
                .expect("rsplit yields at least one segment")
                .to_string()
        })
        .collect();
    registered.sort();

    assert_eq!(
        declared, registered,
        "declared `#[tauri::command]` fns and the pinned EXPECTED registration \
         list disagree. Either a command was declared but never registered in \
         `generate_handler![]`, or EXPECTED drifted. Reconcile both, and see \
         the module docs for the thin-bridge policy."
    );
}

/// Recursively collect the names of `#[tauri::command]`-annotated fns
/// under `dir`.
fn collect_declared_commands(dir: &PathBuf, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {}", dir.display(), e)) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_declared_commands(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
            out.extend(declared_commands(&source));
        }
    }
}

/// Scan `source` for `#[tauri::command]` attributes and return the name
/// of each annotated fn. Tolerates further attributes, doc comments, and
/// blank lines between the attribute and the `fn` line.
fn declared_commands(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = false;
    for line in source.lines() {
        let t = line.trim_start();
        if t.starts_with("#[tauri::command") {
            pending = true;
            continue;
        }
        if !pending {
            continue;
        }
        if let Some(idx) = t.find("fn ") {
            let after = &t[idx + 3..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
                pending = false;
            }
        } else if !(t.is_empty() || t.starts_with('#') || t.starts_with('/')) {
            // Anything that isn't another attribute, a comment, or a
            // blank line means the attribute wasn't followed by a fn we
            // can parse — stop waiting rather than mis-attributing.
            pending = false;
        }
    }
    out
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

#[test]
fn declared_commands_parses_attribute_variants() {
    let sample = r#"
#[tauri::command]
pub fn plain() {}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::needless_pass_by_value)]
pub async fn with_args_and_attrs(x: String) {}

/// Mentions #[tauri::command] in a doc comment — not a declaration.
fn not_a_command() {}
"#;
    assert_eq!(
        declared_commands(sample),
        vec!["plain".to_string(), "with_args_and_attrs".to_string()]
    );
}
