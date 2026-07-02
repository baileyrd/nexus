//! Integration tests for the public PTY session surface
//! (gap-analysis 2026-07-01 §4 / queue item 6 — nexus-terminal
//! previously had no `tests/` dir; the spawn → write → read → exit
//! path was covered only by in-src unit tests).
//!
//! Unix-only: the tests spawn a real `/bin/sh` under a PTY. Windows
//! coverage rides the in-src tests until a ConPTY runner exists in CI.

#![cfg(unix)]

use std::time::{Duration, Instant};

use nexus_terminal::{ProcessState, Session, SessionConfig, ShellSpec};

/// Read from the session until `needle` appears in the accumulated
/// output or `deadline` elapses. Returns the transcript so far.
fn read_until(session: &mut Session, needle: &str, deadline: Duration) -> String {
    let start = Instant::now();
    let mut transcript = String::new();
    let mut buf = [0u8; 4096];
    while start.elapsed() < deadline {
        match session.read(&mut buf, Duration::from_millis(100)) {
            Ok(0) => {}
            Ok(n) => transcript.push_str(&String::from_utf8_lossy(&buf[..n])),
            // Timeouts while waiting are expected; real errors end the
            // wait and let the assertion below explain.
            Err(_) => break,
        }
        if transcript.contains(needle) {
            break;
        }
    }
    transcript
}

fn sh_config() -> SessionConfig {
    SessionConfig {
        // Pin /bin/sh so the test doesn't depend on the runner's $SHELL.
        shell: Some(ShellSpec::bare("/bin/sh")),
        ..SessionConfig::default()
    }
}

#[test]
fn spawn_echo_read_roundtrip() {
    let mut session = Session::spawn(sh_config()).expect("spawn /bin/sh under a PTY");
    assert!(session.pid().is_some(), "spawned session must have a pid");
    assert!(matches!(session.state(), ProcessState::Running { .. }));

    session
        .write(b"echo nexus-pty-roundtrip-marker\n")
        .expect("write to PTY");
    let transcript = read_until(
        &mut session,
        "nexus-pty-roundtrip-marker",
        Duration::from_secs(10),
    );
    assert!(
        transcript.contains("nexus-pty-roundtrip-marker"),
        "echoed marker must appear in PTY output; transcript so far: {transcript:?}"
    );
}

#[test]
fn exit_transitions_out_of_running() {
    let mut session = Session::spawn(sh_config()).expect("spawn /bin/sh under a PTY");
    session.write(b"exit 0\n").expect("write exit");

    // `state()` is a latched value — the child is only reaped by
    // `try_wait_exit`. Drain output and poll until it reports exit.
    let start = Instant::now();
    let mut buf = [0u8; 1024];
    let mut exited = false;
    while start.elapsed() < Duration::from_secs(10) {
        let _ = session.read(&mut buf, Duration::from_millis(100));
        if session.try_wait_exit().is_some() {
            exited = true;
            break;
        }
    }
    assert!(exited, "child must be reapable after `exit`");
    assert!(
        !matches!(session.state(), ProcessState::Running { .. }),
        "latched state must leave Running once reaped, state: {:?}",
        session.state()
    );
}
