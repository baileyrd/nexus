//! Move 3 — Snapshots as transactional substrate.
//!
//! The propose → review → commit flow adds an explicit gate between
//! the model's reasoning and any action that mutates external state:
//!
//! ```text
//! Reasoning → ProposalStore::submit(action, token)
//!                  ↓ capability gate check
//!             Approved ──────────────── Rejected
//!                  ↓ worker executes
//!             ProposalStore::commit(entries)
//!                  ↓
//!             Snapshot (reversible record)
//!                  ↓ (optional)
//!             ProposalStore::rollback(snapshot_id)
//! ```
//!
//! [`ProposalStore::submit`] is the single gate point: it checks the
//! session's [`CapabilityToken`] against the action's required
//! capability and immediately transitions to [`ProposalState::Approved`]
//! or [`ProposalState::Rejected`]. No approved proposal is ever
//! re-checked — the approval is a capability-gate receipt.
//!
//! [`ProposalStore::commit`] packages the worker's side-effect
//! records into a [`Snapshot`] that can later be reversed via
//! [`ProposalStore::rollback`]. Phase 1 ships the type lattice and
//! store; the IPC handlers that expose proposals/snapshots to callers
//! (and the session worker that drives commit) land in later phases.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use nexus_plugin_api::{token::CapabilityToken, Capability, CapabilityError};

// ─── Identifiers ─────────────────────────────────────────────────────────────

/// Opaque identifier for a [`Proposal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProposalId(Uuid);

impl ProposalId {
    /// Allocate a fresh random proposal id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ProposalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProposalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Opaque identifier for a committed [`Snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(Uuid);

impl SnapshotId {
    /// Allocate a fresh random snapshot id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for SnapshotId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ─── Proposed action ─────────────────────────────────────────────────────────

/// The concrete action the model wants to take.
///
/// Each variant maps to exactly one [`Capability`] via
/// [`ProposedAction::capability_required`] so the gate can evaluate
/// the proposal without inspecting the action's payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposedAction {
    /// Write `content` to `path` within the forge root.
    FsWrite {
        /// Path relative to the forge root.
        path: String,
        /// Content to write.
        content: String,
    },
    /// Write `content` to `path` outside the forge root (HIGH risk).
    FsWriteExternal {
        /// Absolute path outside the forge root.
        path: String,
        /// Content to write.
        content: String,
    },
    /// Delete `path` within the forge root.
    FsDelete {
        /// Path relative to the forge root.
        path: String,
    },
    /// Call `command` on `target_plugin` with `args`.
    IpcCall {
        /// Reverse-DNS plugin id of the target.
        target_plugin: String,
        /// Command to invoke on the target plugin.
        command: String,
        /// Arguments to pass to the command.
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Write `value` to the session's KV store at `key`.
    KvWrite {
        /// KV key.
        key: String,
        /// Value to store.
        value: serde_json::Value,
    },
    /// Spawn a child process with the given `argv` (HIGH risk).
    ProcessSpawn {
        /// Argument vector; `argv[0]` is the executable.
        argv: Vec<String>,
    },
}

impl ProposedAction {
    /// The single capability the gate checks for this action.
    /// [`ProposalStore::submit`] calls this to determine which
    /// capability to check against the session's [`CapabilityToken`].
    #[must_use]
    pub fn capability_required(&self) -> Capability {
        match self {
            Self::FsWrite { .. } => Capability::FsWrite,
            Self::FsWriteExternal { .. } => Capability::FsWriteExternal,
            Self::FsDelete { .. } => Capability::FsWrite,
            Self::IpcCall { .. } => Capability::IpcCall,
            Self::KvWrite { .. } => Capability::KvWrite,
            Self::ProcessSpawn { .. } => Capability::ProcessSpawn,
        }
    }
}

// ─── Proposal lifecycle ───────────────────────────────────────────────────────

/// Lifecycle state of a [`Proposal`].
///
/// Transitions are one-way and driven by [`ProposalStore`]:
/// `Pending → Approved → Committed { snapshot_id }` on the happy
/// path, or `Pending → Rejected { reason }` when the capability gate
/// denies the action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProposalState {
    /// Submitted; awaiting the capability gate (Phase 1 resolves
    /// synchronously in [`ProposalStore::submit`]).
    Pending,
    /// Gate check passed; the worker may execute the action.
    Approved,
    /// Gate check failed; the action will not execute. The session
    /// worker should surface `reason` to the model so it can replan.
    Rejected {
        /// Human-readable explanation (capability name + error text).
        reason: String,
    },
    /// Action executed; side-effects are captured in the linked snapshot.
    Committed {
        /// Snapshot that records the reversible side-effects.
        snapshot_id: SnapshotId,
    },
}

/// One model-proposed action tracked through the gate and into the
/// snapshot layer.
///
/// A proposal's lifecycle ends when it reaches [`ProposalState::Rejected`]
/// or [`ProposalState::Committed`]. Approved proposals that have not
/// yet been committed are "in-flight" — the worker has permission but
/// has not yet executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique identifier for this proposal.
    pub id: ProposalId,
    /// Session that produced this proposal.
    pub session_id: Uuid,
    /// The action the model wants to take.
    pub action: ProposedAction,
    /// Current lifecycle state.
    pub state: ProposalState,
    /// When the proposal was submitted to the gate.
    pub proposed_at: DateTime<Utc>,
    /// When the gate made its decision (Approved/Rejected) or when
    /// the action was committed. `None` while `Pending`.
    pub resolved_at: Option<DateTime<Utc>>,
}

