//! Legacy-shell freeze guardrail.
//!
//! Caps the count of `#[tauri::command]` handlers in `crates/nexus-app/src/`
//! at the Phase-0 baseline. New Tauri commands are the wrong answer per
//! `CONTRIBUTING.md` and `docs/adr/0011-adopt-plugin-first-shell.md` — new
//! capability work belongs as a service-crate IPC handler plus a plugin in
//! `shell/src/plugins/nexus/`.
//!
//! If you are legitimately *removing* a handler (post-parity work, bug fix),
//! lower `BASELINE_COMMANDS` and note the reason in your commit message. The
//! baseline only goes down, never up.

use std::path::{Path, PathBuf};

/// Count of `#[tauri::command]` attributes in `crates/nexus-app/src/` on the
/// day the freeze landed. The test asserts the current count is `<=` this.
const BASELINE_COMMANDS: usize = 95;

/// Literal attribute substring. A single-token pattern is robust against
/// whitespace, line breaks between handler body and attribute, intervening
/// comments, etc. — every real occurrence contains this exact string.
const COMMAND_ATTR: &str = "#[tauri::command]";

#[test]
fn legacy_shell_tauri_command_count_does_not_grow() {
    let src_root = workspace_root().join("crates").join("nexus-app").join("src");
    let count = count_attr_in_tree(&src_root, COMMAND_ATTR);

    assert!(
        count <= BASELINE_COMMANDS,
        "legacy shell freeze violated: found {count} `{COMMAND_ATTR}` in \
         crates/nexus-app/src (baseline is {BASELINE_COMMANDS}).\n\
         \n\
         New Tauri commands in the legacy shell are forbidden — see\n\
         CONTRIBUTING.md and docs/adr/0011-adopt-plugin-first-shell.md.\n\
         New capability work belongs as a service-crate IPC handler plus a\n\
         plugin in shell/src/plugins/nexus/.\n\
         \n\
         If you are legitimately removing a handler, lower BASELINE_COMMANDS\n\
         in crates/nexus-bootstrap/tests/legacy_freeze.rs and note the reason\n\
         in your commit message."
    );
}

fn count_attr_in_tree(dir: &Path, needle: &str) -> usize {
    let mut total = 0;
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("failed to read dir entry");
        let path = entry.path();
        let ft = entry.file_type().expect("failed to stat entry");
        if ft.is_dir() {
            total += count_attr_in_tree(&path, needle);
        } else if ft.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            total += text.matches(needle).count();
        }
    }
    total
}

/// Walk up from `CARGO_MANIFEST_DIR` to the workspace root (the directory
/// holding the top-level `Cargo.toml` with `[workspace]`).
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            panic!("could not locate workspace root starting from CARGO_MANIFEST_DIR");
        }
    }
}
