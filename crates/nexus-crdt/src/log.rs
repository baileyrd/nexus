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
///
/// Serialized form represents `ops` as a `Vec<(OpId, CrdtOp)>` because
/// JSON object keys must be strings — `OpId` is a struct.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpLog {
    /// Ops in causal-application order. The order is *one* valid
    /// linearisation of causality, not the only one — replaying a
    /// different valid linearisation on a fresh doc yields the same
    /// final state.
    history: Vec<OpId>,
    /// Op storage keyed by id.
    #[serde(with = "ops_map_vec")]
    ops: HashMap<OpId, CrdtOp>,
    /// Cached version-vector summary of `history`.
    vv: VersionVector,
    /// Per-site high-water mark of *pruned* ops (BL-074 follow-up
    /// compaction). An op id whose lamport ≤ `pruned_floor.get(site)`
    /// was either pruned (known seen) or is below the prune horizon
    /// and must be treated as already-applied to keep peers
    /// convergent. Distinct from `vv` so [`Self::append`] stays the
    /// authoritative path for "have we actually seen this op".
    #[serde(default)]
    pruned_floor: VersionVector,
}

mod ops_map_vec {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::id::OpId;
    use crate::op::CrdtOp;

    pub(super) fn serialize<S: Serializer>(
        map: &HashMap<OpId, CrdtOp>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let v: Vec<(OpId, &CrdtOp)> = map.iter().map(|(k, v)| (*k, v)).collect();
        v.serialize(s)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<OpId, CrdtOp>, D::Error> {
        let v: Vec<(OpId, CrdtOp)> = Vec::deserialize(d)?;
        Ok(v.into_iter().collect())
    }
}

impl OpLog {
    /// Create an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Has `id` been appended already, or is it below the prune
    /// horizon (and so already known-seen)?
    #[must_use]
    pub fn contains(&self, id: OpId) -> bool {
        self.ops.contains_key(&id) || self.pruned_floor.contains(id)
    }

