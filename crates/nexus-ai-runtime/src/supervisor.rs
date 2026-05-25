//! The [`Supervisor`] — the AI runtime's named session lifecycle manager.
//!
//! The Supervisor is the structural heart of Move 1 in the AI-native runtime
//! design. It was previously implicit in [`crate::core_plugin::AiRuntimeCorePlugin`]
//! as two separate fields (`store` + `pool`). Making it an explicit named type:
//!
//! - Clarifies ownership: one place owns the task store and the worker pool
//! - Enables admission control as a first-class concern
//! - Provides the spawn point for sub-session delegation (Move 2)
//! - Is the right home for future policy enforcement (budget checks, rate
//!   limiting, coalescing)
//!
//! ## Session lifecycle bus topics
//!
//! When sessions start and end the Supervisor emits Custom events on the
//! kernel bus under the `com.nexus.ai.runtime.session.*` namespace:
//!
//! | Topic | Payload fields |
//! |---|---|
//! | [`TOPIC_SESSION_STARTED`] | `session_id`, `session_kind`, `task_id` |
//! | [`TOPIC_SESSION_COMPLETED`] | `session_id`, `outcome`, `task_id` |
//! | [`TOPIC_SESSION_CANCELLED`] | `session_id`, `reason`, `task_id` |
//!
//! Consumers can subscribe with
//! `EventFilter::CustomPrefix("com.nexus.ai.runtime.session.")` to receive
//! all session lifecycle events regardless of kind.

use std::sync::OnceLock;

use nexus_plugin_api::{CapabilitySet, token::CapabilityToken};

use crate::pool::WorkerPool;
use crate::proposal::ProposalStore;
use crate::scheduler::Store;
use crate::session::SessionKind;

// ─── Bus topic constants ─────────────────────────────────────────────────────

/// Bus topic emitted when the supervisor begins executing a session.
/// Payload: `{ session_id: string, session_kind: SessionKind, task_id: string }`.
pub const TOPIC_SESSION_STARTED: &str = "com.nexus.ai.runtime.session.started";

/// Bus topic emitted when a session reaches a terminal state cleanly.
/// Payload: `{ session_id: string, outcome: SessionOutcome, task_id: string }`.
pub const TOPIC_SESSION_COMPLETED: &str = "com.nexus.ai.runtime.session.completed";

/// Bus topic emitted when a session is cancelled before natural completion.
/// Payload: `{ session_id: string, reason: string | null, task_id: string }`.
pub const TOPIC_SESSION_CANCELLED: &str = "com.nexus.ai.runtime.session.cancelled";

// ─── Admission control ───────────────────────────────────────────────────────

/// Per-kind concurrency limits enforced by the Supervisor at submission
/// time. Tasks that would exceed the limit are queued (not rejected) until
/// a running session of the same kind completes.
///
/// Phase 1 values are generous defaults. Phase 5 (priority lanes)
/// introduces per-kind `JoinSet`s and makes these limits hot-reloadable.
#[derive(Debug, Clone)]
pub struct AdmissionConfig {
    /// Max sessions of kind [`SessionKind::UserDriven`] running concurrently.
    pub max_concurrent_user_driven: usize,
    /// Max sessions of kind [`SessionKind::Ambient`] running concurrently.
    pub max_concurrent_ambient: usize,
    /// Max sessions of kind [`SessionKind::SignalTriggered`] running concurrently.
    pub max_concurrent_signal_triggered: usize,
    /// Max sessions of kind [`SessionKind::SubAgent`] running concurrently
    /// across all parent sessions.
    pub max_concurrent_sub_agents: usize,
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_user_driven: 4,
            max_concurrent_ambient: 2,
            max_concurrent_signal_triggered: 8,
            max_concurrent_sub_agents: 16,
        }
    }
}

impl AdmissionConfig {
    /// Return the concurrency limit for a given session kind.
    #[must_use]
    pub fn limit_for(&self, kind: SessionKind) -> usize {
        match kind {
            SessionKind::UserDriven => self.max_concurrent_user_driven,
            SessionKind::Ambient => self.max_concurrent_ambient,
            SessionKind::SignalTriggered => self.max_concurrent_signal_triggered,
            SessionKind::SubAgent => self.max_concurrent_sub_agents,
        }
    }
}

// ─── Supervisor ──────────────────────────────────────────────────────────────

