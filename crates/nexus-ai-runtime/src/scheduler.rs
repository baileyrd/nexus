//! In-memory run store + state machine.
//!
//! The store is the single source of truth for live runs in the
//! Phase-1 runtime: every `submit` writes an [`AgentRun`] entry,
//! every typed [`crate::events::AiEvent`] from a worker (or from the
//! bus republisher) appends to the run's ring buffer and may advance
//! its [`crate::RunStatus`].
//!
//! Persistence to `<forge>/.forge/ai-runtime/runs.db` is not yet
//! wired — it lands later under "persistence across restart" in ADR
//! 0028 §Open follow-ups. The Phase-1 store is purely in-memory and
//! is dropped on plugin shutdown.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::events::AiEvent;
use crate::{
    AgentRun, AgentRunSummary, AiRuntimeListArgs, EventRing, RunStatus, SharedEventRing, TaskPriority,
};

/// One run's bookkeeping. Stored in the shared map; the per-run event
/// ring is kept behind its own `Arc` so worker tasks can record events
/// without holding the outer map lock.
pub(crate) struct RunRow {
    pub task_id: Uuid,
    pub kind_label: String,
    pub priority: TaskPriority,
    pub parent: Option<Uuid>,
    pub caller_plugin_id: String,
    pub status: RunStatus,
    pub submitted_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub events: SharedEventRing,
    /// Notifier fired once the run reaches a terminal status
    /// (Completed / Failed / Cancelled). `wait_for` callers grab
    /// this Arc and await `notified()`. Callers MUST check the
    /// status synchronously BEFORE constructing the `notified()`
    /// future to avoid the lost-wakeup race — the handler in
    /// `core_plugin::handle_wait_for` does this two-step (call
    /// `is_terminal` first, then build `notified()`, then re-check
    /// status before awaiting).
    pub terminal: Arc<Notify>,
    /// BL-134 Phase 5 — cooperative cancellation gate. The
    /// `handle_cancel` IPC handler flips the flag and notifies; the
    /// worker observes via `tokio::select!` and emits `Cancelled`
    /// before the inner ipc_call's result, suppressing the
    /// underlying `Finished`/`Failed` event the call would otherwise
    /// produce. The token is checked once before the select arm
    /// (cancel-before-spawn race) and once inside the arm (cancel-
    /// during-execution).
    pub cancel: Arc<CancelGate>,
}

/// BL-134 Phase 5 — atomic cancel flag paired with a Notify so the
/// worker can park on `notified()` and the handler can fire-and-
/// forget. The flag is what distinguishes a genuine cancellation
/// from a spurious wakeup; the Notify is the wake mechanism.
#[derive(Debug)]
pub(crate) struct CancelGate {
    flag: AtomicBool,
    /// Free-form reason captured at cancel time — surfaced in the
    /// emitted `AiEvent::Cancelled.by` field.
    reason: Mutex<Option<String>>,
    notify: Notify,
}

impl CancelGate {
    pub(crate) fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
            reason: Mutex::new(None),
            notify: Notify::new(),
        }
    }

    /// Signal cancellation. Returns `true` if this was the first
    /// signal; subsequent `request` calls are no-ops so idempotent
    /// `handle_cancel` IPC behaviour falls out for free.
    pub(crate) fn request(&self, reason: Option<String>) -> bool {
        if self.flag.swap(true, Ordering::SeqCst) {
            return false;
        }
        if let Ok(mut g) = self.reason.lock() {
            *g = reason;
        }
        self.notify.notify_waiters();
        true
    }

    /// `true` if cancel has been signalled. Cheap atomic load.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Reason captured at cancel time; cleared on read so the next
    /// observer sees `None`. Used by the worker once when emitting
    /// `Cancelled.by`.
    pub(crate) fn take_reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|mut g| g.take())
    }

    /// Build the future the worker awaits in its select arm.
    /// `notified()` parks until the next `notify_waiters()`. The
    /// caller is responsible for the pre-check / post-check pattern
    /// so that a cancel signalled before the future is constructed
    /// isn't lost.
    pub(crate) async fn cancelled(&self) {
        self.notify.notified().await;
    }
}

