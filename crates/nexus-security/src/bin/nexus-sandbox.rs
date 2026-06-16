//! `nexus-sandbox` — the OS-sandbox helper sidecar.
//!
//! Applies a [`SandboxPolicy`](nexus_types::SandboxPolicy) to *itself* and then
//! `exec`s a target command. Because it is single-threaded by construction,
//! installing Landlock / seccomp here is free of the after-`fork()` allocation
//! hazard that a `std::process::Command::pre_exec` hook would have in a
//! multithreaded parent (see `docs/0.1.2/os-sandbox.md`).
//!
//! Invoked by [`nexus_security::os_sandbox::sandbox_command`]. Argv layout:
//!
//! ```text
//! nexus-sandbox <policy-json> <cwd> -- <program> [args...]
//! ```
//!
//! On success this process is *replaced* by the target (so the target inherits
//! the confinement, which survives `execve`). On any error it prints a
//! `nexus-sandbox:` diagnostic to stderr and exits non-zero.

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        linux::run()
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("nexus-sandbox: OS sandbox is only supported on Linux");
        ExitCode::FAILURE
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;
    use std::process::{Command, ExitCode};

    use std::os::unix::process::CommandExt;

    use nexus_security::os_sandbox::confine_current_thread;
    use nexus_types::SandboxPolicy;

    pub(super) fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        // args[0] = self, [1] = policy json, [2] = cwd, [3] = "--", [4] = program, [5..] = args.
        if args.len() < 5 || args[3] != "--" {
            eprintln!(
                "nexus-sandbox: usage: nexus-sandbox <policy-json> <cwd> -- <program> [args...]"
            );
            return ExitCode::FAILURE;
        }
        let policy: SandboxPolicy = match serde_json::from_str(&args[1]) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("nexus-sandbox: invalid policy json: {e}");
                return ExitCode::FAILURE;
            }
        };
        let cwd = Path::new(&args[2]);
        let program = &args[4];
        let rest = &args[5..];

        // Confine *before* exec; the Landlock domain + seccomp filter survive
        // execve, so the target runs inside them.
        if let Err(e) = confine_current_thread(&policy, cwd) {
            eprintln!("nexus-sandbox: failed to apply sandbox policy: {e}");
            return ExitCode::FAILURE;
        }

        // `exec` replaces this process and only returns on failure.
        let err = Command::new(program).args(rest).current_dir(cwd).exec();
        eprintln!("nexus-sandbox: failed to exec {program:?}: {err}");
        ExitCode::FAILURE
    }
}
