//! In-memory session manager (PRD-09 §2).
//!
//! # Scope
//!
//! Owns a collection of live [`Session`] + [`OutputBuffer`] pairs keyed by
//! [`SessionId`] and exposes the per-session operations consumers actually
//! need (spawn, write, drain, resize, kill, snapshot). Enforces the
//! PRD-spec `max_sessions = 50` hard cap so a runaway caller cannot
//! exhaust file descriptors.
//!
//! # What this is NOT (yet)
//!
//! - **`SQLite` persistence (§2.2)**: session metadata and scrollback
//!   survive only as long as the manager is live. A future Phase D pass
//!   can add a `save` / `restore` cycle without changing this shape — the
//!   manager already stores everything a persistence layer needs.
//! - **LRU eviction (§2.3)**: the max-sessions cap is enforced by
//!   rejecting new spawns, not by evicting the oldest. LRU requires a
//!   persistence target for the evicted scrollback.
//! - **Last-accessed timestamps**: tracked as a `last_accessed` instant
//!   so the LRU pass has data when it lands, but no policy reads it yet.
//!
//! # Microkernel fit
//!
//! Still a plain library type. A future `com.nexus.terminal` core plugin
//! can wrap a `Mutex<SessionManager>` inside its `dispatch_async` handler
//! so WASM / script plugins call `com.nexus.terminal.spawn` / `write` /
//! `read` over IPC. Nothing in this module reaches into the kernel bus
//! or capability system; those boundaries live at the core-plugin layer.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::buffer::OutputBuffer;
use crate::error::TerminalError;
use crate::session::{Session, SessionConfig, SessionId, Signal};

/// PRD-09 §2.3 hard cap on simultaneously-active sessions per workspace.
pub const DEFAULT_MAX_SESSIONS: usize = 50;

/// One session plus its captured-output ring. The manager owns the pair
/// so consumers never see a live `Session` without its matching buffer.
struct Entry {
    session: Session,
    buffer: OutputBuffer,
    last_accessed: Instant,
}

/// In-memory registry of live PTY sessions. Not `Sync`; wrap in a mutex
/// for concurrent access.
pub struct SessionManager {
    sessions: HashMap<SessionId, Entry>,
    max_sessions: usize,
    default_buffer_capacity: usize,
}

