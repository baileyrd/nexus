//! Per-process memory monitoring (PRD-09 §7).
//!
//! # Role
//!
//! Reads RSS for a live process (Unix: `/proc/<pid>/status`; Windows:
//! `GetProcessMemoryInfo`), caches a short rolling window of samples
//! per pid, and evaluates configured soft / hard limits against the
//! latest sample. The PRD's §4.3 auto-kill-on-memory-exceeded path and
//! the §14 yellow-badge indicator both read from here.
//!
//! # Microkernel fit
//!
//! Plain library. The polling cadence is the caller's job (matches
//! §7.2's "1 s per active process" suggestion but isn't enforced
//! here) — this module provides one-shot reads and a cheap
//! [`MemoryMonitor`] buffer the core plugin / UI schedules around.
//!
//! # What this is NOT
//!
//! - A killer. Crossing the hard limit produces
//!   [`MemoryLimitAction::HardExceeded`]; the caller decides whether
//!   to send [`crate::Signal::Kill`] via [`crate::SessionManager`].
//!   Keeping the policy / mechanism split means tests don't need a
//!   process that's actually consuming memory.
//! - A per-session feature. Memory is tracked by raw pid so callers
//!   that spawn subprocesses outside the PTY (background jobs, CI
//!   runners) can reuse the same monitor.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::TerminalError;

/// Default rolling-window length — PRD §7.2 "60-second rolling window
/// of memory samples" at the default 1 Hz polling cadence.
pub const DEFAULT_HISTORY_SAMPLES: usize = 60;

/// One RSS sample. `bytes` is the resident-set size of the process at
/// `observed_at` (monotonic clock). Serde-compatible so the event bus
/// can forward these to AI subscribers (§12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySample {
    /// Process identifier the sample was taken for.
    pub pid: u32,
    /// Resident-set size in bytes at observation time.
    pub bytes: u64,
    /// Milliseconds since this monitor's first sample. Monotonic —
    /// tests pin it by constructing samples manually.
    pub elapsed_ms: u64,
}

/// Per-process soft/hard thresholds (MB). `None` disables that tier.
/// PRD §7.3 defaults are 250 / 500 MB; callers override per command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLimits {
    /// Warning threshold in MB. Crossing it produces
    /// [`MemoryLimitAction::SoftExceeded`] — UI turns the badge yellow;
    /// the process keeps running.
    pub soft_mb: Option<u32>,
    /// Kill threshold in MB. Crossing it produces
    /// [`MemoryLimitAction::HardExceeded`]; the caller kills the
    /// session and transitions the FSM to Crashed.
    pub hard_mb: Option<u32>,
}

impl MemoryLimits {
    /// PRD-09 §7.3 defaults (250 MB soft, 500 MB hard).
    #[must_use]
    pub const fn default_recommended() -> Self {
        Self {
            soft_mb: Some(250),
            hard_mb: Some(500),
        }
    }

    /// No thresholds — every sample is `Ok`.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            soft_mb: None,
            hard_mb: None,
        }
    }

    /// Evaluate a single `bytes` reading against the limits. Hard
    /// beats soft — a process that blew through both in one jump
    /// triggers the kill path, not a warning.
    #[must_use]
    pub fn evaluate(&self, bytes: u64) -> MemoryLimitAction {
        let mb = bytes / 1_048_576;
        if let Some(hard) = self.hard_mb {
            if mb >= u64::from(hard) {
                return MemoryLimitAction::HardExceeded {
                    bytes,
                    limit_mb: hard,
                };
            }
        }
        if let Some(soft) = self.soft_mb {
            if mb >= u64::from(soft) {
                return MemoryLimitAction::SoftExceeded {
                    bytes,
                    limit_mb: soft,
                };
            }
        }
        MemoryLimitAction::Ok { bytes }
    }
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self::default_recommended()
    }
}

/// What a single sample implies. Kept as an enum (not a pair of bools)
/// so downstream `match` statements exhaustively cover every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryLimitAction {
    /// Under both thresholds (or thresholds are disabled).
    Ok {
        /// Latest RSS reading.
        bytes: u64,
    },
    /// At or above soft, below hard. UI should warn; no process action.
    SoftExceeded {
        /// Latest RSS reading.
        bytes: u64,
        /// The soft threshold (MB) that was crossed.
        limit_mb: u32,
    },
    /// At or above hard. Caller kills the process.
    HardExceeded {
        /// Latest RSS reading.
        bytes: u64,
        /// The hard threshold (MB) that was crossed.
        limit_mb: u32,
    },
}