    /// Append `op`. Idempotent: a duplicate `OpId` is silently ignored.
    /// An op below the prune floor (already seen and compacted) is
    /// also silently dropped — re-appending would resurrect a state
    /// every replica has agreed to forget. Returns `true` if the log
    /// changed, `false` if the op was a duplicate or pruned.
    pub fn append(&mut self, op: CrdtOp) -> bool {
        if self.contains(op.id) {
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

    /// Compaction primitive: drop every op whose [`OpId`] is dominated
    /// by `stable_vv` (i.e., contained in it), and advance the
    /// internal prune floor so [`Self::contains`] keeps reporting
    /// those ids as seen. Returns the number of ops removed.
    ///
    /// **Caller invariant:** `stable_vv` must represent state every
    /// active replica has acknowledged. If a replica below the floor
    /// later resurrects and sends one of the pruned ops, this log
    /// will drop it as a duplicate — exactly the behaviour you want
    /// when the resurrection is a stale replica catching up, but
    /// catastrophic if `stable_vv` was computed against a partial
    /// view of the cluster.
    ///
    /// Safe usage in `nexus`: only call from the BL-007 git-pull
    /// landing path after the merged log has been written to disk
    /// and acknowledged by every site whose state contributed to
    /// `stable_vv`. The publisher does not call this automatically.
    pub fn prune_dominated(&mut self, stable_vv: &VersionVector) -> usize {
        let before = self.history.len();
        self.history.retain(|id| !stable_vv.contains(*id));
        self.ops.retain(|id, _| !stable_vv.contains(*id));
        // Advance the prune floor — site by site — so future
        // `contains(id)` calls report seen for any id at or below
        // the floor regardless of whether it's still in `ops`.
        for (site, lamport) in &stable_vv.0 {
            let entry = self.pruned_floor.0.entry(*site).or_default();
            if *lamport > *entry {
                *entry = *lamport;
            }
        }
        before - self.history.len()
    }

    /// Borrow the prune floor (for diagnostics / persistence).
    #[must_use]
    pub fn pruned_floor(&self) -> &VersionVector {
        &self.pruned_floor
    }

    /// Idempotent union: absorb every op in `other` that this log
    /// hasn't seen, in `other.history` order. Returns the number of
    /// ops absorbed (0 when the union is a no-op).
    ///
    /// Phase 4 git-merge driver: on a pull conflict, both sides of the
    /// `.forge/.editor/crdt/<sha>.json` file are loaded as `OpLog`s and
    /// the result is the merge of the two. Order doesn't affect
    /// convergence — replaying the merged log on a fresh
    /// [`crate::CrdtDoc`] produces the same state regardless of which
    /// side was merged into which.
    pub fn merge(&mut self, other: &Self) -> usize {
        let mut absorbed = 0;
        for op in other.iter() {
            if self.append(op.clone()) {
                absorbed += 1;
            }
        }
        absorbed
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
    fn prune_dominated_removes_ops_and_keeps_contains_truthful() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let mut log = OpLog::new();
        log.append(dummy_op(s1, Lamport(1), VersionVector::new()));
        log.append(dummy_op(s1, Lamport(2), VersionVector::new()));
        log.append(dummy_op(s1, Lamport(3), VersionVector::new()));
        log.append(dummy_op(s2, Lamport(7), VersionVector::new()));

        // Stable VV says everyone has seen s1 up to 2 and s2 up to 7.
        let mut stable = VersionVector::new();
        stable.observe(OpId::new(s1, Lamport(2)));
        stable.observe(OpId::new(s2, Lamport(7)));

        let removed = log.prune_dominated(&stable);
        assert_eq!(removed, 3, "s1/1, s1/2, s2/7 are pruned");
        assert_eq!(log.len(), 1, "only s1/3 remains");

        // Pruned ids still report as seen.
        assert!(log.contains(OpId::new(s1, Lamport(1))));
        assert!(log.contains(OpId::new(s1, Lamport(2))));
        assert!(log.contains(OpId::new(s2, Lamport(7))));
        // Surviving op is still seen.
        assert!(log.contains(OpId::new(s1, Lamport(3))));
        // An id above the floor that we never had reports unseen.
        assert!(!log.contains(OpId::new(s1, Lamport(99))));
    }

    #[test]
    fn prune_then_merge_skips_pruned_ops() {
        // Two logs share history; one prunes, the other still has the
        // ops. Merging the unpruned into the pruned must not
        // resurrect the pruned ops (the floor blocks them as
        // already-seen duplicates).
        let s1 = SiteId::new();
        let mut a = OpLog::new();
        let mut b = OpLog::new();
        let op1 = dummy_op(s1, Lamport(1), VersionVector::new());
        let op2 = dummy_op(s1, Lamport(2), VersionVector::new());
        a.append(op1.clone());
        a.append(op2.clone());
        b.append(op1);
        b.append(op2);

        let mut stable = VersionVector::new();
        stable.observe(OpId::new(s1, Lamport(2)));
        a.prune_dominated(&stable);
        assert_eq!(a.len(), 0, "all ops at/below stable were pruned");

        let absorbed = a.merge(&b);
        assert_eq!(absorbed, 0, "pruned ops must NOT be re-absorbed");
        assert_eq!(a.len(), 0);
    }

    #[test]
    fn merge_is_idempotent_and_unions() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let mut a = OpLog::new();
        let mut b = OpLog::new();
        // a: ops 1,2 from s1
        a.append(dummy_op(s1, Lamport(1), VersionVector::new()));
        a.append(dummy_op(s1, Lamport(2), VersionVector::new()));
        // b: op 1 from s1 (overlaps), op 7 from s2 (new)
        b.append(dummy_op(s1, Lamport(1), VersionVector::new()));
        b.append(dummy_op(s2, Lamport(7), VersionVector::new()));

        let absorbed = a.merge(&b);
        assert_eq!(absorbed, 1, "only s2/7 was new to a");
        assert_eq!(a.len(), 3);

        // Second merge is a no-op (idempotent).
        let absorbed2 = a.merge(&b);
        assert_eq!(absorbed2, 0);
        assert_eq!(a.len(), 3);

        // Resulting VV reflects both sites.
        assert_eq!(a.version_vector().get(&s1), Lamport(2));
        assert_eq!(a.version_vector().get(&s2), Lamport(7));
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