impl SessionManager {
    /// Build a manager with the PRD defaults (50 sessions cap, 10 MB
    /// ring buffer per session).
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_SESSIONS, OutputBuffer::DEFAULT_CAPACITY)
    }

    /// Build a manager with explicit limits — used by tests to pressure
    /// the session cap without spawning 50 PTYs.
    #[must_use]
    pub fn with_limits(max_sessions: usize, default_buffer_capacity: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions,
            default_buffer_capacity,
        }
    }

    /// Number of sessions currently alive.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the manager holds zero sessions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Hard cap this manager enforces on simultaneous sessions.
    #[must_use]
    pub fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// Every session id currently tracked, in arbitrary order. Callers
    /// that need ordering (e.g. a UI list) should sort on `last_accessed`
    /// via [`Self::last_accessed`].
    #[must_use]
    pub fn ids(&self) -> Vec<SessionId> {
        self.sessions.keys().cloned().collect()
    }

    /// Spawn a new session and store it. Returns the fresh id.
    ///
    /// # Errors
    /// - [`TerminalError::ShellDetection`] when the manager is already at
    ///   its [`Self::max_sessions`] cap.
    /// - Any error from [`Session::spawn`] (PTY alloc failure, shell
    ///   spawn failure, …) — propagated verbatim.
    pub fn spawn(&mut self, config: SessionConfig) -> Result<SessionId, TerminalError> {
        if self.sessions.len() >= self.max_sessions {
            return Err(TerminalError::ShellDetection {
                reason: format!(
                    "session cap reached ({} active, max {})",
                    self.sessions.len(),
                    self.max_sessions
                ),
            });
        }
        let session = Session::spawn(config)?;
        let id = session.id().clone();
        self.sessions.insert(
            id.clone(),
            Entry {
                session,
                buffer: OutputBuffer::with_capacity(self.default_buffer_capacity),
                last_accessed: Instant::now(),
            },
        );
        Ok(id)
    }

    /// Write `bytes` to the session's child stdin.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::write`].
    pub fn write(&mut self, id: &SessionId, bytes: &[u8]) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed = Instant::now();
        entry.session.write(bytes)
    }

    /// Drain whatever is currently available from the PTY into the
    /// session's ring buffer, blocking up to `timeout` for the first
    /// byte. Returns the number of bytes read.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::read_into_buffer`].
    pub fn drain(
        &mut self,
        id: &SessionId,
        timeout: Duration,
    ) -> Result<usize, TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed = Instant::now();
        let Entry { session, buffer, .. } = entry;
        session.read_into_buffer(buffer, timeout)
    }

    /// Update the PTY's reported window size (PRD-09 §1.1).
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::resize`].
    pub fn resize(
        &mut self,
        id: &SessionId,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed = Instant::now();
        entry.session.resize(cols, rows)
    }

    /// Graceful-shutdown ladder (PRD-09 §5.1) on the named session:
    /// SIGINT → wait → SIGTERM → wait → SIGKILL, with `step_timeout`
    /// between each escalation. Returns the signal that actually
    /// terminated the child. See [`Session::request_shutdown`] for
    /// platform caveats (Windows collapses to force-kill).
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::request_shutdown`].
    pub fn request_shutdown(
        &mut self,
        id: &SessionId,
        step_timeout: Duration,
    ) -> Result<Signal, TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed = Instant::now();
        entry.session.request_shutdown(step_timeout)
    }

    /// Force-kill the session's child. The entry stays in the manager so
    /// callers can still read the final buffer contents; remove with
    /// [`Self::remove`] once that's no longer needed.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::kill`].
    pub fn kill(&mut self, id: &SessionId) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed = Instant::now();
        entry.session.kill()
    }

    /// Remove a session from the manager. If the child is still running
    /// it is killed synchronously on drop. Returns the final buffer
    /// snapshot for the caller to persist, log, or display.
    #[must_use]
    pub fn remove(&mut self, id: &SessionId) -> Option<Vec<u8>> {
        self.sessions.remove(id).map(|entry| entry.buffer.snapshot())
    }

    /// Copy the session's current output buffer into a fresh `Vec`.
    /// Returns `None` if `id` is not tracked.
    #[must_use]
    pub fn buffer_snapshot(&self, id: &SessionId) -> Option<Vec<u8>> {
        self.sessions.get(id).map(|e| e.buffer.snapshot())
    }

    /// Byte count currently held in the session's ring buffer. Cheap —
    /// does not allocate.
    #[must_use]
    pub fn buffer_len(&self, id: &SessionId) -> Option<usize> {
        self.sessions.get(id).map(|e| e.buffer.len())
    }

    /// Timestamp of the last read/write/resize/kill for `id`, or `None`
    /// if the session is unknown. Used by the future LRU eviction pass.
    #[must_use]
    pub fn last_accessed(&self, id: &SessionId) -> Option<Instant> {
        self.sessions.get(id).map(|e| e.last_accessed)
    }

    /// Poll every tracked session for natural exit (child finished
    /// without us calling [`Self::kill`]). Returns the ids of sessions
    /// that have exited since the last poll, paired with their exit
    /// code. Child entries stay in the manager — the buffer snapshot
    /// remains readable until [`Self::remove`].
    pub fn reap_exited(&mut self) -> Vec<(SessionId, u32)> {
        let mut exits = Vec::new();
        for (id, entry) in &mut self.sessions {
            if let Some(code) = entry.session.try_wait_exit() {
                exits.push((id.clone(), code));
            }
        }
        exits
    }

    fn entry_mut(&mut self, id: &SessionId) -> Result<&mut Entry, TerminalError> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| TerminalError::NotRunning(id.as_str().to_string()))
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionManager")
            .field("sessions", &self.sessions.len())
            .field("max_sessions", &self.max_sessions)
            .field("default_buffer_capacity", &self.default_buffer_capacity)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellSpec;

    fn unix_only(test_name: &str) -> bool {
        if !cfg!(unix) {
            eprintln!("skipping {test_name}: unix-only");
            return false;
        }
        true
    }

    fn sh_printf(marker: &str) -> SessionConfig {
        SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("printf '{marker}'")],
            }),
            ..SessionConfig::default()
        }
    }

    #[test]
    fn new_manager_is_empty_and_at_default_cap() {
        let m = SessionManager::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert_eq!(m.max_sessions(), DEFAULT_MAX_SESSIONS);
        assert!(m.ids().is_empty());
    }

    #[test]
    fn spawn_tracks_session_and_returns_stable_id() {
        if !unix_only("spawn_tracks_session_and_returns_stable_id") {
            return;
        }
        let mut m = SessionManager::new();
        let id = m.spawn(sh_printf("smoke")).expect("spawn");
        assert_eq!(m.len(), 1);
        assert!(m.ids().contains(&id));
        assert!(m.last_accessed(&id).is_some());
    }

    #[test]
    fn spawn_refuses_past_max_sessions_cap() {
        if !unix_only("spawn_refuses_past_max_sessions_cap") {
            return;
        }
        let mut m = SessionManager::with_limits(2, 1024);
        let _a = m.spawn(sh_printf("a")).expect("spawn a");
        let _b = m.spawn(sh_printf("b")).expect("spawn b");
        let err = m.spawn(sh_printf("c")).unwrap_err();
        assert!(
            matches!(err, TerminalError::ShellDetection { .. }),
            "expected cap error, got {err:?}",
        );
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn drain_routes_output_into_owned_buffer() {
        if !unix_only("drain_routes_output_into_owned_buffer") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 1024);
        let id = m.spawn(sh_printf("hello-from-mgr")).expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let _ = m.drain(&id, Duration::from_millis(200)).expect("drain");
            let snap = m.buffer_snapshot(&id).expect("buffer_snapshot");
            if snap.windows(14).any(|w| w == b"hello-from-mgr") {
                return;
            }
        }
        let snap = m.buffer_snapshot(&id).unwrap_or_default();
        panic!(
            "expected marker in buffer, got {:?}",
            String::from_utf8_lossy(&snap),
        );
    }

    #[test]
    fn unknown_id_returns_not_running() {
        let mut m = SessionManager::new();
        let ghost = SessionId::from_string("ghost-id");
        let err = m.write(&ghost, b"x").unwrap_err();
        assert!(matches!(err, TerminalError::NotRunning(_)), "got {err:?}");
        let err = m.drain(&ghost, Duration::from_millis(10)).unwrap_err();
        assert!(matches!(err, TerminalError::NotRunning(_)), "got {err:?}");
    }

    #[test]
    fn remove_returns_final_buffer_and_drops_entry() {
        if !unix_only("remove_returns_final_buffer_and_drops_entry") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 1024);
        let id = m.spawn(sh_printf("final-bytes")).expect("spawn");
        // Give the child time to print and land in the ring.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let _ = m.drain(&id, Duration::from_millis(200)).expect("drain");
            if m.buffer_len(&id).unwrap_or(0) > 0 {
                break;
            }
        }
        let snapshot = m.remove(&id).expect("remove returns snapshot");
        assert!(
            String::from_utf8_lossy(&snapshot).contains("final-bytes"),
            "snapshot missing marker: {:?}",
            String::from_utf8_lossy(&snapshot),
        );
        assert!(m.is_empty());
        assert!(m.buffer_snapshot(&id).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn request_shutdown_returns_signal_used() {
        if !unix_only("request_shutdown_returns_signal_used") {
            return;
        }
        let mut m = SessionManager::with_limits(2, 1024);
        let id = m
            .spawn(SessionConfig {
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 5".into()],
                }),
                ..SessionConfig::default()
            })
            .expect("spawn");
        let finisher = m
            .request_shutdown(&id, Duration::from_millis(500))
            .expect("shutdown");
        assert_eq!(finisher, Signal::Int);
    }

    #[test]
    fn reap_exited_surfaces_naturally_finished_children() {
        if !unix_only("reap_exited_surfaces_naturally_finished_children") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 1024);
        let id = m
            .spawn(SessionConfig {
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "true".into()],
                }),
                ..SessionConfig::default()
            })
            .expect("spawn");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = None;
        while Instant::now() < deadline {
            let exits = m.reap_exited();
            if let Some((exited_id, code)) = exits.into_iter().next() {
                assert_eq!(exited_id, id);
                seen = Some(code);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen.is_some(), "child never surfaced in reap_exited");
        // Entry stays in the manager so the caller can still inspect it.
        assert_eq!(m.len(), 1);
    }
}
