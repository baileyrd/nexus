//! Programmable Terminal API — PRD-09 §11.
//!
//! # Role
//!
//! The [`TerminalServer`] trait is the surface plugins and AI agents
//! will call against. It wraps session lifecycle, input/output, search,
//! and an event stream into one coherent object so a core plugin can
//! expose `com.nexus.terminal.{spawn,send,read,search,subscribe}` over
//! dispatch without having to thread `SessionManager` + `LineBuffer` +
//! signal state separately.
//!
//! # Microkernel fit
//!
//! This module is a plain library — the trait and the default impl
//! have zero coupling to the kernel bus. A future
//! `com.nexus.terminal` core plugin holds a
//! `Mutex<InMemoryTerminalServer>` and forwards each dispatch method
//! to the matching trait call; plugin IPC sees nothing of the
//! PTY-internal types.
//!
//! # Concurrency shape
//!
//! The library stays sync + runtime-agnostic — every trait method
//! takes `&mut self` or `&self`, returns immediately, and leaves
//! scheduling to the caller. Async wrappers (tokio, smol, the
//! plugin host) are free to spawn background tasks around
//! [`TerminalServer::pump`] without this module importing a runtime.
//!
//! # Why `pump` is explicit
//!
//! Unlike the PRD §11.1 sketch where `read_output` implicitly drains,
//! we split the "move bytes from the PTY into the buffer" step into
//! [`TerminalServer::pump`]. This keeps read-only methods `&self` and
//! lets the core plugin own the cadence of PTY reads — matching the
//! sync shape of [`crate::SessionManager::drain`] without forcing a
//! hidden I/O into every snapshot call.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::TerminalError;
use crate::lines::Line;
use crate::manager::SessionManager;
use crate::session::{SessionConfig, SessionId};
use crate::shell::ShellSpec;

/// Minimum per-pump budget the [`TerminalServer::wait_for_pattern`]
/// implementation spends polling the PTY between pattern checks. Short
/// enough that a pattern that matches a single output flush is caught
/// within tens of milliseconds, long enough that the caller isn't
/// burning CPU spinning.
const WAIT_PUMP_INTERVAL: Duration = Duration::from_millis(25);

/// Input for [`TerminalServer::create_session`]. Thin wrapper so the
/// trait doesn't leak [`SessionConfig`]'s `portable_pty::PtySize` type
/// through plugin boundaries — a future IPC bridge can serialise this
/// without needing to know about `portable-pty`'s struct layout.
#[derive(Debug, Clone, Default)]
pub struct ServerSpawnConfig {
    /// Optional user-facing label. Falls back to the session id on
    /// display when absent.
    pub name: Option<String>,
    /// Explicit shell override (priority 1 in PRD-09 §1.2).
    pub shell: Option<ShellSpec>,
    /// Working directory to spawn in. `None` inherits the parent cwd.
    pub working_dir: Option<PathBuf>,
    /// Extra env vars layered on top of the inherited environment.
    /// Secret-masking happens at the UI layer; this field carries the
    /// resolved values.
    pub env: Vec<(String, String)>,
}

impl ServerSpawnConfig {
    /// Named-only builder for the common "spawn a default shell and
    /// label it" case.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
    }
}

/// PRD-09 §11.1 structured output line. Bridges [`Line`]'s
/// `SystemTime` + `Vec<u8>` into serde-friendly Unix-ms + `String`
/// fields the plugin IPC boundary can carry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputLine {
    /// Milliseconds since Unix epoch at first ingestion.
    pub timestamp_ms: u64,
    /// ANSI-stripped text content (no trailing newline).
    pub content: String,
    /// Raw bytes as received — includes ANSI sequences.
    pub raw: Vec<u8>,
    /// Adjacent-repeat counter (1 for a distinct line, >1 after
    /// spinner/progress-bar collapse). See [`Line::repeats`].
    pub repeats: u32,
}

impl From<&Line> for OutputLine {
    fn from(l: &Line) -> Self {
        let ms = l
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|d| {
                u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
            })
            .unwrap_or(0);
        Self {
            timestamp_ms: ms,
            content: l.text_only.clone(),
            raw: l.raw.clone(),
            repeats: l.repeats,
        }
    }
}

