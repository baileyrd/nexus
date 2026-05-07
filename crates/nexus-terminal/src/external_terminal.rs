//! BL-059: "Open in external terminal" escape hatch.
//!
//! Some interactive programs (vim, htop, less, REPLs that use full
//! terminal control) work poorly inside the in-app PTY layered under
//! xterm.js. This module hands a saved-command's working directory off
//! to the user's preferred external terminal emulator, opening a fresh
//! interactive shell rooted at that path.
//!
//! Intentionally narrow scope:
//!
//! - Open the user's interactive shell at `working_dir`. Don't auto-
//!   execute the saved command's `shell_cmd`. The user gets a normal
//!   shell prompt; if they want to run the command they paste / re-
//!   type it, same as any other terminal hand-off.
//! - Don't pass per-command env vars. Each emulator's argv shape for
//!   "set env on launch" varies (some don't support it at all); the
//!   shell that opens reads the user's normal login profile, which is
//!   what they want for an escape hatch. Tracked as a follow-up if
//!   anyone asks.
//!
//! The detection table is exposed (`SUPPORTED_TERMINALS`) so callers
//! and tests can introspect priority order and per-platform argv.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Family tag for a terminal emulator. Carried on the success result
/// so the IPC caller can surface "opened in <kind>" to the UI without
/// the shell having to inspect the launched argv.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    /// `kitty` (Linux, macOS).
    Kitty,
    /// `alacritty` (Linux, macOS, Windows).
    Alacritty,
    /// `wezterm` (Linux, macOS, Windows).
    Wezterm,
    /// `ghostty` (Linux, macOS).
    Ghostty,
    /// GNOME Terminal (`gnome-terminal`).
    GnomeTerminal,
    /// KDE Konsole (`konsole`).
    Konsole,
    /// XFCE Terminal (`xfce4-terminal`).
    Xfce4Terminal,
    /// Plain `xterm` — the universal Linux fallback.
    Xterm,
    /// Debian alternatives' `x-terminal-emulator` symlink.
    XTerminalEmulator,
    /// Windows Terminal (`wt.exe`).
    WindowsTerminal,
    /// macOS Terminal.app, opened via `open -a Terminal`.
    MacTerminal,
    /// macOS iTerm2.app, opened via `open -a iTerm`.
    Iterm2,
}

/// Concrete process-spawn shape for a `TerminalKind` against a
/// working directory.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Program to run (resolved against PATH at spawn time).
    pub program: String,
    /// Argument vector.
    pub args: Vec<String>,
}

/// Default priority order. Earlier entries win when multiple
/// candidates are installed. Populated cross-platform; per-platform
/// availability filtering happens at detect-time.
pub const DEFAULT_PRIORITY: &[TerminalKind] = &[
    // Power-user emulators first — the kind a user with one of these
    // installed almost certainly wants to land in.
    TerminalKind::Iterm2,
    TerminalKind::Wezterm,
    TerminalKind::Ghostty,
    TerminalKind::Kitty,
    TerminalKind::Alacritty,
    TerminalKind::WindowsTerminal,
    // Desktop-environment defaults next.
    TerminalKind::GnomeTerminal,
    TerminalKind::Konsole,
    TerminalKind::Xfce4Terminal,
    TerminalKind::MacTerminal,
    // Last-resort fallbacks.
    TerminalKind::XTerminalEmulator,
    TerminalKind::Xterm,
];

