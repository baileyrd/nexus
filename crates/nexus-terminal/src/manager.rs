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

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::buffer::OutputBuffer;
use crate::error::TerminalError;
use crate::lines::{Line, LineBuffer};
use crate::session::{ProcessState, Session, SessionConfig, SessionId, Signal};

/// PRD-09 §2.3 hard cap on simultaneously-active sessions per workspace.
pub const DEFAULT_MAX_SESSIONS: usize = 50;

/// One session plus its captured-output rings. The manager owns all
/// three so consumers never see a live `Session` without the buffers it
/// feeds: [`OutputBuffer`] holds raw bytes (PRD-09 §3.1), [`LineBuffer`]
/// holds the ANSI-stripped line view the server API exposes (§3.2).
struct Entry {
    session: Session,
    buffer: OutputBuffer,
    lines: LineBuffer,
    /// Most-recent access time. `Cell` so read-side accessors that
    /// take `&self` (e.g. `lines_snapshot`, `buffer_read_since`) can
    /// bump it without forcing `&mut self` everywhere upstream.
    /// Single-threaded by construction — `SessionManager` is `!Sync`,
    /// so callers wrap it in a `Mutex<SessionManager>` and the
    /// interior mutability is safe.
    last_accessed: Cell<Instant>,
    /// Optional human-readable label for the programmable API's
    /// `SessionInfo` surface (PRD-09 §11). `None` falls back to the
    /// session id for display.
    name: Option<String>,
    /// Unix-seconds creation timestamp. Stable across the session's
    /// lifetime — `last_accessed` moves independently.
    created_at: u64,
    /// Server-side VT grid fed the same raw PTY bytes as the buffers above
    /// (RFC 0003). Models the screen + scrollback + OSC 133 command/exit-code
    /// boundaries for headless (agent/CLI/TUI) introspection, parallel to the
    /// frontend's own emulator (xterm.js).
    vt: nexus_vt::Vt,
}