/// PRD-09 §11.1 session metadata surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    /// Opaque session identifier.
    pub id: String,
    /// Human-readable label; falls back to `id` when unset.
    pub name: String,
    /// Shell path the PTY is running.
    pub shell: String,
    /// Working directory the shell was spawned in, if known.
    pub working_dir: Option<String>,
    /// Current line count in the buffer.
    pub line_count: usize,
    /// Unix-seconds creation timestamp.
    pub created_at: u64,
}

/// PRD-09 §11.2 events published to subscribers. Variants cover the
/// minimum the AI / UI layer needs to react to session lifecycle and
/// output flow; richer ProcessEvents (memory, crash reason) belong to
/// the §7 / §4 layers and land as they come online.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalEvent {
    /// A new session was created via [`TerminalServer::create_session`].
    SessionCreated {
        /// Session id.
        id: String,
        /// Human-readable label; `None` if unset.
        name: Option<String>,
    },
    /// One line of output arrived in the session's line buffer.
    OutputReceived {
        /// Session id.
        id: String,
        /// The line itself.
        line: OutputLine,
    },
    /// A pattern previously registered via
    /// [`TerminalServer::wait_for_pattern`] matched on a line. The
    /// server emits this before returning from `wait_for_pattern`.
    PatternMatched {
        /// Session id.
        id: String,
        /// The pattern (string or regex source) that matched.
        pattern: String,
        /// Index of the matching line within the buffer at match time.
        line_index: usize,
    },
    /// Session's PTY child exited (naturally or via signal). The
    /// session entry is still in the manager — callers can still read
    /// the final buffer — but no further input will reach it.
    SessionClosed {
        /// Session id.
        id: String,
        /// Exit code if known.
        exit_code: Option<u32>,
    },
}

/// PRD-09 §11.1 programmable terminal surface.
pub trait TerminalServer {
    /// Spawn a new session with the given config. Emits
    /// [`TerminalEvent::SessionCreated`] to every live subscriber.
    ///
    /// # Errors
    /// Propagates [`TerminalError`] from PTY allocation / spawn /
    /// session-cap enforcement.
    fn create_session(&mut self, cfg: ServerSpawnConfig) -> Result<SessionId, TerminalError>;

    /// Gracefully shut the session down via the §5.1 signal ladder,
    /// emitting [`TerminalEvent::SessionClosed`] with the finishing
    /// exit code. The session entry stays in the manager so the
    /// caller can still read the final buffer.
    ///
    /// # Errors
    /// Propagates [`TerminalError::NotRunning`] for unknown ids plus
    /// any I/O error from the shutdown ladder.
    fn close_session(&mut self, id: &SessionId) -> Result<(), TerminalError>;

    /// Write `input` to the session's child stdin, appending a
    /// trailing newline to behave like a user pressing Enter.
    ///
    /// # Errors
    /// Propagates [`TerminalError::NotRunning`] / [`TerminalError::Io`].
    fn send_input(&mut self, id: &SessionId, input: &str) -> Result<(), TerminalError>;

    /// Write raw bytes to the session's child stdin — no newline added.
    /// Use for control sequences (`\x03` for Ctrl-C, arrow keys, …).
    ///
    /// # Errors
    /// Propagates [`TerminalError::NotRunning`] / [`TerminalError::Io`].
    fn send_raw_input(&mut self, id: &SessionId, data: &[u8]) -> Result<(), TerminalError>;

    /// Drain whatever the PTY has produced since the last call into the
    /// session's line + byte buffers, then emit
    /// [`TerminalEvent::OutputReceived`] once per new line. Blocks up to
    /// `timeout` for the first byte. Returns the number of bytes read.
    ///
    /// # Errors
    /// Propagates [`TerminalError::NotRunning`] / [`TerminalError::Io`].
    fn pump(&mut self, id: &SessionId, timeout: Duration) -> Result<usize, TerminalError>;

    /// Read a window of structured lines from the session's buffer.
    /// `start` / `count` behave like Python slicing (clamped to the
    /// available range). Omitted values default to "whole buffer".
    ///
    /// # Errors
    /// [`TerminalError::NotRunning`] if the id is unknown.
    fn read_output(
        &self,
        id: &SessionId,
        start: Option<usize>,
        count: Option<usize>,
    ) -> Result<Vec<OutputLine>, TerminalError>;

    /// Search the line buffer for lines matching `query`. `is_regex`
    /// toggles regex-lite vs literal substring. Returns line indices
    /// into the current buffer (unstable across eviction).
    ///
    /// # Errors
    /// [`TerminalError::NotRunning`] if the id is unknown, or a
    /// [`TerminalError::Persist`]-flavoured carrier for regex parse
    /// failures so the caller can surface the reason.
    fn search_output(
        &self,
        id: &SessionId,
        query: &str,
        is_regex: bool,
    ) -> Result<Vec<usize>, TerminalError>;