impl MemoryLimitAction {
    /// The RSS reading this action was computed from. Convenience for
    /// UI code that just wants the number.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        match *self {
            MemoryLimitAction::Ok { bytes }
            | MemoryLimitAction::SoftExceeded { bytes, .. }
            | MemoryLimitAction::HardExceeded { bytes, .. } => bytes,
        }
    }

    /// Whether the hard limit tripped — i.e. the caller should kill.
    #[must_use]
    pub const fn should_kill(&self) -> bool {
        matches!(self, MemoryLimitAction::HardExceeded { .. })
    }
}

/// Rolling-window memory monitor for a set of processes. One instance
/// typically lives per workspace; the core plugin schedules periodic
/// [`Self::sample`] calls from a background task.
pub struct MemoryMonitor {
    history_len: usize,
    started: Instant,
    /// Per-pid history + limits. `VecDeque` keeps push-back / pop-front
    /// O(1) at the small history sizes this uses.
    processes: HashMap<u32, ProcessEntry>,
}

struct ProcessEntry {
    samples: VecDeque<MemorySample>,
    limits: MemoryLimits,
}

impl MemoryMonitor {
    /// Build a monitor with the default 60-sample history.
    #[must_use]
    pub fn new() -> Self {
        Self::with_history(DEFAULT_HISTORY_SAMPLES)
    }

    /// Build with a custom history length. `0` disables retention — the
    /// monitor still evaluates limits but keeps no samples.
    #[must_use]
    pub fn with_history(history_len: usize) -> Self {
        Self {
            history_len,
            started: Instant::now(),
            processes: HashMap::new(),
        }
    }

    /// Register a process with the monitor. Replaces any existing
    /// entry for the same pid (safe: pid reuse after a crash is
    /// caught by the caller re-registering).
    pub fn track(&mut self, pid: u32, limits: MemoryLimits) {
        self.processes.insert(
            pid,
            ProcessEntry {
                samples: VecDeque::with_capacity(self.history_len.max(1)),
                limits,
            },
        );
    }

    /// Stop tracking a pid. Returns whether anything was removed — the
    /// caller rarely needs to check, but useful in tests.
    pub fn untrack(&mut self, pid: u32) -> bool {
        self.processes.remove(&pid).is_some()
    }

    /// Pids currently tracked. Arbitrary order.
    #[must_use]
    pub fn tracked_pids(&self) -> Vec<u32> {
        self.processes.keys().copied().collect()
    }

    /// Read the latest RSS for `pid` via the platform reader, append
    /// to history, and evaluate against the configured limits. The
    /// pid must have been [`Self::track`]ed first.
    ///
    /// # Errors
    /// - [`TerminalError::Persist`] carrying "unknown pid …" if the
    ///   caller sample()s an untracked pid.
    /// - [`TerminalError::Io`] from the platform RSS read failing
    ///   (process gone, permission denied).
    pub fn sample(&mut self, pid: u32) -> Result<MemoryLimitAction, TerminalError> {
        if !self.processes.contains_key(&pid) {
            return Err(TerminalError::Persist(format!(
                "memory monitor: unknown pid {pid}",
            )));
        }
        let bytes = read_process_rss(pid)?;
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.record_sample(pid, bytes, elapsed_ms)
    }

    /// Test-only entrypoint: inject a sample without touching the
    /// platform reader. Exposed so tests can exercise the history +
    /// limit evaluation paths without a real subprocess.
    ///
    /// # Errors
    /// Returns [`TerminalError::NotRunning`] if `pid` was never tracked.
    pub fn record_sample(
        &mut self,
        pid: u32,
        bytes: u64,
        elapsed_ms: u64,
    ) -> Result<MemoryLimitAction, TerminalError> {
        let entry = self
            .processes
            .get_mut(&pid)
            .ok_or_else(|| TerminalError::Persist(format!("unknown pid {pid}")))?;
        let sample = MemorySample {
            pid,
            bytes,
            elapsed_ms,
        };
        if self.history_len > 0 {
            if entry.samples.len() == self.history_len {
                entry.samples.pop_front();
            }
            entry.samples.push_back(sample);
        }
        Ok(entry.limits.evaluate(bytes))
    }

    /// Most-recent sample for `pid`, if any.
    #[must_use]
    pub fn latest(&self, pid: u32) -> Option<MemorySample> {
        self.processes
            .get(&pid)
            .and_then(|e| e.samples.back().copied())
    }

