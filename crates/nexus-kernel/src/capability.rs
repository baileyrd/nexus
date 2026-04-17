//! Capability system: enum of named capabilities, string conversion, set type.

use serde::{Deserialize, Serialize};

/// A named capability that can be granted to a plugin.
///
/// Capabilities are the single source of truth for the plugin permission
/// system. Plugin manifests reference them as hierarchical dot-namespaced
/// strings (e.g., `"fs.read"`); this enum is the canonical in-memory form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Read files within the forge root.
    FsRead,
    /// Write files within the forge root.
    FsWrite,
    /// Read files outside the forge root (HIGH risk).
    FsReadExternal,
    /// Write files outside the forge root (HIGH risk).
    FsWriteExternal,
    /// Outbound HTTP to any host (HIGH risk).
    NetHttp,
    /// Outbound HTTP to localhost only.
    NetHttpLocalhost,
    /// Spawn child processes (HIGH risk).
    ProcessSpawn,
    /// Read the plugin's own KV store.
    KvRead,
    /// Write the plugin's own KV store.
    KvWrite,
    /// Call IPC commands on other plugins (HIGH risk).
    IpcCall,
    /// Query `SQLite` tables registered by the plugin.
    DbQuery,
    /// Write to `SQLite` tables registered by the plugin.
    DbWrite,
    /// Publish events to the kernel event bus.
    EventsPublish,
    /// Show UI notifications (toasts) to the user.
    UiNotify,
}

/// Error parsing a capability string.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityParseError {
    /// The string does not match any known capability name.
    #[error("unknown capability string '{0}'")]
    UnknownString(String),
}

impl Capability {
    /// Canonical string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::FsRead           => "fs.read",
            Capability::FsWrite          => "fs.write",
            Capability::FsReadExternal   => "fs.read.external",
            Capability::FsWriteExternal  => "fs.write.external",
            Capability::NetHttp          => "net.http",
            Capability::NetHttpLocalhost => "net.http.localhost",
            Capability::ProcessSpawn     => "process.spawn",
            Capability::KvRead           => "kv.read",
            Capability::KvWrite          => "kv.write",
            Capability::IpcCall          => "ipc.call",
            Capability::DbQuery          => "db.query",
            Capability::DbWrite          => "db.write",
            Capability::EventsPublish    => "events.publish",
            Capability::UiNotify         => "ui.notify",
        }
    }

    /// Parse from a manifest string. Returns `CapabilityParseError::UnknownString`
    /// for unknown inputs.
    ///
    /// # Errors
    /// Returns an error if `s` is not a recognized capability name.
    #[allow(clippy::should_implement_trait)] // intentional inherent method — parse errors don't need the FromStr trait plumbing
    pub fn from_str(s: &str) -> Result<Self, CapabilityParseError> {
        match s {
            "fs.read"            => Ok(Capability::FsRead),
            "fs.write"           => Ok(Capability::FsWrite),
            "fs.read.external"   => Ok(Capability::FsReadExternal),
            "fs.write.external"  => Ok(Capability::FsWriteExternal),
            "net.http"           => Ok(Capability::NetHttp),
            "net.http.localhost" => Ok(Capability::NetHttpLocalhost),
            "process.spawn"      => Ok(Capability::ProcessSpawn),
            "kv.read"            => Ok(Capability::KvRead),
            "kv.write"           => Ok(Capability::KvWrite),
            "ipc.call"           => Ok(Capability::IpcCall),
            "db.query"           => Ok(Capability::DbQuery),
            "db.write"           => Ok(Capability::DbWrite),
            "events.publish"     => Ok(Capability::EventsPublish),
            "ui.notify"          => Ok(Capability::UiNotify),
            other                => Err(CapabilityParseError::UnknownString(other.to_string())),
        }
    }

    /// Whether this capability is classified HIGH-risk. HIGH-risk capabilities
    /// bypass sandbox boundaries (external filesystem access, arbitrary network
    /// egress, process spawning, cross-plugin IPC) and are subject to
    /// install-time user consent (F-5.1.1): `build_capabilities` drops them
    /// from the granted set unless they appear in the plugin's
    /// `granted_caps.json`. Kept in lockstep with `nexus-security::risk_level`.
    #[must_use]
    pub const fn is_high_risk(self) -> bool {
        matches!(
            self,
            Capability::FsReadExternal
                | Capability::FsWriteExternal
                | Capability::NetHttp
                | Capability::ProcessSpawn
                | Capability::IpcCall
        )
    }

    /// All capability variants, for exhaustive iteration.
    pub const ALL: &'static [Capability] = &[
        Capability::FsRead,
        Capability::FsWrite,
        Capability::FsReadExternal,
        Capability::FsWriteExternal,
        Capability::NetHttp,
        Capability::NetHttpLocalhost,
        Capability::ProcessSpawn,
        Capability::KvRead,
        Capability::KvWrite,
        Capability::IpcCall,
        Capability::DbQuery,
        Capability::DbWrite,
        Capability::EventsPublish,
        Capability::UiNotify,
    ];
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