    /// Register a fresh subscriber and return the receiving end. The
    /// returned [`Receiver`] drops the subscription when dropped; the
    /// server cleans up dead senders on the next emit pass.
    fn subscribe_events(&mut self) -> Receiver<TerminalEvent>;

    /// Pump the session repeatedly until a line matching `pattern`
    /// arrives or `timeout` elapses. `is_regex` toggles regex vs
    /// substring. On match, emits [`TerminalEvent::PatternMatched`]
    /// before returning `Ok(true)`. On timeout returns `Ok(false)`.
    ///
    /// # Errors
    /// Propagates I/O errors from [`Self::pump`].
    fn wait_for_pattern(
        &mut self,
        id: &SessionId,
        pattern: &str,
        is_regex: bool,
        timeout: Duration,
    ) -> Result<bool, TerminalError>;

    /// Look up a session's metadata surface.
    ///
    /// # Errors
    /// [`TerminalError::NotRunning`] if the id is unknown.
    fn get_session_info(&self, id: &SessionId) -> Result<SessionInfo, TerminalError>;

    /// List every session the server knows about. Order is arbitrary;
    /// sort by `created_at` on the caller side if needed.
    fn list_sessions(&self) -> Vec<SessionInfo>;
}

/// Default [`TerminalServer`] implementation: wraps a
/// [`SessionManager`], holds a list of subscribers, and tracks per-
/// session "lines emitted so far" so [`Self::pump`] only fires
/// `OutputReceived` for genuinely new content.
pub struct InMemoryTerminalServer {
    manager: SessionManager,
    subscribers: Vec<Sender<TerminalEvent>>,
    /// For each session, how many lines we have already broadcast
    /// `OutputReceived` for. On pump, the delta between this counter
    /// and the live line-buffer length is what we emit. Tracked per-
    /// server (not per-subscriber) so late subscribers don't replay
    /// the full history — the PRD §11.2 stream is "events from now
    /// on", not a catch-up log.
    emitted_lines: HashMap<SessionId, usize>,
}

impl InMemoryTerminalServer {
    /// Build a server around a fresh, default-limits [`SessionManager`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_manager(SessionManager::new())
    }

    /// Build a server around a pre-configured [`SessionManager`]. Use
    /// this when tests need a tighter session cap.
    #[must_use]
    pub fn with_manager(manager: SessionManager) -> Self {
        Self {
            manager,
            subscribers: Vec::new(),
            emitted_lines: HashMap::new(),
        }
    }

    /// Read access to the underlying manager — useful for LRU /
    /// persistence drivers that want to drive eviction directly.
    #[must_use]
    pub fn manager(&self) -> &SessionManager {
        &self.manager
    }

    /// Mutable access to the underlying manager. Same escape hatch as
    /// [`Self::manager`] for drivers that need to call methods that
    /// aren't surfaced on [`TerminalServer`].
    pub fn manager_mut(&mut self) -> &mut SessionManager {
        &mut self.manager
    }

    /// Broadcast `event` to every live subscriber, pruning senders
    /// whose receivers have dropped.
    fn emit(&mut self, event: TerminalEvent) {
        // retain keeps only subscribers whose send succeeds. A
        // `SendError` means the receiver was dropped — safe to reap.
        self.subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }

    fn session_info(&self, id: &SessionId) -> Result<SessionInfo, TerminalError> {
        let created_at = self
            .manager
            .created_at(id)
            .ok_or_else(|| TerminalError::NotRunning(id.as_str().to_string()))?;
        let line_count = self.manager.line_count(id).unwrap_or(0);
        let name = self
            .manager
            .name(id)
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| id.as_str().to_string());
        // `SessionManager` doesn't expose the shell string for an
        // existing session yet — future work; pass through an empty
        // string so the field stays honest rather than being guessed.
        Ok(SessionInfo {
            id: id.as_str().to_string(),
            name,
            shell: String::new(),
            working_dir: None,
            line_count,
            created_at,
        })
    }

}

