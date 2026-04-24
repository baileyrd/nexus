//! `nexus desktop` — launch the Tauri-based shell as a subprocess.
//!
//! The shell (`nexus-shell`) is not built as part of the Cargo workspace
//! (the workspace has `exclude = ["shell"]`). It's shipped alongside the
//! `nexus` binary in the release package. This module resolves the shell
//! binary at runtime and spawns it, forwarding any extra arguments and
//! propagating the exit code.
//!
//! Resolution order (§7 default iii of docs/PHASE-4-IMPLEMENTATION-PLAN.md):
//!
//! 1. `$NEXUS_SHELL_BIN` env var, if set.
//! 2. Sibling of the current executable
//!    (e.g. `<prefix>/bin/nexus-shell` when `nexus` lives at
//!    `<prefix>/bin/nexus`).
//! 3. `PATH` lookup.
//!
//! On resolution failure, the error message mirrors the plan's §4.1 Risks
//! row so users get an actionable hint.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

/// Binary name we're looking for. Windows adds `.exe` automatically via the
/// sibling-directory path join; PATH lookup also handles the extension.
const SHELL_BIN_NAME: &str = "nexus-shell";

/// Launch `nexus-shell`, forwarding `passthrough_args` and returning the
/// child's exit code (0 on success; the caller should propagate it via
/// `std::process::exit`).
pub fn launch(passthrough_args: &[String]) -> Result<i32> {
    let bin = resolve_shell_binary()?;
    tracing::info!(binary = %bin.display(), "launching nexus-shell");

    let status = Command::new(&bin)
        .args(passthrough_args)
        .status()
        .with_context(|| format!("failed to spawn {}", bin.display()))?;

    // On Unix, `code()` returns None if the process was killed by a signal.
    // Fall back to 128 + signal by convention, but for simplicity use 1.
    Ok(status.code().unwrap_or(1))
}

/// Resolve the shell binary per the documented lookup order. Returns an
/// actionable error when nothing is found.
fn resolve_shell_binary() -> Result<PathBuf> {
    // 1. Explicit env var override. Empty-string values are treated the
    //    same as unset (POSIX convention; avoids surprising failures when
    //    callers export the var with no value).
    if let Ok(explicit) = std::env::var("NEXUS_SHELL_BIN") {
        if !explicit.is_empty() {
            let path = PathBuf::from(&explicit);
            if path.exists() {
                return Ok(path);
            }
            return Err(anyhow!(
                "NEXUS_SHELL_BIN points to {explicit:?}, but no file exists there"
            ));
        }
    }

    // 2. Sibling of the current exe.
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let candidate = parent.join(shell_bin_filename());
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // 3. PATH lookup (manual — avoid adding a new crate dep just for this).
    if let Some(path) = search_path(shell_bin_filename()) {
        return Ok(path);
    }

    Err(anyhow!(
        "Could not find `{SHELL_BIN_NAME}` binary. \
         Set NEXUS_SHELL_BIN env var or install the shell bundle."
    ))
}

/// Platform-specific binary filename (appends `.exe` on Windows).
fn shell_bin_filename() -> &'static str {
    if cfg!(windows) {
        // Leaky static string — fine; process-lifetime.
        concat!("nexus-shell", ".exe")
    } else {
        SHELL_BIN_NAME
    }
}

/// Walk `$PATH` looking for `name`. Returns the first match.
fn search_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_p: &Path) -> bool {
    // Windows: treat any file on PATH as executable for lookup purposes.
    // `Command::status` will return a real error if it isn't.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_from_env_var() {
        // `/bin/true` exists on every Unix CI image we care about.
        #[cfg(unix)]
        {
            // SAFETY: set_var is safe in single-threaded test context.
            unsafe { std::env::set_var("NEXUS_SHELL_BIN", "/bin/true"); }
            let path = resolve_shell_binary().expect("env-var resolution");
            assert_eq!(path, PathBuf::from("/bin/true"));
            unsafe { std::env::remove_var("NEXUS_SHELL_BIN"); }
        }
    }

    #[test]
    fn env_var_nonexistent_errors() {
        #[cfg(unix)]
        {
            unsafe { std::env::set_var("NEXUS_SHELL_BIN", "/definitely/not/here/nexus-shell"); }
            let err = resolve_shell_binary().expect_err("should fail");
            let msg = format!("{err}");
            assert!(msg.contains("NEXUS_SHELL_BIN"), "got: {msg}");
            unsafe { std::env::remove_var("NEXUS_SHELL_BIN"); }
        }
    }
}