use std::collections::HashSet;

/// A set of capabilities granted to a plugin at load time.
///
/// Immutable once constructed — capabilities are not modified at runtime in M1.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    set: HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set (no capabilities granted).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            set: HashSet::new(),
        }
    }

    /// Build a set from an iterator of capabilities.
    #[must_use]
    #[allow(clippy::should_implement_trait)] // intentional inherent method — sidesteps FromIterator for ergonomic naming
    pub fn from_iter(iter: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            set: iter.into_iter().collect(),
        }
    }

    /// Check whether the set contains a specific capability.
    #[must_use]
    pub fn contains(&self, cap: Capability) -> bool {
        self.set.contains(&cap)
    }

    /// Iterate over the capabilities in the set.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.set.iter()
    }

    /// Number of capabilities in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_roundtrip_via_string() {
        for &cap in Capability::ALL {
            let s = cap.as_str();
            let parsed = Capability::from_str(s).unwrap();
            assert_eq!(parsed, cap, "roundtrip failed for {cap:?}");
        }
    }

    #[test]
    fn unknown_string_returns_error() {
        let err = Capability::from_str("fs.bogus").unwrap_err();
        match err {
            CapabilityParseError::UnknownString(s) => assert_eq!(s, "fs.bogus"),
        }
    }

    #[test]
    fn typo_returns_error_not_wrong_variant() {
        let err = Capability::from_str("fs_read").unwrap_err();
        match err {
            CapabilityParseError::UnknownString(s) => assert_eq!(s, "fs_read"),
        }
    }

    #[test]
    fn all_has_fourteen_variants() {
        assert_eq!(Capability::ALL.len(), 14);
    }

    #[test]
    fn as_str_is_dot_namespaced() {
        assert_eq!(Capability::FsReadExternal.as_str(), "fs.read.external");
        assert_eq!(Capability::NetHttpLocalhost.as_str(), "net.http.localhost");
    }

    #[test]
    fn empty_set_contains_nothing() {
        let set = CapabilitySet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.contains(Capability::FsRead));
    }

    #[test]
    fn set_from_iter_contains_those_caps() {
        let set = CapabilitySet::from_iter([Capability::FsRead, Capability::KvRead]);
        assert_eq!(set.len(), 2);
        assert!(set.contains(Capability::FsRead));
        assert!(set.contains(Capability::KvRead));
        assert!(!set.contains(Capability::FsWrite));
    }

    #[test]
    fn set_is_clone_and_independent() {
        let set = CapabilitySet::from_iter([Capability::FsRead]);
        let cloned = set.clone();
        assert!(cloned.contains(Capability::FsRead));
    }

    #[test]
    fn display_matches_as_str() {
        for &cap in Capability::ALL {
            assert_eq!(format!("{cap}"), cap.as_str());
        }
    }

    #[test]
    fn set_iter_yields_all() {
        let caps = [Capability::FsRead, Capability::KvWrite];
        let set = CapabilitySet::from_iter(caps);
        let collected: HashSet<_> = set.iter().copied().collect();
        assert_eq!(collected.len(), 2);
    }
}
