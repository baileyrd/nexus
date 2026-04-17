//! PTY-backed terminal session (PRD-09 §1.1 / §2.1).
//!
//! A [`Session`] owns one PTY + one child process. Callers write input,
//! read output, resize the pty as the UI resizes, and `kill` when done.
//! On drop the child is force-killed so tests and short-lived sessions
//! don't leak zombies.
//!
//! Everything here is synchronous by design. Async callers wrap each
//! method in `spawn_blocking`, exactly as `nexus-git::GitWorkerHandle`
//! does for `git2`. Keeping the library runtime-agnostic means the core
//! plugin that wraps it can choose its own concurrency shape.

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};

use crate::error::TerminalError;
use crate::shell::{detect_default_shell, ShellSpec};

/// Stable identifier for a session. Wraps a UUID so callers can key a
/// registry or persist references without leaking the PTY type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Mint a fresh, random id.
    #[must_use]
    pub fn new_random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Build from an externally-supplied string (e.g. loaded from `SQLite`).
    #[must_use]
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Raw id for display / persistence.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parameters for [`Session::spawn`]. All fields are optional — an empty
/// [`SessionConfig::default`] is sufficient to open a shell with sensible
/// defaults.
#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    /// Explicit shell override (PRD-09 §1.2 item 1). Takes priority over
    /// `$SHELL` / platform fallbacks when present.
    pub shell: Option<ShellSpec>,
    /// Working directory the child is spawned in. `None` = inherit the
    /// parent's cwd.
    pub working_dir: Option<PathBuf>,
    /// Initial PTY size. Defaults to 80×24.
    pub initial_size: Option<PtySize>,
    /// Extra env vars merged on top of the inherited environment.
    pub env: Vec<(String, String)>,
}

/// Live terminal session — a PTY master + a child process we spawned on
/// it. Thread-safety caveat: `MasterPty`, `Child`, and the reader are all
/// `Send` but not `Sync`, so the session is not meant to be shared
/// unsynchronised. Each method takes `&mut self` and callers hold the
/// session exclusively.
pub struct Session {
    id: SessionId,
    master: Box<dyn MasterPty + Send>,
    /// The spawned child — `None` after [`Self::kill`] returns.
    child: Option<Box<dyn Child + Send + Sync>>,
    /// Persistent reader over the PTY's stdout-side. Keep it around so
    /// short successive reads don't pay a clone cost.
    reader: Arc<Mutex<Box<dyn Read + Send>>>,
    /// Persistent writer into the PTY's stdin-side.
    writer: Box<dyn Write + Send>,
    /// Cached shell command for error messages.
    shell_display: String,
}

impl Session {
    /// Spawn a fresh PTY + child shell.
    ///
    /// # Errors
    /// Returns [`TerminalError::PtyAlloc`] if the host refuses to allocate
    /// a pty, or [`TerminalError::Spawn`] if the shell binary cannot be
    /// launched (not on PATH, permission denied, …).
    pub fn spawn(config: SessionConfig) -> Result<Self, TerminalError> {
        let shell = config.shell.unwrap_or_else(detect_default_shell);
        let shell_display = shell.program.display().to_string();

        let size = config.initial_size.unwrap_or(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        });

        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(size)
            .map_err(|e| TerminalError::PtyAlloc(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&shell.program);
        for a in &shell.args {
            cmd.arg(a);
        }
        if let Some(ref wd) = config.working_dir {
            cmd.cwd(wd);
        }
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TerminalError::Spawn {
                shell: shell_display.clone(),
                reason: e.to_string(),
            })?;

        // `slave` is dropped on purpose — the child keeps its fds; holding
        // the slave side open here would prevent EOF propagation to the
        // reader when the child exits.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TerminalError::PtyAlloc(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| TerminalError::PtyAlloc(e.to_string()))?;