    /// Immutable slice of all retained samples, oldest first.
    #[must_use]
    pub fn history(&self, pid: u32) -> Vec<MemorySample> {
        self.processes
            .get(&pid)
            .map(|e| e.samples.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Replace the limit config for a tracked pid. Returns the previous
    /// value, or `None` if the pid wasn't tracked (nothing is
    /// installed in that case).
    pub fn set_limits(&mut self, pid: u32, limits: MemoryLimits) -> Option<MemoryLimits> {
        self.processes.get_mut(&pid).map(|e| {
            let prev = e.limits;
            e.limits = limits;
            prev
        })
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Read RSS (bytes) for `pid` from the current platform. Unix parses
/// `/proc/<pid>/status`; Windows calls `GetProcessMemoryInfo`.
///
/// # Errors
/// - [`TerminalError::Io`] if the process is gone, the `/proc` file is
///   unreadable, or the Win32 API fails.
pub fn read_process_rss(pid: u32) -> Result<u64, TerminalError> {
    platform::read_rss(pid)
}

/// Hint for PRD §7.2's polling cadence. Caller's scheduler matches
/// this if it wants PRD-default behaviour; it's a pure suggestion.
pub const RECOMMENDED_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(unix)]
mod platform {
    use std::io;

    use crate::error::TerminalError;

    pub(super) fn read_rss(pid: u32) -> Result<u64, TerminalError> {
        // /proc/<pid>/status is line-oriented with `Key: value` rows.
        // VmRSS is in kB (historically; documented in proc(5)).
        let path = format!("/proc/{pid}/status");
        let contents = std::fs::read_to_string(&path)?;
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                // Expected shape: "VmRSS:    12345 kB". Strip whitespace,
                // take the numeric prefix, multiply to bytes.
                let trimmed = rest.trim();
                let num_part: String = trimmed
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                if num_part.is_empty() {
                    return Err(TerminalError::Io(io::Error::other(format!(
                        "VmRSS parse failed: '{trimmed}'",
                    ))));
                }
                let kb: u64 = num_part.parse().map_err(|e| {
                    TerminalError::Io(io::Error::other(format!("VmRSS parse: {e}")))
                })?;
                return Ok(kb * 1024);
            }
        }
        Err(TerminalError::Io(io::Error::other(format!(
            "VmRSS not found in {path}",
        ))))
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::mem::MaybeUninit;

    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    use crate::error::TerminalError;

    pub(super) fn read_rss(pid: u32) -> Result<u64, TerminalError> {
        // PROCESS_QUERY_LIMITED_INFORMATION works across UAC boundaries
        // in a way that PROCESS_ALL_ACCESS does not — prefer it, fall
        // back to adding VM_READ which GetProcessMemoryInfo docs list.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                FALSE,
                pid,
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(TerminalError::Io(io::Error::last_os_error()));
        }
        let mut counters: MaybeUninit<PROCESS_MEMORY_COUNTERS> = MaybeUninit::uninit();
        let rc = unsafe {
            GetProcessMemoryInfo(
                handle,
                counters.as_mut_ptr(),
                u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>())
                    .unwrap_or(u32::MAX),
            )
        };
        let err = if rc == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        unsafe { CloseHandle(handle) };
        if let Some(e) = err {
            return Err(TerminalError::Io(e));
        }
        // SAFETY: GetProcessMemoryInfo succeeded, so `counters` is
        // initialised. WorkingSetSize is the Win32 analogue of RSS.
        let counters = unsafe { counters.assume_init() };
        Ok(counters.WorkingSetSize as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_default_recommended_matches_prd_numbers() {
        let l = MemoryLimits::default_recommended();
        assert_eq!(l.soft_mb, Some(250));
        assert_eq!(l.hard_mb, Some(500));
    }

    #[test]
    fn limits_evaluate_returns_ok_under_soft() {
        let l = MemoryLimits {
            soft_mb: Some(100),
            hard_mb: Some(200),
        };
        let a = l.evaluate(50 * 1_048_576);
        assert!(matches!(a, MemoryLimitAction::Ok { .. }));
        assert_eq!(a.bytes(), 50 * 1_048_576);
        assert!(!a.should_kill());
    }

    #[test]
    fn limits_evaluate_soft_fires_between_soft_and_hard() {
        let l = MemoryLimits {
            soft_mb: Some(100),
            hard_mb: Some(200),
        };
        let a = l.evaluate(150 * 1_048_576);
        match a {
            MemoryLimitAction::SoftExceeded { limit_mb, .. } => assert_eq!(limit_mb, 100),
            other => panic!("expected SoftExceeded, got {other:?}"),
        }
        assert!(!a.should_kill());
    }

    #[test]
    fn limits_evaluate_hard_beats_soft_when_both_crossed() {
        let l = MemoryLimits {
            soft_mb: Some(100),
            hard_mb: Some(200),
        };
        let a = l.evaluate(300 * 1_048_576);
        assert!(matches!(a, MemoryLimitAction::HardExceeded { limit_mb: 200, .. }));
        assert!(a.should_kill());
    }

    #[test]
    fn limits_unlimited_never_fires() {
        let l = MemoryLimits::unlimited();
        assert!(matches!(
            l.evaluate(10 * 1_073_741_824),
            MemoryLimitAction::Ok { .. },
        ));
    }

    #[test]
    fn monitor_track_and_untrack_roundtrip() {
        let mut m = MemoryMonitor::new();
        m.track(42, MemoryLimits::unlimited());
        assert_eq!(m.tracked_pids(), vec![42]);
        assert!(m.untrack(42));
        assert!(!m.untrack(42)); // idempotent
        assert!(m.tracked_pids().is_empty());
    }

    #[test]
    fn monitor_record_sample_returns_evaluated_action_and_stores_history() {
        let mut m = MemoryMonitor::with_history(3);
        m.track(
            1,
            MemoryLimits {
                soft_mb: Some(1),
                hard_mb: Some(10),
            },
        );
        // Under soft
        let a = m.record_sample(1, 500_000, 0).expect("sample 1");
        assert!(matches!(a, MemoryLimitAction::Ok { .. }));
        // Over soft
        let a = m.record_sample(1, 5 * 1_048_576, 100).expect("sample 2");
        assert!(matches!(a, MemoryLimitAction::SoftExceeded { .. }));
        // Over hard
        let a = m.record_sample(1, 20 * 1_048_576, 200).expect("sample 3");
        assert!(matches!(a, MemoryLimitAction::HardExceeded { .. }));
        assert!(a.should_kill());

        let hist = m.history(1);
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[0].elapsed_ms, 0);
        assert_eq!(hist[2].bytes, 20 * 1_048_576);
    }

    #[test]
    fn monitor_history_respects_cap_and_drops_oldest() {
        let mut m = MemoryMonitor::with_history(2);
        m.track(1, MemoryLimits::unlimited());
        m.record_sample(1, 10, 0).unwrap();
        m.record_sample(1, 20, 1).unwrap();
        m.record_sample(1, 30, 2).unwrap();
        let hist = m.history(1);
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].bytes, 20);
        assert_eq!(hist[1].bytes, 30);
    }

    #[test]
    fn monitor_zero_history_evaluates_but_stores_nothing() {
        let mut m = MemoryMonitor::with_history(0);
        m.track(
            1,
            MemoryLimits {
                soft_mb: Some(1),
                hard_mb: None,
            },
        );
        let a = m.record_sample(1, 5 * 1_048_576, 0).expect("sample");
        assert!(matches!(a, MemoryLimitAction::SoftExceeded { .. }));
        assert!(m.history(1).is_empty());
        assert!(m.latest(1).is_none());
    }

    #[test]
    fn monitor_untracked_pid_surfaces_error() {
        let mut m = MemoryMonitor::new();
        let err = m.record_sample(999, 0, 0).unwrap_err();
        assert!(matches!(err, TerminalError::Persist(_)));
    }

    #[test]
    fn monitor_set_limits_returns_previous_or_none_for_unknown_pid() {
        let mut m = MemoryMonitor::new();
        assert!(m.set_limits(1, MemoryLimits::unlimited()).is_none());
        m.track(1, MemoryLimits::default_recommended());
        let prev = m
            .set_limits(1, MemoryLimits::unlimited())
            .expect("previous");
        assert_eq!(prev, MemoryLimits::default_recommended());
    }

    #[cfg(unix)]
    #[test]
    fn read_process_rss_for_self_returns_plausible_nonzero_bytes() {
        let pid = std::process::id();
        let rss = read_process_rss(pid).expect("read self");
        // Any healthy process has at least a few MB of RSS; anchor
        // loosely to avoid being flaky.
        assert!(rss > 1_048_576, "unexpectedly small RSS: {rss}");
        assert!(rss < 100 * 1_073_741_824, "implausibly large RSS: {rss}");
    }

    #[cfg(unix)]
    #[test]
    fn read_process_rss_for_nonexistent_pid_returns_io_error() {
        // Pick a pid that cannot exist — pid 0 / pid 1 are real; use
        // a huge value past the kernel's pid_max default (4 194 303).
        let err = read_process_rss(u32::MAX - 1).unwrap_err();
        assert!(matches!(err, TerminalError::Io(_)));
    }

    #[cfg(unix)]
    #[test]
    fn monitor_sample_for_tracked_self_pid_records_a_sample() {
        let mut m = MemoryMonitor::new();
        let pid = std::process::id();
        m.track(pid, MemoryLimits::unlimited());
        let a = m.sample(pid).expect("sample self");
        assert!(matches!(a, MemoryLimitAction::Ok { .. }));
        let hist = m.history(pid);
        assert_eq!(hist.len(), 1);
        assert!(hist[0].bytes > 0);
    }
}