/// A finished command's exit code (`None` when the shell omitted it) and its
/// captured output (`None` when no OSC 133;C output region was marked). Returned
/// by [`SessionManager::take_finished_command`].
pub type FinishedCommand = (Option<i32>, Option<String>);

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
        // Size the VT grid to the requested PTY size (default 80×24, matching
        // `Session::spawn`). Read before `config` is moved into spawn.
        let (cols, rows) = match &config.initial_size {
            Some(s) => (s.cols as usize, s.rows as usize),
            None => (80, 24),
        };
        let session = Session::spawn(config)?;
        let id = session.id().clone();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.sessions.insert(
            id.clone(),
            Entry {
                session,
                buffer: OutputBuffer::with_capacity(self.default_buffer_capacity),
                lines: LineBuffer::new(),
                last_accessed: Cell::new(Instant::now()),
                name: None,
                created_at: now,
                vt: nexus_vt::Vt::new(cols, rows),
            },
        );
        Ok(id)
    }

    /// Attach a human-readable label to a session. Used by the
    /// programmable API's `SessionInfo` surface (PRD-09 §11) so callers
    /// can identify sessions by name rather than by UUID.
    ///
    /// # Errors
    /// [`TerminalError::NotRunning`] if `id` is not tracked.
    pub fn set_name(
        &mut self,
        id: &SessionId,
        name: impl Into<String>,
    ) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.name = Some(name.into());
        Ok(())
    }

    /// Read the label previously set via [`Self::set_name`], or `None`
    /// if none was set (or the session is unknown).
    #[must_use]
    pub fn name(&self, id: &SessionId) -> Option<&str> {
        self.sessions.get(id).and_then(|e| e.name.as_deref())
    }

    /// OS process id of the session's spawned child shell, or `None` if
    /// the session is unknown / the child has been reaped. Powers the
    /// memory-monitor wiring (BL-061) — the poller needs the live pid
    /// to read RSS without re-discovering it through the manager API.
    #[must_use]
    pub fn pid(&self, id: &SessionId) -> Option<u32> {
        self.sessions.get(id).and_then(|e| e.session.pid())
    }

    /// Unix-seconds timestamp the session was spawned at, or `None` if
    /// the id is unknown. Feeds the `SessionInfo.created_at` field.
    #[must_use]
    pub fn created_at(&self, id: &SessionId) -> Option<u64> {
        self.sessions.get(id).map(|e| e.created_at)
    }

    /// Number of structured lines currently held for `id`.
    #[must_use]
    pub fn line_count(&self, id: &SessionId) -> Option<usize> {
        self.sessions.get(id).map(|e| e.lines.len())
    }

    /// Snapshot of the session's structured line view — the ANSI-
    /// stripped, ring-bounded form the server API reads from. Returns
    /// a range `[start, start + count)`, clamped to the available lines.
    /// Omitting `start` reads from the front; omitting `count` reads to
    /// the end.
    ///
    /// Cloning `Line` records is cheap by design (plain `String` +
    /// `Vec<u8>`) — the caller can filter/search them freely without
    /// holding any lock on the manager.
    #[must_use]
    pub fn lines_snapshot(
        &self,
        id: &SessionId,
        start: Option<usize>,
        count: Option<usize>,
    ) -> Option<Vec<Line>> {
        let entry = self.sessions.get(id)?;
        // BL-062 — bump LRU on every read so a busy session that
        // only consumes output (no input, no resize) doesn't fall
        // off the eviction list. `Cell::set` is cheap; the access
        // is single-threaded under the manager's outer mutex.
        entry.last_accessed.set(Instant::now());
        let total = entry.lines.len();
        let start = start.unwrap_or(0).min(total);
        let end = match count {
            Some(c) => start.saturating_add(c).min(total),
            None => total,
        };
        Some(
            entry
                .lines
                .iter()
                .skip(start)
                .take(end - start)
                .cloned()
                .collect(),
        )
    }

    /// The session's current visible screen as text, modelled by the VT grid
    /// (RFC 0003). `None` if the id is unknown.
    #[must_use]
    pub fn vt_screen(&self, id: &SessionId) -> Option<String> {
        let entry = self.sessions.get(id)?;
        entry.last_accessed.set(Instant::now());
        Some(entry.vt.screen_text())
    }

    /// The most recent `max` scrollback lines of the VT grid as text (oldest
    /// first). `None` if the id is unknown.
    #[must_use]
    pub fn vt_scrollback(&self, id: &SessionId, max: usize) -> Option<String> {
        let entry = self.sessions.get(id)?;
        entry.last_accessed.set(Instant::now());
        Some(entry.vt.scrollback_text(max))
    }

    /// The VT grid cursor as `(col, row)` (both zero-based). `None` if unknown.
    #[must_use]
    pub fn vt_cursor(&self, id: &SessionId) -> Option<(usize, usize)> {
        let entry = self.sessions.get(id)?;
        entry.last_accessed.set(Instant::now());
        Some(entry.vt.cursor())
    }

    /// The working directory last reported by the child via OSC 7 (empty until
    /// set). `None` if the id is unknown.
    #[must_use]
    pub fn vt_cwd(&self, id: &SessionId) -> Option<String> {
        let entry = self.sessions.get(id)?;
        entry.last_accessed.set(Instant::now());
        Some(entry.vt.cwd().to_string())
    }

    /// The last finished command's exit code and captured output (OSC 133;C..D).
    /// Read-only — unlike [`Self::take_finished_command`] this does not clear the
    /// pending flag. `None` if the id is unknown.
    #[must_use]
    pub fn vt_last_exit(&self, id: &SessionId) -> Option<FinishedCommand> {
        let entry = self.sessions.get(id)?;
        entry.last_accessed.set(Instant::now());
        Some((
            entry.vt.last_exit(),
            entry.vt.last_command_output().map(str::to_owned),
        ))
    }

    /// Search the session's line buffer for `query`. Returns the indices
    /// of matching lines (into the line buffer as it is *now* — indices
    /// are not stable across eviction). `is_regex = true` interprets the
    /// query as a regular expression; an invalid regex returns `None`.
    #[must_use]
    pub fn lines_search(&self, id: &SessionId, query: &str, is_regex: bool) -> Option<Vec<usize>> {
        let entry = self.sessions.get(id)?;
        if is_regex {
            let re = regex_lite::Regex::new(query).ok()?;
            Some(
                entry
                    .lines
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| re.is_match(&l.text_only))
                    .map(|(i, _)| i)
                    .collect(),
            )
        } else {
            Some(
                entry
                    .lines
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.text_only.contains(query))
                    .map(|(i, _)| i)
                    .collect(),
            )
        }
    }

    /// PRD-09 §2.3 LRU eviction — closes the least-recently-accessed
    /// **terminated** session and returns its id + final scrollback
    /// snapshot. Live (still-running) sessions are skipped: BL-062
    /// makes a deliberate distinction between "free up space by
    /// reaping a corpse" and "kill someone's running process" — only
    /// the first is auto-eviction.
    ///
    /// The caller is expected to persist the snapshot (via
    /// [`crate::persist::SqliteSessionStore::save_scrollback`] or
    /// equivalent) before dropping it.
    ///
    /// Returns `None` when the manager is empty **or** every session
    /// is still running. Callers that need the "all running" case
    /// distinguished from "manager empty" can check `len()` first.
    #[must_use]
    pub fn evict_lru(&mut self) -> Option<(SessionId, Vec<u8>)> {
        // Refresh cached `state()` first — `try_wait_exit` returns
        // `Some` for any child that has exited but hadn't been reaped
        // yet, latching `state` to `Exited` / `Killed`. Without this
        // pass, a session whose child has finished but no one polled
        // would still report `Running` and skip eviction.
        let _ = self.reap_exited();
        // Filter to entries whose underlying session has terminated.
        // `state()` is a cheap field read — no syscall — so iterating
        // every session here is fine even at the 50-session cap.
        let victim = self
            .sessions
            .iter()
            .filter(|(_, entry)| entry.session.state().is_terminated())
            .min_by_key(|(_, entry)| entry.last_accessed.get())
            .map(|(id, _)| id.clone())?;
        let snapshot = self.remove(&victim)?;
        tracing::debug!(
            session_id = victim.as_str(),
            "evicted LRU stopped session to make room",
        );
        Some((victim, snapshot))
    }

    /// Same as [`Self::spawn`], but when the manager is at its cap this
    /// evicts the LRU **stopped** session first (rather than returning
    /// an error). When every session is still running, the cap check
    /// in [`Self::spawn`] surfaces [`TerminalError::ShellDetection`] —
    /// matching the pre-BL-062 behaviour, deliberately preserved so an
    /// auto-spawn never SIGKILLs a process the user is actively
    /// driving.
    ///
    /// Returns the fresh id plus, if eviction occurred, the evicted id
    /// + its final scrollback snapshot so the caller can persist it.
    ///
    /// # Errors
    /// Propagates any error from [`Session::spawn`].
    #[allow(clippy::type_complexity)]
    pub fn spawn_or_evict(
        &mut self,
        config: SessionConfig,
    ) -> Result<(SessionId, Option<(SessionId, Vec<u8>)>), TerminalError> {
        let evicted = if self.sessions.len() >= self.max_sessions {
            self.evict_lru()
        } else {
            None
        };
        let id = self.spawn(config)?;
        Ok((id, evicted))
    }

    /// Write `bytes` to the session's child stdin.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::write`].
    pub fn write(&mut self, id: &SessionId, bytes: &[u8]) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed.set(Instant::now());
        entry.session.write(bytes)
    }

    /// Drain whatever is currently available from the PTY into the
    /// session's ring buffer, blocking up to `timeout` for the first
    /// byte. Returns the number of bytes read.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::read_into_buffer`].
    pub fn drain(&mut self, id: &SessionId, timeout: Duration) -> Result<usize, TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed.set(Instant::now());
        let Entry {
            session,
            buffer,
            lines,
            vt,
            ..
        } = entry;
        session.read_into(Some(buffer), Some(lines), Some(vt), timeout)
    }

    /// Drain the session's "a command just finished" flag (OSC 133;D), returning
    /// the exit code (`None` if the shell omitted it) and captured output for the
    /// completion, or `None` if no command finished since the last call. Used by
    /// the server to emit one `CommandFinished` event per completion.
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    pub fn take_finished_command(
        &mut self,
        id: &SessionId,
    ) -> Result<Option<FinishedCommand>, TerminalError> {
        Ok(self.entry_mut(id)?.vt.take_finished_command())
    }

    /// Update the PTY's reported window size (PRD-09 §1.1).
    ///
    /// # Errors
    /// - [`TerminalError::NotRunning`] if `id` is not tracked.
    /// - Any I/O error from [`Session::resize`].
    pub fn resize(&mut self, id: &SessionId, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let entry = self.entry_mut(id)?;
        entry.last_accessed.set(Instant::now());
        // Keep the server-side VT grid in step with the PTY so screen reads and
        // OSC 133 output capture survive a mid-command resize.
        entry.vt.resize(cols as usize, rows as usize);
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
        entry.last_accessed.set(Instant::now());
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
        entry.last_accessed.set(Instant::now());
        entry.session.kill()
    }

    /// Remove a session from the manager. If the child is still running
    /// it is killed synchronously on drop. Returns the final buffer
    /// snapshot for the caller to persist, log, or display.
    #[must_use]
    pub fn remove(&mut self, id: &SessionId) -> Option<Vec<u8>> {
        self.sessions
            .remove(id)
            .map(|entry| entry.buffer.snapshot())
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

    /// Read the raw buffer bytes whose monotonic byte offset is `>= cursor`.
    /// The offset domain is "total bytes ever written to this buffer" —
    /// equivalently `buffer.dropped() + buffer.len()` is the next cursor
    /// a caller would pass after this call.
    ///
    /// Returns `(next_cursor, bytes)`. If `cursor` sits before the ring's
    /// oldest retained byte (i.e. output was evicted past it), clamps to
    /// the ring start and returns whatever is still available — callers
    /// must accept possible byte loss under heavy output. A `cursor` past
    /// the current end returns an empty `Vec` and an unchanged cursor.
    /// Returns `None` if `id` is not tracked.
    #[must_use]
    pub fn buffer_read_since(&self, id: &SessionId, cursor: u64) -> Option<(u64, Vec<u8>)> {
        // BL-062: deliberately does NOT bump `last_accessed`. The
        // WI-12 drainer thread polls this on every active session
        // every few ms; bumping here would make every session look
        // active to LRU and pin it forever. The user-facing
        // `read_raw_since` IPC path already drives a `drain()` call
        // before this lookup, and `drain` does bump.
        let entry = self.sessions.get(id)?;
        let buf = &entry.buffer;
        let dropped = buf.dropped();
        let len = buf.len() as u64;
        let next_cursor = dropped.saturating_add(len);
        // Cursor past the end: no new bytes, echo it back unchanged.
        if cursor >= next_cursor {
            return Some((next_cursor, Vec::new()));
        }
        // Clamp a stale cursor to the oldest retained byte — the caller
        // asked for bytes that have since been evicted.
        let start_offset = if cursor < dropped {
            0usize
        } else {
            usize::try_from(cursor - dropped).unwrap_or(usize::MAX)
        };
        let (head, tail) = buf.slices();
        let total = head.len() + tail.len();
        let mut out = Vec::with_capacity(total.saturating_sub(start_offset));
        if start_offset < head.len() {
            out.extend_from_slice(&head[start_offset..]);
            out.extend_from_slice(tail);
        } else {
            let tail_off = start_offset - head.len();
            if tail_off < tail.len() {
                out.extend_from_slice(&tail[tail_off..]);
            }
        }
        Some((next_cursor, out))
    }

    /// Current lifecycle state of the session (PRD-09 §4). `None` if
    /// the id is not tracked. Reads the latched state on the underlying
    /// `Session` — cheap and does not poll the child. Callers that need
    /// up-to-date terminal states should call [`Self::reap_exited`]
    /// first.
    #[must_use]
    pub fn state(&self, id: &SessionId) -> Option<ProcessState> {
        self.sessions.get(id).map(|e| e.session.state())
    }

    /// Timestamp of the last read/write/resize/kill for `id`, or `None`
    /// if the session is unknown. Used by the future LRU eviction pass.
    #[must_use]
    pub fn last_accessed(&self, id: &SessionId) -> Option<Instant> {
        self.sessions.get(id).map(|e| e.last_accessed.get())
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
    fn vt_grid_tracks_screen_and_osc_133_exit() {
        if !unix_only("vt_grid_tracks_screen_and_osc_133_exit") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 4096);
        // A shell that emits an OSC 133 command-output region with text then a
        // finished mark with exit 5.
        let cfg = SessionConfig {
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "printf '\\033]133;C\\007grid-line\\n\\033]133;D;5\\007'".into(),
                ],
            }),
            ..SessionConfig::default()
        };
        let id = m.spawn(cfg).expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let _ = m.drain(&id, Duration::from_millis(200)).expect("drain");
            if m.vt_last_exit(&id).and_then(|(e, _)| e) == Some(5) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "OSC 133 exit not captured; screen={:?}",
                m.vt_screen(&id),
            );
        }
        // The screen models the command output; last-exit carries the code + text.
        assert!(m.vt_screen(&id).unwrap_or_default().contains("grid-line"));
        let (exit, output) = m.vt_last_exit(&id).expect("last exit");
        assert_eq!(exit, Some(5));
        assert!(output.unwrap_or_default().contains("grid-line"));
        // Unknown id reads as None — the handler maps this to NotRunning.
        assert!(m.vt_screen(&SessionId::from_string("nope")).is_none());
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
    fn evict_lru_returns_oldest_session_with_snapshot() {
        if !unix_only("evict_lru_returns_oldest_session_with_snapshot") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 1024);
        let a = m.spawn(sh_printf("alpha-mark")).expect("spawn a");
        // Advance the monotonic clock so `b.last_accessed > a.last_accessed`
        // without depending on insertion order.
        std::thread::sleep(Duration::from_millis(10));
        let b = m.spawn(sh_printf("beta-mark")).expect("spawn b");

        // Touch `a` so `b` becomes the LRU.
        std::thread::sleep(Duration::from_millis(10));
        let _ = m.drain(&a, Duration::from_millis(50));

        let (victim_id, _snap) = m.evict_lru().expect("evict");
        assert_eq!(victim_id, b);
        // `a` survives; `b` is gone.
        assert!(m.ids().contains(&a));
        assert!(!m.ids().contains(&b));
    }

    #[test]
    fn evict_lru_on_empty_manager_returns_none() {
        let mut m = SessionManager::new();
        assert!(m.evict_lru().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn spawn_or_evict_returns_no_eviction_when_under_cap() {
        if !unix_only("spawn_or_evict_returns_no_eviction_when_under_cap") {
            return;
        }
        let mut m = SessionManager::with_limits(4, 1024);
        let (_id, evicted) = m.spawn_or_evict(sh_printf("a")).expect("spawn");
        assert!(evicted.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn spawn_or_evict_evicts_lru_when_at_cap() {
        if !unix_only("spawn_or_evict_evicts_lru_when_at_cap") {
            return;
        }
        let mut m = SessionManager::with_limits(2, 1024);
        let first = m.spawn(sh_printf("first")).expect("spawn first");
        std::thread::sleep(Duration::from_millis(10));
        let _second = m.spawn(sh_printf("second")).expect("spawn second");
        std::thread::sleep(Duration::from_millis(10));
        let (new_id, evicted) = m
            .spawn_or_evict(sh_printf("third"))
            .expect("spawn with eviction");
        let (ev_id, _snap) = evicted.expect("should have evicted");
        assert_eq!(ev_id, first, "oldest session should be the eviction victim");
        assert!(m.ids().contains(&new_id));
        assert_eq!(m.len(), 2);
    }

    /// BL-062 — when the manager is at its cap and *every* session
    /// is still running, `spawn_or_evict` surfaces the underlying
    /// `spawn` cap error rather than silently killing a live session.
    /// The DoD's "preserve current SessionLimitExceeded behaviour"
    /// invariant lives here.
    #[cfg(unix)]
    #[test]
    fn spawn_or_evict_refuses_to_kill_running_sessions_at_cap() {
        if !unix_only("spawn_or_evict_refuses_to_kill_running_sessions_at_cap") {
            return;
        }
        let mut m = SessionManager::with_limits(2, 1024);
        // Long-running children — their state stays Running through
        // the cap check.
        let _a = m
            .spawn(SessionConfig {
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 5".into()],
                }),
                ..SessionConfig::default()
            })
            .expect("spawn a");
        let _b = m
            .spawn(SessionConfig {
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 5".into()],
                }),
                ..SessionConfig::default()
            })
            .expect("spawn b");
        let err = m
            .spawn_or_evict(SessionConfig {
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 5".into()],
                }),
                ..SessionConfig::default()
            })
            .expect_err("should refuse without a stopped session to evict");
        match err {
            TerminalError::ShellDetection { reason } => {
                assert!(reason.contains("session cap reached"), "got: {reason}");
            }
            other => panic!("expected ShellDetection, got {other:?}"),
        }
        assert_eq!(m.len(), 2, "no live session should have been killed");
    }

    /// BL-062 — `lines_snapshot` is one of the read accessors the DoD
    /// wants to count as access; verify the timestamp moves.
    #[cfg(unix)]
    #[test]
    fn lines_snapshot_bumps_last_accessed() {
        if !unix_only("lines_snapshot_bumps_last_accessed") {
            return;
        }
        let mut m = SessionManager::with_limits(2, 1024);
        let id = m.spawn(sh_printf("hi")).expect("spawn");
        let before = m.last_accessed(&id).expect("present");
        std::thread::sleep(Duration::from_millis(15));
        let _ = m.lines_snapshot(&id, None, None).expect("snapshot");
        let after = m.last_accessed(&id).expect("present");
        assert!(after > before, "lines_snapshot should bump last_accessed");
    }

    #[cfg(unix)]
    #[test]
    fn state_passthrough_reflects_session_lifecycle() {
        if !unix_only("state_passthrough_reflects_session_lifecycle") {
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
        assert_eq!(m.state(&id), Some(ProcessState::Running));
        m.kill(&id).expect("kill");
        match m.state(&id) {
            Some(ProcessState::Killed {
                signal: Signal::Kill,
                ..
            }) => {}
            other => panic!("expected Killed(Kill), got {other:?}"),
        }
        // Unknown id returns None.
        let ghost = SessionId::from_string("nope");
        assert_eq!(m.state(&ghost), None);
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
