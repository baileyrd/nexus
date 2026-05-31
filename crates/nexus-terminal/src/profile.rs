//! Shell-profile sourcing (PRD-09 §1.3).
//!
//! # Scope
//!
//! portable-pty spawns a *non-login, non-interactive* child by default —
//! the shell reads neither `.bash_profile` nor `.bashrc`, so aliases,
//! functions, and user path extensions never materialise. PRD-09 §1.3
//! requires us to explicitly source the right rc file at session init
//! so interactive conveniences work.
//!
//! This module is deliberately small: it returns the *command string* to
//! write to the PTY right after spawn. Actually writing it is the
//! caller's job (the UI or a core plugin) — we don't take a
//! [`crate::Session`] here, which keeps this module trivially unit-
//! testable and keeps [`crate::Session`] platform-agnostic about what
//! the first write happens to be.
//!
//! # Microkernel fit
//!
//! Pure function module. No IPC, no kernel bus, no side effects.
//!
//! # What this is NOT
//!
//! - A replacement for [`crate::env::resolve_env`]. This handles the
//!   *interactive* profile layer (aliases + functions that only exist
//!   at shell level); [`crate::env::resolve_env`] handles the *env
//!   var* layer that a consumer can fully compute outside the shell.
//! - A login-shell spawner. We source the rc file on an already-spawned
//!   non-login shell because portable-pty's `CommandBuilder` doesn't
//!   prefix `-` to argv[0], and shelling out through `-l` changes
//!   working-directory semantics in surprising ways.

use std::path::Path;

use crate::shell::ShellSpec;

/// Which rc file to source for a given shell. Returns `None` for shells
/// we do not know how to configure interactively (e.g. `cmd.exe`,
/// `pwsh` — `PowerShell`'s profile system is different and lives in a
/// follow-up).
#[must_use]
pub fn profile_path_for_shell(shell: &ShellSpec) -> Option<&'static str> {
    // Match on the binary's file_name rather than the full path so
    // custom install locations (/usr/local/bin/bash, /opt/homebrew/bin/zsh)
    // still resolve correctly.
    let name = shell.program.file_name().and_then(|s| s.to_str())?;
    // Strip a `.exe` suffix so Git-for-Windows bash is recognised.
    let base = name.strip_suffix(".exe").unwrap_or(name);
    match base {
        "bash" | "sh" => Some("~/.bashrc"),
        "zsh" => Some("~/.zshrc"),
        "fish" => Some("~/.config/fish/config.fish"),
        _ => None,
    }
}

/// Command to write to the PTY to source the shell's rc file on startup
/// (PRD-09 §1.3). Returns `None` for shells without a supported profile
/// convention.
///
/// The command always ends with a newline so a caller can write it
/// straight to [`crate::Session::write`] without extra ceremony. It
/// silences errors with `2>/dev/null` and guards with `[ -f … ]` so a
/// missing rc file is a no-op, not a visible startup error.
#[must_use]
pub fn profile_source_command(shell: &ShellSpec) -> Option<String> {
    let path = profile_path_for_shell(shell)?;
    let name = shell.program.file_name().and_then(|s| s.to_str())?;
    let base = name.strip_suffix(".exe").unwrap_or(name);
    let cmd = match base {
        "fish" => {
            // Fish has no `[ -f … ]` test; `test -f` is available and
            // the syntax is identical enough.
            format!("test -f {path}; and source {path} 2>/dev/null\n")
        }
        _ => {
            // POSIX shells: guard, then dot-source. Quoted to tolerate
            // paths with spaces (though the rc file paths here don't
            // contain any).
            format!("[ -f {path} ] && . {path} 2>/dev/null\n")
        }
    };
    Some(cmd)
}

/// Whether a shell is one we know how to source a profile for.
#[must_use]
pub fn supports_profile_sourcing(shell: &ShellSpec) -> bool {
    profile_path_for_shell(shell).is_some()
}

/// Minimal convenience for callers that have a bare path rather than a
/// [`ShellSpec`]. Used in a couple of the integration tests in this
/// crate; exposed publicly so external drivers can skip building a full
/// spec when they already know the shell binary.
#[must_use]
pub fn profile_source_command_for_path(path: &Path) -> Option<String> {
    profile_source_command(&ShellSpec::bare(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_and_sh_source_bashrc() {
        assert_eq!(
            profile_path_for_shell(&ShellSpec::bare("/bin/bash")),
            Some("~/.bashrc"),
        );
        assert_eq!(
            profile_path_for_shell(&ShellSpec::bare("/bin/sh")),
            Some("~/.bashrc"),
        );
    }

    #[test]
    fn zsh_sources_zshrc() {
        assert_eq!(
            profile_path_for_shell(&ShellSpec::bare("/bin/zsh")),
            Some("~/.zshrc"),
        );
    }

    #[test]
    fn git_for_windows_bash_is_recognised() {
        // Git-for-Windows ships `bash.exe` — we should source the rc
        // file regardless of the `.exe` suffix.
        assert_eq!(
            profile_path_for_shell(&ShellSpec::bare("C:/Program Files/Git/usr/bin/bash.exe",)),
            Some("~/.bashrc"),
        );
    }

    #[test]
    fn cmd_and_pwsh_have_no_rc_file() {
        assert_eq!(
            profile_path_for_shell(&ShellSpec::bare("C:/Windows/System32/cmd.exe")),
            None,
        );
        assert_eq!(profile_path_for_shell(&ShellSpec::bare("pwsh")), None);
    }

    #[test]
    fn bash_command_uses_guarded_dot_source_and_newline() {
        let cmd = profile_source_command(&ShellSpec::bare("/bin/bash")).expect("bash cmd");
        assert!(cmd.ends_with('\n'), "command should end with newline");
        assert!(cmd.contains("[ -f ~/.bashrc ]"));
        assert!(cmd.contains(". ~/.bashrc"));
        assert!(cmd.contains("2>/dev/null"));
    }

    #[test]
    fn fish_command_uses_fish_specific_guard() {
        let cmd = profile_source_command(&ShellSpec::bare("/usr/bin/fish")).expect("fish cmd");
        assert!(cmd.contains("test -f"));
        assert!(cmd.contains("and source"));
    }

    #[test]
    fn unsupported_shell_returns_none() {
        assert!(profile_source_command(&ShellSpec::bare("cmd.exe")).is_none());
        assert!(!supports_profile_sourcing(&ShellSpec::bare("cmd.exe")));
    }

    #[test]
    fn path_convenience_matches_spec_output() {
        let from_path = profile_source_command_for_path(Path::new("/bin/zsh"));
        let from_spec = profile_source_command(&ShellSpec::bare("/bin/zsh"));
        assert_eq!(from_path, from_spec);
    }
}