impl Default for CancelGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true for statuses that mean the worker has finished and
/// the run's outcome is no longer going to change.
pub(crate) fn is_terminal(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
    )
}

/// Thread-safe handle to the run store. Cloned around to give each
/// worker (and the bus republisher) the same backing map.
#[derive(Clone)]
pub(crate) struct Store {
    inner: Arc<Mutex<HashMap<Uuid, RunRow>>>,
}

impl Store {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a freshly-submitted task. Returns the per-run ring so
    /// the caller can wire it into the worker without a second lookup.
    pub(crate) fn insert(
        &self,
        task_id: Uuid,
        kind_label: &str,
        priority: TaskPriority,
        parent: Option<Uuid>,
        caller_plugin_id: &str,
    ) -> SharedEventRing {
        let ring: SharedEventRing = Arc::new(EventRing::new());
        let row = RunRow {
            task_id,
            kind_label: kind_label.to_string(),
            priority,
            parent,
            caller_plugin_id: caller_plugin_id.to_string(),
            status: RunStatus::Queued,
            submitted_at: Utc::now(),
            started_at: None,
            finished_at: None,
            events: Arc::clone(&ring),
            terminal: Arc::new(Notify::new()),
            cancel: Arc::new(CancelGate::new()),
        };
        let mut g = self.inner.lock().expect("store poisoned");
        g.insert(task_id, row);
        ring
    }

    /// Grab the cancel gate for a run. Used by the worker (which
    /// awaits `cancel.cancelled()` in its select arm) and by the
    /// `handle_cancel` IPC handler (which calls `gate.request(...)`).
    pub(crate) fn cancel_gate(&self, task_id: Uuid) -> Option<Arc<CancelGate>> {
        let g = self.inner.lock().expect("store poisoned");
        g.get(&task_id).map(|row| Arc::clone(&row.cancel))
    }

    /// Grab the terminal-state notifier for a run. Used by the
    /// `wait_for` IPC handler to await completion without busy-polling.
    pub(crate) fn terminal_notify(&self, task_id: Uuid) -> Option<Arc<Notify>> {
        let g = self.inner.lock().expect("store poisoned");
        g.get(&task_id).map(|row| Arc::clone(&row.terminal))
    }

    /// Check whether a run is already in a terminal status. Lets
    /// `wait_for` short-circuit before the `notified()` await arm in
    /// the (common) case where the caller polls late.
    pub(crate) fn is_terminal(&self, task_id: Uuid) -> Option<bool> {
        let g = self.inner.lock().expect("store poisoned");
        g.get(&task_id).map(|row| is_terminal(&row.status))
    }

    /// Borrow the per-run ring without taking the outer lock for the
    /// duration of an event recording. Returns `None` if the run has
    /// been evicted (Phase 1 never evicts, but this guard keeps the
    /// API truthful).
    pub(crate) fn ring_for(&self, task_id: Uuid) -> Option<SharedEventRing> {
        let g = self.inner.lock().expect("store poisoned");
        g.get(&task_id).map(|row| Arc::clone(&row.events))
    }

