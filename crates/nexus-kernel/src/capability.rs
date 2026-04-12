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
    /// Query SQLite tables registered by the plugin.
    DbQuery,
    /// Write to SQLite tables registered by the plugin.
    DbWrite,
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
        }
    }

    /// Parse from a manifest string. Returns `CapabilityParseError::UnknownString`
    /// for unknown inputs.
    ///
    /// # Errors
    /// Returns an error if `s` is not a recognized capability name.
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
            other                => Err(CapabilityParseError::UnknownString(other.to_string())),
        }
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
    ];
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
    fn all_has_twelve_variants() {
        assert_eq!(Capability::ALL.len(), 12);
    }

    #[test]
    fn as_str_is_dot_namespaced() {
        assert_eq!(Capability::FsReadExternal.as_str(), "fs.read.external");
        assert_eq!(Capability::NetHttpLocalhost.as_str(), "net.http.localhost");
    }
}
