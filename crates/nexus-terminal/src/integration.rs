//! OSC 133 shell-integration emitters (RFC 0003 PR-6).
//!
//! Ships the per-shell scripts (vendored under `assets/shell-integration/`) that
//! make a shell emit OSC 133 semantic-prompt marks — the *primary* command /
//! exit-code signal the server-side VT grid captures (the `precmd.rs` printf
//! sentinel is the fallback for un-instrumented shells). A session opts in via
//! `SessionConfig.shell_integration`; `Session::spawn` then writes
//! [`integration_payload`] into the PTY right after the shell starts.
//!
//! The bundled `nexus-rush` shell does **not** emit OSC 133 yet — it has no
//! precmd/preexec hook — so rush sessions fall back to the sentinel. Teaching
//! rush to emit OSC 133 is an RFC 0002 Stage 2 follow-up.

use std::path::Path;

/// A shell with a bundled OSC 133 integration script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationShell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

impl IntegrationShell {
    /// Detect the integration shell from a program path by basename (stripping a
    /// `.exe` suffix). Returns `None` for shells without an emitter (`sh`, `cmd`,
    /// the bundled `nexus-rush`, …).
    #[must_use]
    pub fn detect(program: &Path) -> Option<Self> {
        let name = program.file_name()?.to_str()?;
        let stem = name.strip_suffix(".exe").unwrap_or(name);
        match stem {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            "pwsh" | "powershell" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// The raw embedded integration script for this shell.
    #[must_use]
    pub fn script(self) -> &'static [u8] {
        match self {
            Self::Bash => include_bytes!("../assets/shell-integration/bash.sh"),
            Self::Zsh => include_bytes!("../assets/shell-integration/zsh.sh"),
            Self::Fish => include_bytes!("../assets/shell-integration/fish.fish"),
            Self::Pwsh => include_bytes!("../assets/shell-integration/pwsh.ps1"),
        }
    }
}

/// Heredoc delimiter for the POSIX `source /dev/stdin` wrapper. Distinctive so
/// it can't collide with script content.
const POSIX_HEREDOC_DELIM: &[u8] = b"__NEXUS_OSC133_EOF__";

/// Wrap a script so a POSIX shell sources it from a quoted heredoc: the script
/// runs in the shell's own context (so `return` / hook setup behave) as a single
/// command, rather than line-by-line interactive input.
fn posix_source_wrapper(script: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(script.len() + 64);
    out.extend_from_slice(b"source /dev/stdin <<'");
    out.extend_from_slice(POSIX_HEREDOC_DELIM);
    out.extend_from_slice(b"'\n");
    out.extend_from_slice(script);
    if !script.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(POSIX_HEREDOC_DELIM);
    out.push(b'\n');
    out
}

/// The bytes to write into a freshly-spawned session's PTY to load the OSC 133
/// integration for `program`'s shell, or `None` for shells without an emitter.
///
/// POSIX shells (bash/zsh) are sourced via a `source /dev/stdin` heredoc; fish
/// and PowerShell receive the script directly (their multi-line blocks parse
/// fine as interactive input and neither misfires a top-level guard on a fresh
/// session).
#[must_use]
pub fn integration_payload(program: &Path) -> Option<Vec<u8>> {
    let shell = IntegrationShell::detect(program)?;
    let script = shell.script();
    Some(match shell {
        IntegrationShell::Bash | IntegrationShell::Zsh => posix_source_wrapper(script),
        IntegrationShell::Fish | IntegrationShell::Pwsh => {
            let mut out = script.to_vec();
            if !out.ends_with(b"\n") {
                out.push(b'\n');
            }
            out
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_known_shells_by_basename() {
        assert_eq!(
            IntegrationShell::detect(&PathBuf::from("/bin/bash")),
            Some(IntegrationShell::Bash)
        );
        assert_eq!(
            IntegrationShell::detect(&PathBuf::from("/usr/bin/zsh")),
            Some(IntegrationShell::Zsh)
        );
        assert_eq!(
            IntegrationShell::detect(&PathBuf::from("C:/Program Files/PowerShell/pwsh.exe")),
            Some(IntegrationShell::Pwsh)
        );
        // No emitter for plain sh or the bundled rush.
        assert_eq!(IntegrationShell::detect(&PathBuf::from("/bin/sh")), None);
        assert_eq!(IntegrationShell::detect(&PathBuf::from("/opt/nexus-rush")), None);
    }

    #[test]
    fn scripts_carry_the_osc_133_finished_mark() {
        for sh in [
            IntegrationShell::Bash,
            IntegrationShell::Zsh,
            IntegrationShell::Fish,
            IntegrationShell::Pwsh,
        ] {
            let s = sh.script();
            assert!(!s.is_empty(), "{sh:?} script is empty");
            assert!(
                s.windows(5).any(|w| w == b"133;D"),
                "{sh:?} script is missing the 133;D finished mark",
            );
        }
    }

    #[test]
    fn posix_payload_sources_from_heredoc() {
        let payload = integration_payload(&PathBuf::from("/bin/bash")).expect("bash payload");
        let text = String::from_utf8_lossy(&payload);
        assert!(text.starts_with("source /dev/stdin <<'__NEXUS_OSC133_EOF__'\n"));
        assert!(text.trim_end().ends_with("__NEXUS_OSC133_EOF__"));
        assert!(text.contains("133;D"));
    }

    #[test]
    fn no_payload_for_shells_without_an_emitter() {
        assert!(integration_payload(&PathBuf::from("/bin/sh")).is_none());
    }
}