/// Build the `LaunchSpec` for `kind` rooted at `working_dir`. Returns
/// `None` for kinds that don't have an argv shape on the current
/// build target — e.g. `Iterm2` on Linux.
#[must_use]
pub fn launch_spec(kind: TerminalKind, working_dir: &Path) -> Option<LaunchSpec> {
    let dir = working_dir.display().to_string();
    match kind {
        TerminalKind::Kitty => Some(LaunchSpec {
            program: "kitty".into(),
            args: vec!["--directory".into(), dir],
        }),
        TerminalKind::Alacritty => Some(LaunchSpec {
            program: "alacritty".into(),
            args: vec!["--working-directory".into(), dir],
        }),
        TerminalKind::Wezterm => Some(LaunchSpec {
            program: "wezterm".into(),
            args: vec!["start".into(), "--cwd".into(), dir],
        }),
        TerminalKind::Ghostty => Some(LaunchSpec {
            program: "ghostty".into(),
            // Ghostty's CLI uses `--working-directory=<path>` (single
            // arg, no separate value token).
            args: vec![format!("--working-directory={dir}")],
        }),
        TerminalKind::GnomeTerminal => Some(LaunchSpec {
            program: "gnome-terminal".into(),
            args: vec![format!("--working-directory={dir}")],
        }),
        TerminalKind::Konsole => Some(LaunchSpec {
            program: "konsole".into(),
            args: vec!["--workdir".into(), dir],
        }),
        TerminalKind::Xfce4Terminal => Some(LaunchSpec {
            program: "xfce4-terminal".into(),
            args: vec![format!("--working-directory={dir}")],
        }),
        TerminalKind::Xterm => {
            // xterm has no `--cwd` flag. Spawn a login shell rooted
            // at the directory via `bash -c 'cd DIR; exec $SHELL'`.
            // Single-quoting around `dir` is unsafe if the path
            // contains a `'`; we fall back to `bash -c` with an
            // explicit cd to keep things simple. PATHs with embedded
            // single-quotes are vanishingly rare on user-controlled
            // forge roots.
            Some(LaunchSpec {
                program: "xterm".into(),
                args: vec![
                    "-e".into(),
                    "bash".into(),
                    "-c".into(),
                    format!("cd {} && exec $SHELL", shell_quote(&dir)),
                ],
            })
        }
        TerminalKind::XTerminalEmulator => Some(LaunchSpec {
            // Debian's `x-terminal-emulator` is a `update-alternatives`
            // symlink. Most concrete pointers honour `-e` for the
            // command + a working-dir prelude in the same shape as
            // xterm's fallback.
            program: "x-terminal-emulator".into(),
            args: vec![
                "-e".into(),
                "bash".into(),
                "-c".into(),
                format!("cd {} && exec $SHELL", shell_quote(&dir)),
            ],
        }),
        TerminalKind::WindowsTerminal => {
            if !cfg!(windows) {
                return None;
            }
            Some(LaunchSpec {
                program: "wt.exe".into(),
                args: vec!["-d".into(), dir],
            })
        }
        TerminalKind::MacTerminal => {
            if !cfg!(target_os = "macos") {
                return None;
            }
            Some(LaunchSpec {
                program: "open".into(),
                args: vec!["-a".into(), "Terminal".into(), dir],
            })
        }
        TerminalKind::Iterm2 => {
            if !cfg!(target_os = "macos") {
                return None;
            }
            Some(LaunchSpec {
                program: "open".into(),
                args: vec!["-a".into(), "iTerm".into(), dir],
            })
        }
    }
}