    /// Advance the run's status + timestamps if the event implies a
    /// transition. Returns `true` when the row was found. When the
    /// transition is terminal (Completed / Failed / Cancelled) the
    /// row's `terminal` notifier is signalled so any `wait_for`
    /// callers parked on `notified()` wake up.
    pub(crate) fn observe_status(&self, event: &AiEvent) -> bool {
        let task_id = event.task_id();
        let Some(transition) = event.implied_status() else {
            return self.inner.lock().expect("store poisoned").contains_key(&task_id);
        };
        let terminal_notifier = {
            let mut g = self.inner.lock().expect("store poisoned");
            let Some(row) = g.get_mut(&task_id) else {
                return false;
            };
            match transition {
                RunStatus::Running if row.started_at.is_none() => {
                    row.started_at = Some(Utc::now());
                }
                RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled => {
                    if row.finished_at.is_none() {
                        row.finished_at = Some(Utc::now());
                    }
                }
                _ => {}
            }
            row.status = transition.clone();
            // Only grab a clone of the Arc<Notify> if the new status is
            // terminal; non-terminal transitions don't need to wake
            // anyone. The outer-lock guard is dropped before we fire
            // the notify so concurrent `wait_for` calls don't deadlock
            // on the map lock during `notified().await` re-entry.
            is_terminal(&transition).then(|| Arc::clone(&row.terminal))
        };
        if let Some(notify) = terminal_notifier {
            notify.notify_waiters();
        }
        true
    }

    /// Snapshot a single run, including its event-ring contents.
    pub(crate) fn get(&self, task_id: Uuid) -> Option<AgentRun> {
        let g = self.inner.lock().expect("store poisoned");
        let row = g.get(&task_id)?;
        let events = row.events.snapshot();
        Some(AgentRun {
            task_id: row.task_id,
            kind: row.kind_label.clone(),
            priority: row.priority,
            parent: row.parent,
            caller_plugin_id: row.caller_plugin_id.clone(),
            status: row.status.clone(),
            submitted_at: row.submitted_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            events,
        })
    }

    /// Compact list with optional filters. The result is sorted by
    /// `submitted_at` descending so the most recent runs render at
    /// the top of the observability panel without per-call sorting.
    pub(crate) fn list(&self, args: &AiRuntimeListArgs) -> Vec<AgentRunSummary> {
        let g = self.inner.lock().expect("store poisoned");
        let mut rows: Vec<&RunRow> = g
            .values()
            .filter(|r| args.status.as_ref().is_none_or(|want| &r.status == want))
            .filter(|r| args.since.as_ref().is_none_or(|since| r.submitted_at >= *since))
            .collect();
        rows.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
        let limit = args.limit.map_or(usize::MAX, |n| n as usize);
        rows.into_iter()
            .take(limit)
            .map(|r| AgentRunSummary {
                task_id: r.task_id,
                kind: r.kind_label.clone(),
                priority: r.priority,
                caller_plugin_id: r.caller_plugin_id.clone(),
                status: r.status.clone(),
                submitted_at: r.submitted_at,
                finished_at: r.finished_at,
            })
            .collect()
    }