impl Proposal {
    /// `true` if the proposal has reached a terminal state — either
    /// `Rejected` (will never execute) or `Committed` (already
    /// executed). `Approved` is not terminal: the worker still needs
    /// to execute and commit.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            ProposalState::Rejected { .. } | ProposalState::Committed { .. }
        )
    }
}

// ─── Snapshot ─────────────────────────────────────────────────────────────────

/// One reversible change recorded at commit time. The `old_*` fields
/// carry the pre-action state so a [`ProposalStore::rollback`] can
/// restore them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotEntry {
    /// A file write (create or overwrite).
    FsWrite {
        /// Path relative to the forge root.
        path: String,
        /// Pre-action content; `None` when the file did not previously
        /// exist (rollback deletes it in that case).
        old_content: Option<String>,
        /// Post-action content written to the file.
        new_content: String,
    },
    /// A file deletion.
    FsDelete {
        /// Path relative to the forge root.
        path: String,
        /// Content of the file before deletion (rollback recreates it).
        old_content: String,
    },
    /// A KV store write.
    KvWrite {
        /// KV key.
        key: String,
        /// Pre-action value; `None` when the key was absent (rollback
        /// deletes the key in that case).
        old_value: Option<serde_json::Value>,
        /// Post-action value stored at the key.
        new_value: serde_json::Value,
    },
}

/// An immutable record of one committed agent action.
///
/// Groups the [`SnapshotEntry`]s produced by a single [`Proposal`]
/// execution. Rolling back replays the entries in *reverse* order so
/// each change is undone in the inverse of the order it was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique identifier.
    pub id: SnapshotId,
    /// Session that produced this snapshot.
    pub session_id: Uuid,
    /// Proposal that was executed to produce these entries.
    pub proposal_id: ProposalId,
    /// Reversible change records in execution order. Rollback replays
    /// these in reverse.
    pub entries: Vec<SnapshotEntry>,
    /// When the snapshot was committed.
    pub committed_at: DateTime<Utc>,
    /// When it was rolled back; `None` while active.
    pub rolled_back_at: Option<DateTime<Utc>>,
}

impl Snapshot {
    /// `true` if this snapshot has been rolled back.
    #[must_use]
    pub fn is_rolled_back(&self) -> bool {
        self.rolled_back_at.is_some()
    }
}

// ─── ProposalStore ───────────────────────────────────────────────────────────

/// Thread-safe store for in-flight proposals and committed snapshots.
///
/// The store is the capability gate's decision log — every model
/// proposal passes through it before the worker executes it. It is
/// also the rollback ledger: committed snapshots are retained so the
/// session can undo actions when the user requests it.
///
/// `Clone`-able like [`crate::scheduler::Store`] — workers and IPC
/// handlers share the same `Arc`-backed maps.
#[derive(Clone, Debug)]
pub struct ProposalStore {
    proposals: Arc<Mutex<HashMap<ProposalId, Proposal>>>,
    snapshots: Arc<Mutex<HashMap<SnapshotId, Snapshot>>>,
}

