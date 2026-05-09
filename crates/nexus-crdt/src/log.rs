//! Append-only operation log keyed by [`OpId`].
//!
//! The log is the source of truth for "what has this site applied".
//! Two sites that have applied the same set of [`OpId`]s necessarily
//! hold convergent state (CRDT property).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::id::{OpId, VersionVector};
use crate::op::CrdtOp;

/// Append-only, idempotent log of CRDT ops.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpLog {
    /// Ops in causal-application order. The order is *one* valid
    /// linearisation of causality, not the only one — replaying a
    /// different valid linearisation on a fresh doc yields the same
    /// final state.
    history: Vec<OpId>,
    /// Op storage keyed by id.
    ops: HashMap<OpId, CrdtOp>,
    /// Cached version-vector summary of `history`.
    vv: VersionVector,
}

impl OpLog {
    /// Create an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Has `id` been appended already?
    #[must_use]
    pub fn contains(&self, id: OpId) -> bool {
        self.ops.contains_key(&id)
    }

    /// Append `op`. Idempotent: a duplicate `OpId` is silently ignored.
    /// Returns `true` if the log changed, `false` if the op was a
    /// duplicate.
    pub fn append(&mut self, op: CrdtOp) -> bool {
        if self.ops.contains_key(&op.id) {
            return false;
        }
        let id = op.id;
        self.history.push(id);
        self.vv.observe(id);
        self.ops.insert(id, op);
        true
    }

    /// Number of ops applied.
    #[must_use]
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// True if no ops have been appended.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Borrow the version vector summarising what has been applied.
    #[must_use]
    pub fn version_vector(&self) -> &VersionVector {
        &self.vv
    }

    /// Iterate ops in causal-application order.
    pub fn iter(&self) -> impl Iterator<Item = &CrdtOp> {
        self.history.iter().filter_map(|id| self.ops.get(id))
    }

    /// Look up a stored op by id.
    #[must_use]
    pub fn get(&self, id: OpId) -> Option<&CrdtOp> {
        self.ops.get(&id)
    }

    /// Return the ops we've applied that `remote_vv` has not yet seen
    /// — useful for gossip / catch-up sync where the remote sends its
    /// VV and we reply with the missing slice.
    #[must_use]
    pub fn missing_for(&self, remote_vv: &VersionVector) -> Vec<&CrdtOp> {
        self.history
            .iter()
            .filter_map(|id| {
                let op = self.ops.get(id)?;
                if remote_vv.contains(*id) {
                    None
                } else {
                    Some(op)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nexus_editor::Operation;
    use uuid::Uuid;

    use super::*;
    use crate::id::{Lamport, SiteId};

    fn dummy_op(site: SiteId, lamport: Lamport, vv: VersionVector) -> CrdtOp {
        CrdtOp {
            id: OpId::new(site, lamport),
            vv_at_creation: vv,
            op: Operation::InsertText {
                block_id: Uuid::new_v4(),
                pos: 0,
                text: "x".into(),
                pre_annotations: vec![],
            },
            rga_ops: vec![],
        }
    }

    #[test]
    fn append_is_idempotent_by_op_id() {
        let s = SiteId::new();
        let op = dummy_op(s, Lamport(1), VersionVector::new());
        let mut log = OpLog::new();
        assert!(log.append(op.clone()));
        assert!(!log.append(op.clone()), "duplicate must be a no-op");
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn version_vector_tracks_appends() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let mut log = OpLog::new();
        log.append(dummy_op(s1, Lamport(1), VersionVector::new()));
        log.append(dummy_op(s1, Lamport(2), VersionVector::new()));
        log.append(dummy_op(s2, Lamport(7), VersionVector::new()));

        let vv = log.version_vector();
        assert_eq!(vv.get(&s1), Lamport(2));
        assert_eq!(vv.get(&s2), Lamport(7));
    }

    #[test]
    fn missing_for_returns_only_unseen_ops() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let mut log = OpLog::new();
        log.append(dummy_op(s1, Lamport(1), VersionVector::new()));
        log.append(dummy_op(s1, Lamport(2), VersionVector::new()));
        log.append(dummy_op(s2, Lamport(3), VersionVector::new()));

        let mut remote = VersionVector::new();
        remote.observe(OpId::new(s1, Lamport(1)));
        let missing: Vec<_> = log.missing_for(&remote).into_iter().map(|o| o.id).collect();
        assert_eq!(
            missing,
            vec![OpId::new(s1, Lamport(2)), OpId::new(s2, Lamport(3)),]
        );
    }
}