    /// Count runs in the requested status. Used by `pool_stats`.
    pub(crate) fn count_status(&self, status: &RunStatus) -> u32 {
        let g = self.inner.lock().expect("store poisoned");
        u32::try_from(g.values().filter(|r| &r.status == status).count()).unwrap_or(u32::MAX)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AiEvent;

    fn task_started(store: &Store, id: Uuid) {
        store.observe_status(&AiEvent::Started {
            task_id: id,
            attempt: 1,
        });
    }

    #[test]
    fn insert_then_get_round_trips() {
        let store = Store::new();
        let id = Uuid::new_v4();
        let _ring = store.insert(id, "session", TaskPriority::Interactive, None, "com.nexus.cli");
        let row = store.get(id).expect("row present");
        assert_eq!(row.task_id, id);
        assert_eq!(row.kind, "session");
        assert_eq!(row.status, RunStatus::Queued);
        assert!(row.started_at.is_none());
        assert!(row.finished_at.is_none());
    }

    #[test]
    fn observe_status_advances_started_then_completed() {
        let store = Store::new();
        let id = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        task_started(&store, id);
        let row = store.get(id).unwrap();
        assert_eq!(row.status, RunStatus::Running);
        assert!(row.started_at.is_some());

        store.observe_status(&AiEvent::Finished {
            task_id: id,
            outcome: serde_json::Value::Null,
        });
        let row = store.get(id).unwrap();
        assert_eq!(row.status, RunStatus::Completed);
        assert!(row.finished_at.is_some());
    }

    #[test]
    fn observe_status_marks_failed_on_failed_event() {
        let store = Store::new();
        let id = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        store.observe_status(&AiEvent::Failed {
            task_id: id,
            error: "boom".into(),
            retriable: false,
        });
        assert_eq!(store.get(id).unwrap().status, RunStatus::Failed);
    }

    #[test]
    fn list_filters_by_status_and_since_and_limits() {
        let store = Store::new();
        for i in 0..5 {
            let id = Uuid::new_v4();
            store.insert(id, "session", TaskPriority::Interactive, None, &format!("p{i}"));
            if i % 2 == 0 {
                store.observe_status(&AiEvent::Finished {
                    task_id: id,
                    outcome: serde_json::Value::Null,
                });
            }
        }
        let completed = store.list(&AiRuntimeListArgs {
            status: Some(RunStatus::Completed),
            limit: None,
            since: None,
        });
        assert_eq!(completed.len(), 3);
        let limited = store.list(&AiRuntimeListArgs {
            status: None,
            limit: Some(2),
            since: None,
        });
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn ring_for_returns_same_handle_as_insert() {
        let store = Store::new();
        let id = Uuid::new_v4();
        let returned = store.insert(id, "session", TaskPriority::Interactive, None, "x");
        let fetched = store.ring_for(id).unwrap();
        // Push through one handle, observe via the other — confirms
        // they share state.
        returned.push(AiEvent::TokenChunk {
            task_id: id,
            text: "hello".into(),
        });
        assert_eq!(fetched.snapshot().len(), 1);
    }

    #[test]
    fn terminal_notify_returns_arc_per_run() {
        let store = Store::new();
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        store.insert(other, "session", TaskPriority::Interactive, None, "x");
        let a = store.terminal_notify(id).expect("notify present");
        let b = store.terminal_notify(id).expect("notify present");
        assert!(Arc::ptr_eq(&a, &b), "same run must hand out the same Arc");
        let c = store.terminal_notify(other).expect("notify present");
        assert!(!Arc::ptr_eq(&a, &c), "different runs must have distinct Arcs");
        assert!(store.terminal_notify(Uuid::new_v4()).is_none());
    }

    #[test]
    fn is_terminal_reflects_status_transitions() {
        let store = Store::new();
        let id = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        assert_eq!(store.is_terminal(id), Some(false));
        store.observe_status(&AiEvent::Started { task_id: id, attempt: 1 });
        assert_eq!(store.is_terminal(id), Some(false));
        store.observe_status(&AiEvent::Finished {
            task_id: id,
            outcome: serde_json::Value::Null,
        });
        assert_eq!(store.is_terminal(id), Some(true));
        assert_eq!(store.is_terminal(Uuid::new_v4()), None);
    }

    #[tokio::test]
    async fn observe_status_terminal_transition_wakes_waiters() {
        let store = Store::new();
        let id = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        let notify = store.terminal_notify(id).unwrap();
        let notified = notify.notified();
        // Drive the transition from another task — `notify_waiters`
        // must wake the parked future even though it was constructed
        // before the transition fired.
        let store_clone = store.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            store_clone.observe_status(&AiEvent::Finished {
                task_id: id,
                outcome: serde_json::Value::Null,
            });
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), notified)
            .await
            .expect("notified must fire");
    }

    #[test]
    fn count_status_counts_only_matching_rows() {
        let store = Store::new();
        for _ in 0..3 {
            let id = Uuid::new_v4();
            store.insert(id, "session", TaskPriority::Interactive, None, "x");
        }
        let id = Uuid::new_v4();
        store.insert(id, "session", TaskPriority::Interactive, None, "x");
        store.observe_status(&AiEvent::Started { task_id: id, attempt: 1 });
        assert_eq!(store.count_status(&RunStatus::Queued), 3);
        assert_eq!(store.count_status(&RunStatus::Running), 1);
    }
}