        Ok(Self {
            id: SessionId::new_random(),
            master: pair.master,
            child: Some(child),
            reader: Arc::new(Mutex::new(reader)),
            writer,
            shell_display,
        })
    }

    /// Identifier for this session. Stable for the session's lifetime.
    #[must_use]
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// Write bytes into the child's stdin. Bytes are flushed immediately.
    ///
    /// # Errors
    /// Returns [`TerminalError::NotRunning`] if the session has been
    /// killed, or [`TerminalError::Io`] on a raw write failure.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.child.is_none() {
            return Err(TerminalError::NotRunning(self.id.0.clone()));
        }
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Read up to `buf.len()` bytes from the PTY, blocking up to `timeout`
    /// for the first byte. Returns the number of bytes actually read — 0
    /// means the child has closed its side (EOF).
    ///
    /// `portable-pty` readers are blocking; to honour the timeout we poll
    /// with a short sleep and a wall-clock budget. It's coarse (tens of
    /// milliseconds), which is fine for this phase — structured streaming
    /// lands in the future output-ring-buffer pass (PRD-09 §3).
    ///
    /// # Panics
    /// Panics if the internal reader mutex is poisoned — which only
    /// happens if another thread holding the lock panicked mid-read. A
    /// poisoned mutex indicates a prior unrecoverable I/O failure, so
    /// continuing with a stale reader would hide the bug.
    ///
    /// # Errors
    /// Returns [`TerminalError::Io`] on non-WouldBlock read failures.
    pub fn read(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TerminalError> {
        let deadline = Instant::now() + timeout;
        loop {
            // Hold the reader mutex only for the poll; release between
            // retries so Drop / kill paths can interrupt.
            let mut guard = self
                .reader
                .lock()
                .expect("reader mutex poisoned — another thread panicked mid-read");
            match guard.read(buf) {
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    drop(guard);
                    if Instant::now() >= deadline {
                        return Ok(0);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Update the PTY's reported window size — drives SIGWINCH delivery
    /// to the child (PRD-09 §1.1 "size synchronized with UI viewport").
    ///
    /// # Errors
    /// Returns [`TerminalError::Io`] on ioctl failure.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        self.master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    /// Force-kill the child process. Subsequent reads/writes return
    /// [`TerminalError::NotRunning`]. Idempotent — killing an
    /// already-killed session is a no-op.
    ///
    /// # Errors
    /// Returns [`TerminalError::Io`] if the underlying `kill` syscall
    /// fails (platform-specific — e.g. permission denied on a process we
    /// no longer have the rights to signal).
    pub fn kill(&mut self) -> Result<(), TerminalError> {
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .map_err(|e| TerminalError::Io(std::io::Error::other(e.to_string())))?;
            // Best-effort wait so the zombie is reaped before we return.
            let _ = child.wait();
        }
        Ok(())
    }

    /// Non-blocking check: has the child exited on its own?
    ///
    /// Returns `None` if the child is still running (or already killed),
    /// `Some(exit_code)` if it has exited cleanly. The exit code is the
    /// platform-dependent value from `Child::wait`.
    pub fn try_wait_exit(&mut self) -> Option<u32> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                // Reap and drop the child so subsequent operations see
                // `NotRunning` instead of continuing to probe a corpse.
                self.child = None;
                Some(status.exit_code())
            }
            _ => None,
        }
    }

    /// Human-readable shell command for logs / error messages.
    #[must_use]
    pub fn shell_display(&self) -> &str {
        &self.shell_display
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // PTY handles are not Debug; surface only the fields that matter
        // for diagnostics.
        // PTY handles (master / reader / writer) are not Debug; surface
        // only the fields that matter for diagnostics and use
        // `finish_non_exhaustive()` to be explicit about the omission.
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("shell", &self.shell_display)
            .field("running", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Best-effort cleanup — ignore errors because Drop cannot
            // propagate them anyway, and the OS will reap eventually.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix_only(test_name: &str) -> bool {
        if !cfg!(unix) {
            eprintln!("skipping {test_name}: unix-only");
            return false;
        }
        true
    }

    #[test]
    fn session_id_is_nonempty_and_unique() {
        let a = SessionId::new_random();
        let b = SessionId::new_random();
        assert!(!a.as_str().is_empty());
        assert_ne!(a, b);
    }

    #[test]
    fn session_id_round_trip_through_from_string() {
        let s = "fixed-id-for-persistence";
        let id = SessionId::from_string(s);
        assert_eq!(id.as_str(), s);
    }

    #[test]
    fn spawn_read_echo_output_and_exit_cleanly() {
        if !unix_only("spawn_read_echo_output_and_exit_cleanly") {
            return;
        }
        // Use `/bin/sh` directly so the test doesn't depend on $SHELL
        // being bash. Give it a command via `-c` that exits immediately.
        let mut session = Session::spawn(SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "printf 'hello from pty'".into()],
            }),
            ..SessionConfig::default()
        })
        .expect("spawn");

        // Read with a 2s budget — plenty for an inline printf.
        let mut buf = [0u8; 256];
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut collected = Vec::new();
        while Instant::now() < deadline {
            let n = session
                .read(&mut buf, Duration::from_millis(200))
                .expect("read");
            if n == 0 {
                break;
            }
            collected.extend_from_slice(&buf[..n]);
            // Check for EOF after each read by polling exit.
            if session.try_wait_exit().is_some() {
                // One more drain after exit so trailing bytes buffered in
                // the pty still land in `collected`.
                let n = session
                    .read(&mut buf, Duration::from_millis(100))
                    .expect("drain");
                collected.extend_from_slice(&buf[..n]);
                break;
            }
        }
        let s = String::from_utf8_lossy(&collected);
        assert!(
            s.contains("hello from pty"),
            "expected 'hello from pty' in output, got: {s:?}"
        );
    }

    #[test]
    fn write_then_read_round_trip_through_cat() {
        if !unix_only("write_then_read_round_trip_through_cat") {
            return;
        }
        // `cat` reads stdin and echoes to stdout — a clean round-trip probe
        // that exercises both `write` and `read` on the same session.
        let mut session = Session::spawn(SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/cat".into(),
                args: vec![],
            }),
            ..SessionConfig::default()
        })
        .expect("spawn cat");

        session.write(b"ping\n").expect("write");
        // Close stdin so `cat` will EOF once it has drained.
        // `cat` doesn't have an easy way to force stdin close from the
        // writer side of a pty master without more ceremony, so instead
        // rely on the next read returning the line and the test ending
        // via kill in Drop.
        let mut buf = [0u8; 64];
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen_ping = false;
        while Instant::now() < deadline {
            let n = session
                .read(&mut buf, Duration::from_millis(200))
                .expect("read");
            if n == 0 {
                continue;
            }
            if String::from_utf8_lossy(&buf[..n]).contains("ping") {
                seen_ping = true;
                break;
            }
        }
        assert!(seen_ping, "did not see 'ping' echoed back from cat");
        // Drop kills the child cleanly.
    }

    #[test]
    fn resize_on_running_session_succeeds() {
        if !unix_only("resize_on_running_session_succeeds") {
            return;
        }
        let mut session = Session::spawn(SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 1".into()],
            }),
            ..SessionConfig::default()
        })
        .expect("spawn");
        session.resize(132, 43).expect("resize");
    }

    #[test]
    fn write_after_kill_returns_not_running() {
        if !unix_only("write_after_kill_returns_not_running") {
            return;
        }
        let mut session = Session::spawn(SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 10".into()],
            }),
            ..SessionConfig::default()
        })
        .expect("spawn");
        session.kill().expect("kill");
        let err = session.write(b"hello\n").unwrap_err();
        assert!(
            matches!(err, TerminalError::NotRunning(_)),
            "expected NotRunning, got {err:?}",
        );
    }

    #[test]
    fn spawn_nonexistent_shell_returns_spawn_error() {
        let err = Session::spawn(SessionConfig {
            shell: Some(ShellSpec {
                program: "/definitely/does/not/exist-12345".into(),
                args: vec![],
            }),
            ..SessionConfig::default()
        })
        .expect_err("should not have spawned");
        assert!(
            matches!(err, TerminalError::Spawn { .. }),
            "expected Spawn, got {err:?}",
        );
    }
}