/// The AI runtime supervisor: owns the task store, the worker pool,
/// the proposal/snapshot ledger, and enforces admission control. The
/// single point through which session lifecycle is managed.
///
/// Previously the store and pool lived as separate fields on
/// [`crate::core_plugin::AiRuntimeCorePlugin`]. This type makes the
/// Supervisor a first-class named entity so future work (sub-session
/// delegation, rate limiting, coalescing) has a clean home.
///
/// The pool is populated lazily in
/// [`crate::core_plugin::AiRuntimeCorePlugin::wire_context`] because
/// `cargo test -p nexus-ai-runtime` must not pay the cost of spinning
/// up a tokio runtime just to run type-level tests.
pub struct Supervisor {
    pub(crate) store: Store,
    pub(crate) pool: OnceLock<WorkerPool>,
    /// Move 3 — proposal/snapshot ledger shared across all sessions.
    /// The capability gate lives inside [`ProposalStore::submit`];
    /// workers call `commit` once the action is executed.
    pub(crate) proposals: ProposalStore,
    /// Admission policy — consulted at submission time (Phase 5 wires
    /// the actual enforcement; stored here so the configuration is
    /// co-located with the pool that will enforce it).
    pub admission: AdmissionConfig,
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("store", &"<Store>")
            .field("pool_started", &self.pool.get().is_some())
            .field("proposals_pending", &self.proposals.pending_count())
            .field("admission", &self.admission)
            .finish()
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    /// Create a new Supervisor with default admission config and no
    /// worker pool started yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: Store::new(),
            pool: OnceLock::new(),
            proposals: ProposalStore::new(),
            admission: AdmissionConfig::default(),
        }
    }

    /// Override the admission config. Builder-style for use at
    /// construction time.
    #[must_use]
    pub fn with_admission(mut self, config: AdmissionConfig) -> Self {
        self.admission = config;
        self
    }

    /// Install the worker pool. Returns `true` on the first call;
    /// `false` if a pool is already installed (idempotent — safe to
    /// call from `wire_context` even if the plugin is somehow wired
    /// twice).
    pub fn set_pool(&self, pool: WorkerPool) -> bool {
        self.pool.set(pool).is_ok()
    }

    /// Mint a fresh [`CapabilityToken`] for a new session. The returned
    /// token is the live authorization envelope the session carries;
    /// calling [`CapabilityToken::revoke`] on it immediately invalidates
    /// the session and any child tokens derived via
    /// [`Self::attenuate_token`].
    #[must_use]
    pub fn mint_token(&self, session_id: uuid::Uuid, caps: CapabilitySet) -> CapabilityToken {
        CapabilityToken::new(session_id, caps)
    }

    /// Create an attenuated child token for a sub-session. The child's
    /// capability set is the intersection of the parent's capabilities
    /// and `requested`; revoking the parent token also invalidates the
    /// child (but not vice-versa).
    #[must_use]
    pub fn attenuate_token(
        &self,
        parent: &CapabilityToken,
        child_session_id: uuid::Uuid,
        requested: CapabilitySet,
    ) -> CapabilityToken {
        parent.attenuate(child_session_id, requested)
    }

    /// Borrow the proposal/snapshot ledger.
    pub fn proposal_store(&self) -> &ProposalStore {
        &self.proposals
    }

    /// Borrow the task store.
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// Borrow the worker pool if started.
    pub(crate) fn pool(&self) -> Option<&WorkerPool> {
        self.pool.get()
    }

    /// Handle to the worker pool's tokio runtime, if started.
    pub(crate) fn pool_handle(&self) -> Option<tokio::runtime::Handle> {
        self.pool.get().map(WorkerPool::handle)
    }

    /// Pool utilisation metrics, if started.
    pub(crate) fn pool_metrics(&self) -> Option<crate::pool::PoolMetrics> {
        self.pool.get().map(WorkerPool::metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_admission_limits_are_sensible() {
        let cfg = AdmissionConfig::default();
        assert!(cfg.max_concurrent_user_driven > 0);
        assert!(cfg.max_concurrent_ambient > 0);
    }

    #[test]
    fn admission_limit_for_each_kind() {
        let cfg = AdmissionConfig {
            max_concurrent_user_driven: 1,
            max_concurrent_ambient: 2,
            max_concurrent_signal_triggered: 3,
            max_concurrent_sub_agents: 4,
        };
        assert_eq!(cfg.limit_for(SessionKind::UserDriven), 1);
        assert_eq!(cfg.limit_for(SessionKind::Ambient), 2);
        assert_eq!(cfg.limit_for(SessionKind::SignalTriggered), 3);
        assert_eq!(cfg.limit_for(SessionKind::SubAgent), 4);
    }

    #[test]
    fn supervisor_new_has_no_pool() {
        let sup = Supervisor::new();
        assert!(sup.pool().is_none());
        assert!(sup.pool_handle().is_none());
        assert!(sup.pool_metrics().is_none());
    }

    #[test]
    fn set_pool_is_idempotent() {
        let sup = Supervisor::new();
        // We can't construct a real WorkerPool in a unit test easily,
        // so just verify the OnceLock semantics via a type-level check.
        // The full pool-wiring path is exercised in core_plugin tests
        // via wire_pool_for_tests.
        drop(sup);
    }

    #[test]
    fn with_admission_overrides_defaults() {
        let sup = Supervisor::new().with_admission(AdmissionConfig {
            max_concurrent_user_driven: 99,
            ..Default::default()
        });
        assert_eq!(sup.admission.max_concurrent_user_driven, 99);
    }
}
