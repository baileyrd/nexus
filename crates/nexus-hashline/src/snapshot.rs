//! A bounded store of file snapshots, keyed by path, for stale-TAG 3-way merge.
//!
//! When a reader records the content it saw, a later edit whose TAG no longer
//! matches the live file can reconstruct that base and merge against it. The
//! store is deliberately small and in-memory (per process / per session); it is
//! a recovery aid, not a source of truth.

use std::collections::{HashMap, VecDeque};

use crate::tag::tag;

/// Maximum number of distinct paths retained (oldest path evicted past this).
pub const MAX_SNAPSHOT_PATHS: usize = 30;
/// Maximum versions kept per path (oldest version dropped past this).
pub const MAX_VERSIONS_PER_PATH: usize = 4;
/// Largest content (in bytes) that will be recorded; larger reads are skipped.
pub const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;

/// One recorded version of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// TAG of the recorded content.
    pub tag: String,
    /// The recorded content.
    pub content: String,
}

/// Bounded, per-path snapshot store with oldest-path / oldest-version eviction.
#[derive(Debug, Default)]
pub struct SnapshotStore {
    /// Path recency order; front = least recently recorded, back = most recent.
    order: VecDeque<String>,
    /// Per-path version ring (front = oldest version, back = newest).
    versions: HashMap<String, VecDeque<Snapshot>>,
}

impl SnapshotStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `content` for `path`. Content over [`MAX_SNAPSHOT_BYTES`] is
    /// ignored; a re-record of the already-newest version is deduplicated.
    pub fn record(&mut self, path: &str, content: &str) {
        if content.len() > MAX_SNAPSHOT_BYTES {
            return;
        }
        let computed = tag(content);
        let ring = self.versions.entry(path.to_string()).or_default();
        if ring.back().is_some_and(|s| s.tag == computed) {
            self.touch(path);
            return;
        }
        ring.push_back(Snapshot {
            tag: computed,
            content: content.to_string(),
        });
        while ring.len() > MAX_VERSIONS_PER_PATH {
            ring.pop_front();
        }
        self.touch(path);
        self.evict();
    }

    /// Find a recorded version of `path` whose TAG matches `wanted` (newest first).
    #[must_use]
    pub fn get_by_tag(&self, path: &str, wanted: &str) -> Option<&Snapshot> {
        self.versions
            .get(path)?
            .iter()
            .rev()
            .find(|s| s.tag.eq_ignore_ascii_case(wanted))
    }

    /// The most recently recorded version of `path`, if any.
    #[must_use]
    pub fn latest(&self, path: &str) -> Option<&Snapshot> {
        self.versions.get(path)?.back()
    }

    /// Number of paths currently retained.
    #[must_use]
    pub fn path_count(&self) -> usize {
        self.order.len()
    }

    fn touch(&mut self, path: &str) {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.order.push_back(path.to_string());
    }

    fn evict(&mut self) {
        while self.order.len() > MAX_SNAPSHOT_PATHS {
            if let Some(old) = self.order.pop_front() {
                self.versions.remove(&old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_finds_by_tag() {
        let mut store = SnapshotStore::new();
        store.record("a", "hello\n");
        let t = tag("hello\n");
        assert_eq!(store.get_by_tag("a", &t).unwrap().content, "hello\n");
        assert!(store.get_by_tag("a", "0000").is_none());
        assert!(store.get_by_tag("missing", &t).is_none());
    }

    #[test]
    fn keeps_bounded_versions_per_path() {
        let mut store = SnapshotStore::new();
        for i in 0..(MAX_VERSIONS_PER_PATH + 2) {
            store.record("a", &format!("v{i}\n"));
        }
        // Oldest two versions evicted; latest is the final write.
        assert_eq!(store.latest("a").unwrap().content, format!("v{}\n", MAX_VERSIONS_PER_PATH + 1));
        assert!(store.get_by_tag("a", &tag("v0\n")).is_none());
        assert!(store.get_by_tag("a", &tag("v1\n")).is_none());
        assert!(store.get_by_tag("a", &tag("v2\n")).is_some());
    }

    #[test]
    fn evicts_oldest_path_past_cap() {
        let mut store = SnapshotStore::new();
        for i in 0..=MAX_SNAPSHOT_PATHS {
            store.record(&format!("p{i}"), "x\n");
        }
        assert_eq!(store.path_count(), MAX_SNAPSHOT_PATHS);
        assert!(store.latest("p0").is_none(), "oldest path should be evicted");
        assert!(store.latest(&format!("p{MAX_SNAPSHOT_PATHS}")).is_some());
    }

    #[test]
    fn dedupes_identical_re_record() {
        let mut store = SnapshotStore::new();
        store.record("a", "same\n");
        store.record("a", "same\n");
        assert_eq!(store.versions.get("a").unwrap().len(), 1);
    }

    #[test]
    fn skips_oversized_content() {
        let mut store = SnapshotStore::new();
        let big = "a".repeat(MAX_SNAPSHOT_BYTES + 1);
        store.record("a", &big);
        assert!(store.latest("a").is_none());
    }
}
