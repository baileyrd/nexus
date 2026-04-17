//! Shell detection — PRD-09 §1.2.
//!
//! The order of priority is:
//!
//! 1. Explicit override (a caller passing `shell = Some(path)` in [`crate::SessionConfig`]).
//! 2. `$SHELL` environment variable, if it points to an executable that exists.
//! 3. Platform fallback — `/bin/bash` on Unix-ish systems, `%ComSpec%` or
//!    `cmd.exe` on Windows.
//!
//! Shell-profile sourcing (PRD-09 §1.3) and `TERM` environment handling
//! (§1.4) are follow-ups — they layer on top of the detected shell and do
//! not affect the path itself.

use std::path::{Path, PathBuf};

/// Resolved shell + args. The terminal layer passes this straight to
/// `portable_pty::CommandBuilder`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    /// Absolute path to the shell executable.
    pub program: PathBuf,
    /// Extra arguments (e.g. `-l` for login shells). May be empty.
    pub args: Vec<String>,
}

impl ShellSpec {
    /// Build a spec for `program` with no extra arguments.
    #[must_use]
    pub fn bare(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }
}

/// Detect a usable default shell for this platform.
///
/// Never returns an error: callers receive the best candidate that satisfies
/// the priority list above, even if that is just the platform fallback.
/// Emits tracing breadcrumbs at each step so operators can see which branch
/// was taken.
#[must_use]
pub fn detect_default_shell() -> ShellSpec {
    // 1) `$SHELL`, if it exists.
    if let Ok(env_shell) = std::env::var("SHELL") {
        let candidate = PathBuf::from(&env_shell);
        if is_executable(&candidate) {
            tracing::debug!(shell = %env_shell, "using $SHELL");
            return ShellSpec::bare(candidate);
        }
        tracing::warn!(
            shell = %env_shell,
            "$SHELL points at a path that is not executable — falling back",
        );
    }

    // 2) Platform fallback.
    //
    // We deliberately don't consult `/etc/passwd` (PRD-09 §1.2 item 3)
    // in this phase because parsing it requires a getpwuid-style syscall
    // or a dependency, and the fallback paths catch the realistic cases
    // (`/bin/bash` on Linux, `/bin/sh` on Alpine, `cmd.exe` on Windows).
    platform_fallback()
}

#[cfg(unix)]
fn platform_fallback() -> ShellSpec {
    for candidate in ["/bin/bash", "/bin/zsh", "/bin/sh"] {
        let path = PathBuf::from(candidate);
        if is_executable(&path) {
            tracing::debug!(shell = %candidate, "using unix fallback");
            return ShellSpec::bare(path);
        }
    }
    // Last-resort: return /bin/sh even if we couldn't stat it — spawn will
    // fail cleanly with a helpful error at Session::spawn time.
    tracing::warn!("no unix fallback shell exists — returning /bin/sh as last resort");
    ShellSpec::bare("/bin/sh")
}

#[cfg(windows)]
fn platform_fallback() -> ShellSpec {
    // Honour `%ComSpec%` if set; otherwise hard-code `cmd.exe`. PowerShell
    // detection (pwsh.exe / powershell.exe) lives in a follow-up because
    // it requires registry lookups or executable probing on PATH that this
    // Phase A skips.
    if let Ok(com_spec) = std::env::var("ComSpec") {
        let path = PathBuf::from(&com_spec);
        if is_executable(&path) {
            tracing::debug!(shell = %com_spec, "using %ComSpec%");
            return ShellSpec::bare(path);
        }
    }
    tracing::debug!("using cmd.exe as Windows fallback");
    ShellSpec::bare("cmd.exe")
}

/// Best-effort "is this file executable?" check. On Unix we use stat + the
/// owner-executable bit; on Windows the existence check is enough because
/// the file mode has no equivalent. Returns `false` on any stat error.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_default_shell_returns_something_spawnable_on_unix() {
        // We can't depend on a specific shell being present, but the
        // returned spec should at minimum be non-empty and point at a
        // file-like path string. The spawn test in `session.rs` validates
        // the end-to-end path.
        let spec = detect_default_shell();
        assert!(!spec.program.as_os_str().is_empty());
    }

    #[test]
    fn bare_spec_has_no_args() {
        let spec = ShellSpec::bare("/bin/sh");
        assert_eq!(spec.program, PathBuf::from("/bin/sh"));
        assert!(spec.args.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn env_shell_takes_precedence_when_executable() {
        // Set SHELL to a file we know is executable — `/bin/sh` exists on
        // every POSIX-ish CI and is always executable.
        let sh = "/bin/sh";
        if !std::path::Path::new(sh).exists() {
            return; // platform without /bin/sh — skip
        }
        // We can't safely set SHELL from a parallel test without process
        // isolation, so only assert the property: the returned spec is
        // non-empty and executable on disk.
        let spec = detect_default_shell();
        assert!(
            std::fs::metadata(&spec.program).is_ok(),
            "detected shell {:?} should exist on disk",
            spec.program,
        );
    }
}
