//! Identity types for the CRDT: site IDs, Lamport clocks, op IDs, and
//! version vectors.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Per-session site identifier. Two sessions on the same forge that
/// edit the same file MUST have distinct `SiteId`s — the CRDT's tiebreak
/// rules assume site IDs are unique.
///
/// In practice each `EditorCorePlugin` session generates one `SiteId`
/// at startup; popout windows and Tauri sub-views inherit it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SiteId(pub Uuid);

impl SiteId {
    /// Generate a fresh site id (UUID v4).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SiteId {
    fn default() -> Self {
        Self::new()
    }
}

/// Lamport timestamp — a monotonically increasing per-site counter.
///
/// On every locally-authored op the site bumps its lamport, then assigns
/// the new value to the op. On every received remote op, the site
/// advances `lamport = max(lamport, remote.lamport)` so future locally-
/// authored ops are guaranteed to dominate everything seen so far on
/// this site.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Lamport(pub u64);

impl Lamport {
    /// Return the next tick after `self`.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Globally-unique operation id. Total order is `(lamport, site)`:
/// lamport breaks most ties; the site UUID is an unambiguous fallback
/// for concurrent ops that share a lamport.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
pub struct OpId {
    /// Logical timestamp (primary sort key).
    pub lamport: Lamport,
    /// Authoring site (tiebreak).
    pub site: SiteId,
}

impl OpId {
    /// Construct an op id from raw parts.
    #[must_use]
    pub fn new(site: SiteId, lamport: Lamport) -> Self {
        Self { lamport, site }
    }
}

/// A version vector: the maximum [`Lamport`] this site has observed
/// from each known remote site. When a remote op arrives whose
/// `vv_at_creation` is dominated by ours, the op is causally ready.
///
/// `dominates(a, b)`  ⇔  for every site, `a[s] >= b[s]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionVector(pub HashMap<SiteId, Lamport>);

impl VersionVector {
    /// Empty vector (no ops observed).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the max lamport observed for `site`, or zero.
    #[must_use]
    pub fn get(&self, site: &SiteId) -> Lamport {
        self.0.get(site).copied().unwrap_or_default()
    }

    /// Record that `id` has been applied: bumps `self[id.site]` to
    /// `max(self[id.site], id.lamport)`.
    pub fn observe(&mut self, id: OpId) {
        let entry = self.0.entry(id.site).or_default();
        if id.lamport > *entry {
            *entry = id.lamport;
        }
    }

    /// Has `id` been observed? True iff `self[id.site] >= id.lamport`.
    #[must_use]
    pub fn contains(&self, id: OpId) -> bool {
        self.get(&id.site) >= id.lamport
    }

    /// Does `self` dominate `other`? True iff for every site present
    /// in `other`, `self[s] >= other[s]`.
    #[must_use]
    pub fn dominates(&self, other: &Self) -> bool {
        other.0.iter().all(|(s, l)| self.get(s) >= *l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamport_orders_op_ids_before_site_id() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let earlier = OpId::new(s1, Lamport(1));
        let later = OpId::new(s2, Lamport(2));
        assert!(earlier < later, "lamport is the primary sort key");
    }

    #[test]
    fn version_vector_observes_and_dominates() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let mut vv = VersionVector::new();
        vv.observe(OpId::new(s1, Lamport(3)));
        vv.observe(OpId::new(s1, Lamport(2))); // older — must not regress
        vv.observe(OpId::new(s2, Lamport(5)));

        assert_eq!(vv.get(&s1), Lamport(3));
        assert_eq!(vv.get(&s2), Lamport(5));

        let mut older = VersionVector::new();
        older.observe(OpId::new(s1, Lamport(2)));
        older.observe(OpId::new(s2, Lamport(5)));
        assert!(vv.dominates(&older));
        assert!(!older.dominates(&vv));
    }

    #[test]
    fn version_vector_contains_is_strict() {
        let s = SiteId::new();
        let mut vv = VersionVector::new();
        vv.observe(OpId::new(s, Lamport(4)));
        assert!(vv.contains(OpId::new(s, Lamport(4))));
        assert!(vv.contains(OpId::new(s, Lamport(3))));
        assert!(!vv.contains(OpId::new(s, Lamport(5))));
    }
}