impl Default for InMemoryTerminalServer {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalServer for InMemoryTerminalServer {
    fn create_session(&mut self, cfg: ServerSpawnConfig) -> Result<SessionId, TerminalError> {
        let session_cfg = SessionConfig {
            shell: cfg.shell,
            working_dir: cfg.working_dir,
            initial_size: None,
            env: cfg.env,
        };
        let id = self.manager.spawn(session_cfg)?;
        if let Some(ref name) = cfg.name {
            // Ignore the unknown-id error path — the id was just minted.
            let _ = self.manager.set_name(&id, name.clone());
        }
        self.emitted_lines.insert(id.clone(), 0);
        self.emit(TerminalEvent::SessionCreated {
            id: id.as_str().to_string(),
            name: cfg.name,
        });
        Ok(id)
    }

    fn close_session(&mut self, id: &SessionId) -> Result<(), TerminalError> {
        let finisher = self
            .manager
            .request_shutdown(id, Duration::from_millis(500))?;
        // Try to reap for an exit code so SessionClosed carries it.
        let exit_code = self
            .manager
            .reap_exited()
            .into_iter()
            .find(|(eid, _)| eid == id)
            .map(|(_, c)| c);
        tracing::debug!(
            session_id = id.as_str(),
            signal = finisher.name(),
            "server closed session",
        );
        self.emit(TerminalEvent::SessionClosed {
            id: id.as_str().to_string(),
            exit_code,
        });
        Ok(())
    }

    fn send_input(&mut self, id: &SessionId, input: &str) -> Result<(), TerminalError> {
        let mut bytes = input.as_bytes().to_vec();
        if !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        self.manager.write(id, &bytes)
    }

    fn send_raw_input(&mut self, id: &SessionId, data: &[u8]) -> Result<(), TerminalError> {
        self.manager.write(id, data)
    }

    fn pump(&mut self, id: &SessionId, timeout: Duration) -> Result<usize, TerminalError> {
        let bytes = self.manager.drain(id, timeout)?;
        let after = self.manager.line_count(id).unwrap_or(0);
        let already = self.emitted_lines.get(id).copied().unwrap_or(0);
        // If the LineBuffer wrapped between pumps, `after` can be
        // less than `already`; treat that as "nothing new" and resync
        // the counter so the next growth emits correctly.
        if after <= already {
            self.emitted_lines.insert(id.clone(), after);
            return Ok(bytes);
        }
        let new_lines = after - already;
        let slice = self
            .manager
            .lines_snapshot(id, Some(already), Some(new_lines))
            .unwrap_or_default();
        for line in slice {
            self.emit(TerminalEvent::OutputReceived {
                id: id.as_str().to_string(),
                line: OutputLine::from(&line),
            });
        }
        self.emitted_lines.insert(id.clone(), after);
        Ok(bytes)
    }

    fn read_output(
        &self,
        id: &SessionId,
        start: Option<usize>,
        count: Option<usize>,
    ) -> Result<Vec<OutputLine>, TerminalError> {
        let snap = self
            .manager
            .lines_snapshot(id, start, count)
            .ok_or_else(|| TerminalError::NotRunning(id.as_str().to_string()))?;
        Ok(snap.iter().map(OutputLine::from).collect())
    }

    fn search_output(
        &self,
        id: &SessionId,
        query: &str,
        is_regex: bool,
    ) -> Result<Vec<usize>, TerminalError> {
        // `None` from `lines_search` covers two cases: the id is
        // unknown, or (for regex mode) the pattern failed to compile.
        // Distinguish them by checking the id first — keeps error
        // messages precise for the caller.
        if self.manager.line_count(id).is_none() {
            return Err(TerminalError::NotRunning(id.as_str().to_string()));
        }
        self.manager
            .lines_search(id, query, is_regex)
            .ok_or_else(|| TerminalError::Persist(format!("invalid regex: {query}")))
    }

    fn subscribe_events(&mut self) -> Receiver<TerminalEvent> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.push(tx);
        rx
    }

