//! End-to-end tests for the `nexus-sandbox` helper sidecar (Linux only).
//!
//! These verify the *plumbing* — the helper decodes the policy, applies
//! confinement, and `exec`s the target with output / exit codes flowing
//! through. The actual filesystem/network *denials* are unit-verified in
//! `nexus_security::os_sandbox` (in-process socket/file probes); here we only
//! depend on `/bin/sh`, which is universal, so the tests stay robust across
//! kernels (Landlock may or may not enforce; seccomp does in this container).

#![cfg(target_os = "linux")]

use std::path::Path;

use nexus_security::sandbox_command;
use nexus_types::SandboxPolicy;

fn helper() -> &'static str {
    env!("CARGO_BIN_EXE_nexus-sandbox")
}

#[test]
fn helper_runs_target_and_passes_output() {
    let mut cmd = sandbox_command(
        Path::new(helper()),
        &SandboxPolicy::DangerFullAccess,
        Path::new("/"),
        "/bin/sh",
        ["-c", "echo SANDBOXED"],
    )
    .unwrap();
    let out = cmd.output().expect("run helper");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "SANDBOXED");
}

#[test]
fn helper_propagates_exit_code_under_confinement() {
    // workspace-write, no network → confinement is applied (landlock best-effort
    // + seccomp). A plain `exit 7` must still run and propagate, proving confine
    // does not break a normal exec.
    let policy = SandboxPolicy::new_workspace_write(vec![]);
    let mut cmd = sandbox_command(
        Path::new(helper()),
        &policy,
        Path::new("/tmp"),
        "/bin/sh",
        ["-c", "exit 7"],
    )
    .unwrap();
    let status = cmd.status().expect("run helper");
    assert_eq!(
        status.code(),
        Some(7),
        "confinement must not break exec / exit-code propagation"
    );
}

#[test]
fn helper_reports_missing_program() {
    let mut cmd = sandbox_command(
        Path::new(helper()),
        &SandboxPolicy::DangerFullAccess,
        Path::new("/"),
        "/nonexistent/program",
        Vec::<&str>::new(),
    )
    .unwrap();
    let out = cmd.output().expect("run helper");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nexus-sandbox:"),
        "expected a nexus-sandbox diagnostic on stderr"
    );
}