impl ProposalStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            proposals: Arc::new(Mutex::new(HashMap::new())),
            snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Submit a proposed action through the capability gate.
    ///
    /// Immediately checks `token.check(action.capability_required())`
    /// and transitions the proposal to `Approved` or `Rejected` — the
    /// Phase-1 gate is synchronous. The proposal is stored in both
    /// cases so the session can inspect the rejection reason and replan.
    ///
    /// # Errors
    /// Returns [`CapabilityError::Denied`] when the session's token
    /// does not grant the required capability (or is revoked). The
    /// proposal is stored as `Rejected` with a human-readable reason.
    pub fn submit(
        &self,
        session_id: Uuid,
        action: ProposedAction,
        token: &CapabilityToken,
    ) -> Result<ProposalId, CapabilityError> {
        let cap = action.capability_required();
        let gate_result = token.check(cap);
        let id = ProposalId::new();
        let now = Utc::now();
        let (state, resolved_at) = match &gate_result {
            Ok(()) => (ProposalState::Approved, Some(now)),
            Err(e) => (
                ProposalState::Rejected {
                    reason: format!("{e}"),
                },
                Some(now),
            ),
        };
        let proposal = Proposal {
            id,
            session_id,
            action,
            state,
            proposed_at: now,
            resolved_at,
        };
        self.proposals
            .lock()
            .expect("proposal store poisoned")
            .insert(id, proposal);
        gate_result.map(|()| id)
    }

    /// Mark an approved proposal as committed and record the snapshot
    /// of its side-effects. Returns the new [`SnapshotId`].
    ///
    /// # Errors
    /// Returns a string describing why the commit was rejected:
    /// - proposal id is unknown
    /// - proposal is not in `Approved` state
    pub fn commit(
        &self,
        proposal_id: ProposalId,
        entries: Vec<SnapshotEntry>,
    ) -> Result<SnapshotId, String> {
        let snapshot_id = SnapshotId::new();
        let session_id;
        {
            let mut g = self.proposals.lock().expect("proposal store poisoned");
            let proposal = g
                .get_mut(&proposal_id)
                .ok_or_else(|| format!("proposal {proposal_id} not found"))?;
            if !matches!(proposal.state, ProposalState::Approved) {
                return Err(format!(
                    "proposal {proposal_id} is not Approved (current state: {:?})",
                    proposal.state
                ));
            }
            session_id = proposal.session_id;
            proposal.state = ProposalState::Committed { snapshot_id };
            proposal.resolved_at = Some(Utc::now());
        }
        let snapshot = Snapshot {
            id: snapshot_id,
            session_id,
            proposal_id,
            entries,
            committed_at: Utc::now(),
            rolled_back_at: None,
        };
        self.snapshots
            .lock()
            .expect("snapshot store poisoned")
            .insert(snapshot_id, snapshot);
        Ok(snapshot_id)
    }

    /// Mark a committed snapshot as rolled back and return a clone of
    /// it. The caller is responsible for actually reversing the
    /// side-effects (by replaying `SnapshotEntry::old_*` fields in
    /// reverse order) **before** calling this method so the ledger
    /// stays consistent with the real world.
    ///
    /// # Errors
    /// Returns a string describing why the rollback was rejected:
    /// - snapshot id is unknown
    /// - snapshot is already rolled back
    pub fn rollback(&self, snapshot_id: SnapshotId) -> Result<Snapshot, String> {
        let mut g = self.snapshots.lock().expect("snapshot store poisoned");
        let snap = g
            .get_mut(&snapshot_id)
            .ok_or_else(|| format!("snapshot {snapshot_id} not found"))?;
        if snap.is_rolled_back() {
            return Err(format!("snapshot {snapshot_id} is already rolled back"));
        }
        snap.rolled_back_at = Some(Utc::now());
        Ok(snap.clone())
    }

    /// Fetch a proposal by id.
    #[must_use]
    pub fn get_proposal(&self, id: ProposalId) -> Option<Proposal> {
        self.proposals
            .lock()
            .expect("proposal store poisoned")
            .get(&id)
            .cloned()
    }

    /// Fetch a snapshot by id.
    #[must_use]
    pub fn get_snapshot(&self, id: SnapshotId) -> Option<Snapshot> {
        self.snapshots
            .lock()
            .expect("snapshot store poisoned")
            .get(&id)
            .cloned()
    }

    /// All proposals for a given session. Order is undefined.
    #[must_use]
    pub fn proposals_for_session(&self, session_id: Uuid) -> Vec<Proposal> {
        self.proposals
            .lock()
            .expect("proposal store poisoned")
            .values()
            .filter(|p| p.session_id == session_id)
            .cloned()
            .collect()
    }

    /// All snapshots for a given session, sorted by `committed_at`
    /// ascending (oldest first — rollback replays in reverse).
    #[must_use]
    pub fn snapshots_for_session(&self, session_id: Uuid) -> Vec<Snapshot> {
        let mut snaps: Vec<Snapshot> = self
            .snapshots
            .lock()
            .expect("snapshot store poisoned")
            .values()
            .filter(|s| s.session_id == session_id)
            .cloned()
            .collect();
        snaps.sort_by_key(|s| s.committed_at);
        snaps
    }

    /// Count of live (non-terminal) proposals across all sessions.
    /// Used by the Supervisor's observability surface.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.proposals
            .lock()
            .expect("proposal store poisoned")
            .values()
            .filter(|p| matches!(p.state, ProposalState::Pending | ProposalState::Approved))
            .count()
    }
}