    fn wait_for_pattern(
        &mut self,
        id: &SessionId,
        pattern: &str,
        is_regex: bool,
        timeout: Duration,
    ) -> Result<bool, TerminalError> {
        let deadline = Instant::now() + timeout;
        loop {
            // Scan what's already in the buffer first — a fast-path
            // caller may have pumped before calling us.
            if let Some(indices) = self.manager.lines_search(id, pattern, is_regex) {
                if let Some(idx) = indices.first().copied() {
                    self.emit(TerminalEvent::PatternMatched {
                        id: id.as_str().to_string(),
                        pattern: pattern.to_string(),
                        line_index: idx,
                    });
                    return Ok(true);
                }
            } else if is_regex {
                // Regex failed to compile — surface it; literal mode
                // never returns `None` for a known session.
                return Err(TerminalError::Persist(format!("invalid regex: {pattern}")));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            let remaining = deadline - now;
            let step = remaining.min(WAIT_PUMP_INTERVAL);
            self.pump(id, step)?;
        }
    }

    fn get_session_info(&self, id: &SessionId) -> Result<SessionInfo, TerminalError> {
        self.session_info(id)
    }

    fn list_sessions(&self) -> Vec<SessionInfo> {
        self.manager
            .ids()
            .into_iter()
            .filter_map(|id| self.session_info(&id).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellSpec;

    fn unix_only(name: &str) -> bool {
        if !cfg!(unix) {
            eprintln!("skipping {name}: unix-only");
            return false;
        }
        true
    }

    fn sh_printf(marker: &str) -> ServerSpawnConfig {
        ServerSpawnConfig {
            name: Some("test".into()),
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("printf '{marker}\\n'")],
            }),
            working_dir: None,
            env: vec![],
        }
    }