/// Best-effort POSIX-shell single-quoting for a path. Wraps in `'…'`
/// and escapes any embedded single quotes. Used for the `xterm` /
/// `x-terminal-emulator` fallbacks where we hand-roll a `bash -c`
/// preamble. Modern emulators accept `--working-directory` directly
/// and don't need this.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str(r"'\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Walk `$PATH` looking for an executable named `program`. On
/// Windows, also checks `<program>.exe`. Returns the first match.
#[must_use]
pub fn which_in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let already_exe = Path::new(program)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    let candidates: Vec<String> = if cfg!(windows) && !already_exe {
        vec![program.to_string(), format!("{program}.exe")]
    } else {
        vec![program.to_string()]
    };
    for dir in std::env::split_paths(&path) {
        for name in &candidates {
            let full = dir.join(name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// Pick the first `TerminalKind` from `priority` whose `LaunchSpec`
/// is supported on the current platform AND whose program is on
/// `$PATH`. `which` is the runtime PATH lookup (injectable for
/// tests); `spec` is the `launch_spec` factory (also injectable).
pub fn pick_first_available(
    priority: &[TerminalKind],
    spec: impl Fn(TerminalKind, &Path) -> Option<LaunchSpec>,
    which: impl Fn(&str) -> Option<PathBuf>,
    working_dir: &Path,
) -> Option<(TerminalKind, LaunchSpec)> {
    for kind in priority.iter().copied() {
        let Some(s) = spec(kind, working_dir) else {
            continue;
        };
        if which(&s.program).is_some() {
            return Some((kind, s));
        }
    }
    None
}

/// Resolve a `TerminalKind` from its serialised name (matches the
/// `serde(rename_all = "snake_case")` form). Used so the IPC caller
/// can override the priority list with the same string tags the
/// settings UI shows.
#[must_use]
pub fn parse_kind(name: &str) -> Option<TerminalKind> {
    Some(match name {
        "kitty" => TerminalKind::Kitty,
        "alacritty" => TerminalKind::Alacritty,
        "wezterm" => TerminalKind::Wezterm,
        "ghostty" => TerminalKind::Ghostty,
        "gnome_terminal" | "gnome-terminal" => TerminalKind::GnomeTerminal,
        "konsole" => TerminalKind::Konsole,
        "xfce4_terminal" | "xfce4-terminal" => TerminalKind::Xfce4Terminal,
        "xterm" => TerminalKind::Xterm,
        "x_terminal_emulator" | "x-terminal-emulator" => TerminalKind::XTerminalEmulator,
        "windows_terminal" | "wt" => TerminalKind::WindowsTerminal,
        "mac_terminal" | "terminal" => TerminalKind::MacTerminal,
        "iterm2" | "iterm" => TerminalKind::Iterm2,
        _ => return None,
    })
}

/// Spawn the chosen emulator detached from this process. stdio is
/// dropped to `Stdio::null()` so a misbehaving emulator can't write
/// onto our parent log; modern emulators fork-exec themselves on
/// startup, so the spawned `Child` exits almost immediately and the
/// dropped handle gets reaped on the next OS pass.
///
/// On Unix the child is given a fresh session via `setsid` so that a
/// SIGHUP delivered to nexus's process group doesn't tear the
/// terminal down with it.
pub fn spawn_detached(spec: &LaunchSpec) -> std::io::Result<()> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // SAFETY: setsid is async-signal-safe and the only fd state
        // the closure mutates is the kernel-managed session id.
        unsafe {
            cmd.pre_exec(|| {
                // Detach from our session / controlling TTY.
                // Failure is non-fatal — we still get a usable child.
                let _ = libc::setsid();
                Ok(())
            });
        }
    }

    cmd.spawn().map(drop)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn launch_spec_kitty_uses_directory_flag() {
        let s = launch_spec(TerminalKind::Kitty, Path::new("/tmp/work")).unwrap();
        assert_eq!(s.program, "kitty");
        assert_eq!(s.args, vec!["--directory".to_string(), "/tmp/work".into()]);
    }

    #[test]
    fn launch_spec_alacritty_uses_working_directory_flag() {
        let s = launch_spec(TerminalKind::Alacritty, Path::new("/tmp/work")).unwrap();
        assert_eq!(s.program, "alacritty");
        assert_eq!(
            s.args,
            vec!["--working-directory".to_string(), "/tmp/work".into()],
        );
    }

    #[test]
    fn launch_spec_wezterm_uses_start_cwd() {
        let s = launch_spec(TerminalKind::Wezterm, Path::new("/tmp/work")).unwrap();
        assert_eq!(s.program, "wezterm");
        assert_eq!(
            s.args,
            vec!["start".to_string(), "--cwd".into(), "/tmp/work".into()],
        );
    }

    #[test]
    fn launch_spec_ghostty_packs_into_single_equals_arg() {
        let s = launch_spec(TerminalKind::Ghostty, Path::new("/tmp/work")).unwrap();
        assert_eq!(s.args, vec!["--working-directory=/tmp/work".to_string()]);
    }

    #[test]
    fn launch_spec_xterm_falls_back_to_bash_dash_c() {
        let s = launch_spec(TerminalKind::Xterm, Path::new("/tmp/work")).unwrap();
        assert_eq!(s.program, "xterm");
        // -e bash -c "cd '/tmp/work' && exec $SHELL"
        assert_eq!(s.args[0], "-e");
        assert_eq!(s.args[1], "bash");
        assert_eq!(s.args[2], "-c");
        assert!(s.args[3].contains("cd '/tmp/work'"));
        assert!(s.args[3].ends_with("exec $SHELL"));
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("with'quote"), r"'with'\''quote'");
    }

    #[test]
    fn pick_first_available_skips_uninstalled_then_picks_next() {
        // Stub `which` so only `alacritty` is "installed". Priority
        // order has Wezterm first (skipped) then Alacritty (picked).
        let priority = vec![TerminalKind::Wezterm, TerminalKind::Alacritty];
        let which = |program: &str| -> Option<PathBuf> {
            (program == "alacritty").then(|| PathBuf::from("/usr/bin/alacritty"))
        };
        let (kind, spec) = pick_first_available(
            &priority,
            launch_spec,
            which,
            Path::new("/tmp"),
        )
        .expect("alacritty available");
        assert_eq!(kind, TerminalKind::Alacritty);
        assert_eq!(spec.program, "alacritty");
    }

    #[test]
    fn pick_first_available_returns_none_when_nothing_resolves() {
        let priority = vec![TerminalKind::Kitty];
        let result = pick_first_available(
            &priority,
            launch_spec,
            |_| None,
            Path::new("/tmp"),
        );
        assert!(result.is_none());
    }

    #[test]
    fn parse_kind_accepts_snake_case_and_hyphenated() {
        assert_eq!(parse_kind("kitty"), Some(TerminalKind::Kitty));
        assert_eq!(parse_kind("gnome_terminal"), Some(TerminalKind::GnomeTerminal));
        assert_eq!(parse_kind("gnome-terminal"), Some(TerminalKind::GnomeTerminal));
        assert_eq!(parse_kind("iterm"), Some(TerminalKind::Iterm2));
        assert_eq!(parse_kind("wt"), Some(TerminalKind::WindowsTerminal));
        assert_eq!(parse_kind("nope"), None);
    }
}