impl Default for ProposalStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_plugin_api::CapabilitySet;

    fn session_id() -> Uuid {
        Uuid::new_v4()
    }

    fn token_with(caps: impl IntoIterator<Item = nexus_plugin_api::Capability>) -> CapabilityToken {
        CapabilityToken::new(session_id(), CapabilitySet::from_iter(caps))
    }

    fn fs_write_action() -> ProposedAction {
        ProposedAction::FsWrite {
            path: "notes/test.md".into(),
            content: "hello".into(),
        }
    }

    #[test]
    fn submit_approved_when_token_grants_capability() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        let result = store.submit(sid, fs_write_action(), &token);
        assert!(result.is_ok(), "expected Approved, got: {result:?}");
        let pid = result.unwrap();
        let proposal = store.get_proposal(pid).expect("proposal stored");
        assert!(matches!(proposal.state, ProposalState::Approved));
        assert!(proposal.resolved_at.is_some());
    }

    #[test]
    fn submit_rejected_when_token_lacks_capability() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(sid, CapabilitySet::default()); // no caps
        let result = store.submit(sid, fs_write_action(), &token);
        assert!(result.is_err(), "expected Rejected");
        // Proposal still stored even on rejection.
        let all = store.proposals_for_session(sid);
        assert_eq!(all.len(), 1);
        assert!(matches!(all[0].state, ProposalState::Rejected { .. }));
    }

    #[test]
    fn submit_rejected_when_token_is_revoked() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        token.revoke();
        let result = store.submit(sid, fs_write_action(), &token);
        assert!(result.is_err(), "revoked token must not approve");
    }

    #[test]
    fn commit_approved_proposal_returns_snapshot_id() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        let pid = store.submit(sid, fs_write_action(), &token).unwrap();
        let entries = vec![SnapshotEntry::FsWrite {
            path: "notes/test.md".into(),
            old_content: None,
            new_content: "hello".into(),
        }];
        let snap_id = store.commit(pid, entries).expect("commit should succeed");
        let snap = store.get_snapshot(snap_id).expect("snapshot stored");
        assert_eq!(snap.proposal_id, pid);
        assert_eq!(snap.session_id, sid);
        assert_eq!(snap.entries.len(), 1);
        assert!(!snap.is_rolled_back());
        // Proposal state transitions to Committed.
        let proposal = store.get_proposal(pid).unwrap();
        assert!(matches!(proposal.state, ProposalState::Committed { .. }));
    }

    #[test]
    fn commit_pending_proposal_errors() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(sid, CapabilitySet::default());
        // Submit returns Err (rejected), but the proposal is stored.
        let _ = store.submit(sid, fs_write_action(), &token);
        let proposals = store.proposals_for_session(sid);
        assert_eq!(proposals.len(), 1);
        let pid = proposals[0].id;
        // Committing a Rejected proposal must fail.
        let err = store.commit(pid, vec![]).unwrap_err();
        assert!(err.contains("not Approved"), "actual: {err}");
    }

    #[test]
    fn rollback_marks_snapshot_and_returns_clone() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        let pid = store.submit(sid, fs_write_action(), &token).unwrap();
        let snap_id = store.commit(pid, vec![]).unwrap();
        let rolled = store.rollback(snap_id).expect("rollback should succeed");
        assert!(rolled.is_rolled_back());
        assert!(store.get_snapshot(snap_id).unwrap().is_rolled_back());
    }

    #[test]
    fn rollback_is_not_idempotent_errors_on_second_call() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        let pid = store.submit(sid, fs_write_action(), &token).unwrap();
        let snap_id = store.commit(pid, vec![]).unwrap();
        store.rollback(snap_id).unwrap();
        let err = store.rollback(snap_id).unwrap_err();
        assert!(err.contains("already rolled back"), "actual: {err}");
    }

    #[test]
    fn rollback_unknown_snapshot_errors() {
        let store = ProposalStore::new();
        let err = store.rollback(SnapshotId::new()).unwrap_err();
        assert!(err.contains("not found"), "actual: {err}");
    }

    #[test]
    fn snapshots_for_session_sorted_by_committed_at() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        // Commit two proposals in order.
        let pid1 = store.submit(sid, fs_write_action(), &token).unwrap();
        let snap1 = store.commit(pid1, vec![]).unwrap();
        let pid2 = store.submit(sid, fs_write_action(), &token).unwrap();
        let snap2 = store.commit(pid2, vec![]).unwrap();
        let snaps = store.snapshots_for_session(sid);
        assert_eq!(snaps.len(), 2);
        assert!(snaps[0].committed_at <= snaps[1].committed_at);
        assert!(snaps.iter().any(|s| s.id == snap1));
        assert!(snaps.iter().any(|s| s.id == snap2));
    }

    #[test]
    fn proposals_for_session_isolation() {
        let store = ProposalStore::new();
        let sid_a = session_id();
        let sid_b = session_id();
        let token_a = CapabilityToken::new(
            sid_a,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        let _ = store.submit(sid_a, fs_write_action(), &token_a);
        let _ = store.submit(sid_a, fs_write_action(), &token_a);
        let token_b = CapabilityToken::new(sid_b, CapabilitySet::default());
        let _ = store.submit(sid_b, fs_write_action(), &token_b);
        assert_eq!(store.proposals_for_session(sid_a).len(), 2);
        assert_eq!(store.proposals_for_session(sid_b).len(), 1);
    }

    #[test]
    fn pending_count_tracks_non_terminal_proposals() {
        let store = ProposalStore::new();
        let sid = session_id();
        let token = CapabilityToken::new(
            sid,
            CapabilitySet::from_iter([nexus_plugin_api::Capability::FsWrite]),
        );
        assert_eq!(store.pending_count(), 0);
        let pid1 = store.submit(sid, fs_write_action(), &token).unwrap();
        // Approved counts as live.
        assert_eq!(store.pending_count(), 1);
        // Commit: now Committed (not live).
        store.commit(pid1, vec![]).unwrap();
        assert_eq!(store.pending_count(), 0);
    }

    #[test]
    fn proposed_action_capability_required_is_stable() {
        assert_eq!(
            ProposedAction::FsWrite {
                path: "x".into(),
                content: "y".into()
            }
            .capability_required(),
            nexus_plugin_api::Capability::FsWrite,
        );
        assert_eq!(
            ProposedAction::FsWriteExternal {
                path: "/tmp/x".into(),
                content: "y".into()
            }
            .capability_required(),
            nexus_plugin_api::Capability::FsWriteExternal,
        );
        assert_eq!(
            ProposedAction::FsDelete { path: "x".into() }.capability_required(),
            nexus_plugin_api::Capability::FsWrite,
        );
        assert_eq!(
            ProposedAction::IpcCall {
                target_plugin: "com.nexus.ai".into(),
                command: "ask".into(),
                args: serde_json::Value::Null
            }
            .capability_required(),
            nexus_plugin_api::Capability::IpcCall,
        );
        assert_eq!(
            ProposedAction::KvWrite {
                key: "k".into(),
                value: serde_json::Value::Null
            }
            .capability_required(),
            nexus_plugin_api::Capability::KvWrite,
        );
        assert_eq!(
            ProposedAction::ProcessSpawn {
                argv: vec!["ls".into()]
            }
            .capability_required(),
            nexus_plugin_api::Capability::ProcessSpawn,
        );
    }
}