    fn sh_echo_lines(lines: &[&str]) -> ServerSpawnConfig {
        let script = lines
            .iter()
            .map(|l| format!("printf '{l}\\n'"))
            .collect::<Vec<_>>()
            .join("; ");
        ServerSpawnConfig {
            name: Some("echo".into()),
            shell: Some(ShellSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), script],
            }),
            working_dir: None,
            env: vec![],
        }
    }

    #[test]
    fn output_line_conversion_preserves_fields() {
        let line = Line {
            timestamp: UNIX_EPOCH + Duration::from_millis(123_456),
            raw: b"hello\n".to_vec(),
            text_only: "hello".into(),
            repeats: 1,
        };
        let out: OutputLine = (&line).into();
        assert_eq!(out.timestamp_ms, 123_456);
        assert_eq!(out.content, "hello");
        assert_eq!(out.raw, b"hello\n");
        assert_eq!(out.repeats, 1);
    }

    #[test]
    fn create_session_emits_event_and_registers_name() {
        if !unix_only("create_session_emits_event_and_registers_name") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let rx = s.subscribe_events();
        let id = s.create_session(sh_printf("x")).expect("create");
        let evt = rx.recv_timeout(Duration::from_secs(1)).expect("got event");
        match evt {
            TerminalEvent::SessionCreated { id: eid, name } => {
                assert_eq!(eid, id.as_str());
                assert_eq!(name.as_deref(), Some("test"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        let info = s.get_session_info(&id).expect("info");
        assert_eq!(info.name, "test");
    }

    #[test]
    fn pump_emits_output_received_for_each_new_line() {
        if !unix_only("pump_emits_output_received_for_each_new_line") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let rx = s.subscribe_events();
        let id = s
            .create_session(sh_echo_lines(&["alpha", "beta", "gamma"]))
            .expect("create");
        // Drain the creation event so we can focus on output events.
        let _ = rx.recv_timeout(Duration::from_millis(500));

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut seen = Vec::<String>::new();
        while Instant::now() < deadline && seen.len() < 3 {
            let _ = s.pump(&id, Duration::from_millis(100));
            while let Ok(evt) = rx.try_recv() {
                if let TerminalEvent::OutputReceived { line, .. } = evt {
                    seen.push(line.content);
                }
            }
        }
        assert!(seen.iter().any(|s| s == "alpha"), "missing alpha: {seen:?}");
        assert!(seen.iter().any(|s| s == "beta"), "missing beta: {seen:?}");
        assert!(seen.iter().any(|s| s == "gamma"), "missing gamma: {seen:?}");
    }

    #[test]
    fn wait_for_pattern_returns_false_on_timeout_for_silent_session() {
        if !unix_only("wait_for_pattern_returns_false_on_timeout_for_silent_session") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        // `sleep 2` produces no output within our 200 ms wait window.
        let id = s
            .create_session(ServerSpawnConfig {
                name: Some("silent".into()),
                shell: Some(ShellSpec {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 2".into()],
                }),
                working_dir: None,
                env: vec![],
            })
            .expect("create");
        let hit = s
            .wait_for_pattern(&id, "never-appears", false, Duration::from_millis(200))
            .expect("wait");
        assert!(!hit, "expected timeout, got match");
    }

    #[test]
    fn wait_for_pattern_matches_substring_and_emits_event() {
        if !unix_only("wait_for_pattern_matches_substring_and_emits_event") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let rx = s.subscribe_events();
        let id = s
            .create_session(sh_echo_lines(&["warmup", "ready-signal", "tail"]))
            .expect("create");
        // Drop the SessionCreated event.
        let _ = rx.recv_timeout(Duration::from_millis(500));

        let hit = s
            .wait_for_pattern(&id, "ready-signal", false, Duration::from_secs(3))
            .expect("wait");
        assert!(hit, "should have found the signal");

        // A PatternMatched event should be among the broadcast events.
        let mut saw_match = false;
        while let Ok(evt) = rx.try_recv() {
            if let TerminalEvent::PatternMatched { pattern, .. } = evt {
                if pattern == "ready-signal" {
                    saw_match = true;
                }
            }
        }
        assert!(saw_match, "expected PatternMatched event");
    }

    #[test]
    fn read_output_returns_line_window() {
        if !unix_only("read_output_returns_line_window") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let id = s
            .create_session(sh_echo_lines(&["a", "b", "c", "d"]))
            .expect("create");
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let _ = s.pump(&id, Duration::from_millis(100));
            if s.manager().line_count(&id).unwrap_or(0) >= 4 {
                break;
            }
        }
        let all = s.read_output(&id, None, None).expect("all");
        let contents: Vec<String> = all.iter().map(|l| l.content.clone()).collect();
        for marker in ["a", "b", "c", "d"] {
            assert!(contents.iter().any(|s| s == marker), "missing {marker}: {contents:?}");
        }

        let first_two = s.read_output(&id, Some(0), Some(2)).expect("window");
        assert_eq!(first_two.len(), 2);
        assert_eq!(first_two[0].content, contents[0]);
    }

    #[test]
    fn search_output_literal_and_regex() {
        if !unix_only("search_output_literal_and_regex") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let id = s
            .create_session(sh_echo_lines(&["error: boom", "ok", "error: kablam"]))
            .expect("create");
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let _ = s.pump(&id, Duration::from_millis(100));
            if s.manager().line_count(&id).unwrap_or(0) >= 3 {
                break;
            }
        }
        let literal = s.search_output(&id, "error:", false).expect("literal");
        assert_eq!(literal.len(), 2);
        let regex = s.search_output(&id, r"^error:\s+\w+", true).expect("regex");
        assert_eq!(regex.len(), 2);
        let bad = s.search_output(&id, "(", true);
        assert!(matches!(bad, Err(TerminalError::Persist(_))));
    }

    #[test]
    fn list_sessions_covers_every_spawned_session() {
        if !unix_only("list_sessions_covers_every_spawned_session") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        let a = s.create_session(sh_printf("a")).expect("create a");
        let b = s.create_session(sh_printf("b")).expect("create b");
        let list = s.list_sessions();
        let ids: Vec<String> = list.iter().map(|i| i.id.clone()).collect();
        assert!(ids.contains(&a.as_str().to_string()));
        assert!(ids.contains(&b.as_str().to_string()));
    }

    #[test]
    fn unknown_id_surfaces_not_running_from_public_methods() {
        let s = InMemoryTerminalServer::new();
        let ghost = SessionId::from_string("ghost");
        assert!(matches!(
            s.read_output(&ghost, None, None),
            Err(TerminalError::NotRunning(_)),
        ));
        assert!(matches!(
            s.search_output(&ghost, "x", false),
            Err(TerminalError::NotRunning(_)),
        ));
        assert!(matches!(
            s.get_session_info(&ghost),
            Err(TerminalError::NotRunning(_)),
        ));
    }

    #[test]
    fn subscriber_cleanup_removes_dropped_receivers_on_next_emit() {
        if !unix_only("subscriber_cleanup_removes_dropped_receivers_on_next_emit") {
            return;
        }
        let mut s = InMemoryTerminalServer::new();
        {
            let _rx = s.subscribe_events();
            // rx drops here — next emit should reap the dead sender.
        }
        let _id = s.create_session(sh_printf("x")).expect("create");
        // After emit, the subscribers list should be empty. We can't
        // inspect it directly; prove it indirectly by subscribing again
        // and observing we still get events (i.e. the emit path didn't
        // panic on the stale sender).
        let rx = s.subscribe_events();
        let _ = s.create_session(sh_printf("y")).expect("create 2");
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());
    }
}
