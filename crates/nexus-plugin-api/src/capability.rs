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
    /// All known capability variants in declaration order.
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

    /// Returns `true` if this capability is classified as HIGH risk.
    ///
    /// High-risk capabilities require explicit user approval (persisted grant).
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
    #[allow(clippy::should_implement_trait)]
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
            other => Err(CapabilityParseError::UnknownString(other.to_string())),
        }
    }
}

/// An immutable set of capabilities granted to a plugin.
///
/// Internally a bitmask over the `Capability` discriminant for O(1) contains.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet(std::collections::HashSet<Capability>);

impl CapabilitySet {
    /// Empty set (no capabilities).
    #[must_use]
    pub fn empty() -> Self {
        Self(std::collections::HashSet::new())
    }

    /// Returns `true` if the set contains `cap`.
    #[must_use]
    pub fn contains(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    /// Insert `cap` into the set.
    pub fn insert(&mut self, cap: Capability) {
        self.0.insert(cap);
    }

    /// Remove `cap` from the set. Returns `true` if it was present.
    pub fn remove(&mut self, cap: Capability) -> bool {
        self.0.remove(&cap)
    }

    /// Iterate over capabilities in the set.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    /// Number of capabilities in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_round_trips_through_string() {
        for &cap in Capability::ALL {
            let s = cap.as_str();
            let parsed = Capability::from_str(s).unwrap();
            assert_eq!(cap, parsed, "round-trip failed for {:?}", cap);
        }
    }

    #[test]
    fn unknown_capability_string_errors() {
        assert!(Capability::from_str("fs.delete").is_err());
        assert!(Capability::from_str("").is_err());
    }

    #[test]
    fn capability_set_contains_inserted() {
        let set = CapabilitySet::from_iter([Capability::FsRead, Capability::NetHttp]);
        assert!(set.contains(Capability::FsRead));
        assert!(set.contains(Capability::NetHttp));
        assert!(!set.contains(Capability::KvRead));
    }

    #[test]
    fn capability_set_empty_is_empty() {
        let set = CapabilitySet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn all_slice_covers_all_discriminants() {
        assert_eq!(Capability::ALL.len(), 14);
    }
}
