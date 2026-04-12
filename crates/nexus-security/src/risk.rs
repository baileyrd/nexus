//! Capability risk-level metadata.
//!
//! Risk levels determine install-time prompting: community plugins requesting
//! HIGH-risk capabilities require explicit user approval.

use nexus_kernel::Capability;
use serde::{Deserialize, Serialize};

/// Risk level assigned to a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Minimal risk — granted without prompt for all trust levels.
    Low,
    /// Moderate risk — granted without prompt for all trust levels.
    Medium,
    /// Significant risk — requires explicit user approval for community plugins.
    High,
}

/// Returns the risk level for a capability.
///
/// The match is exhaustive over all `Capability` variants. Adding a variant
/// to the kernel will cause a compile error here, forcing the risk mapping
/// to be updated.
#[must_use]
pub fn risk_level(cap: Capability) -> RiskLevel {
    match cap {
        Capability::FsRead
        | Capability::KvRead
        | Capability::KvWrite => RiskLevel::Low,

        Capability::FsWrite
        | Capability::NetHttpLocalhost
        | Capability::DbQuery
        | Capability::DbWrite => RiskLevel::Medium,

        Capability::FsReadExternal
        | Capability::FsWriteExternal
        | Capability::NetHttp
        | Capability::ProcessSpawn
        | Capability::IpcCall => RiskLevel::High,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_has_a_risk_level() {
        for &cap in Capability::ALL {
            let _level = risk_level(cap);
        }
    }

    #[test]
    fn fs_read_is_low() {
        assert_eq!(risk_level(Capability::FsRead), RiskLevel::Low);
    }

    #[test]
    fn kv_read_is_low() {
        assert_eq!(risk_level(Capability::KvRead), RiskLevel::Low);
    }

    #[test]
    fn kv_write_is_low() {
        assert_eq!(risk_level(Capability::KvWrite), RiskLevel::Low);
    }

    #[test]
    fn fs_write_is_medium() {
        assert_eq!(risk_level(Capability::FsWrite), RiskLevel::Medium);
    }

    #[test]
    fn net_http_localhost_is_medium() {
        assert_eq!(risk_level(Capability::NetHttpLocalhost), RiskLevel::Medium);
    }

    #[test]
    fn db_query_is_medium() {
        assert_eq!(risk_level(Capability::DbQuery), RiskLevel::Medium);
    }

    #[test]
    fn db_write_is_medium() {
        assert_eq!(risk_level(Capability::DbWrite), RiskLevel::Medium);
    }

    #[test]
    fn fs_read_external_is_high() {
        assert_eq!(risk_level(Capability::FsReadExternal), RiskLevel::High);
    }

    #[test]
    fn fs_write_external_is_high() {
        assert_eq!(risk_level(Capability::FsWriteExternal), RiskLevel::High);
    }

    #[test]
    fn net_http_is_high() {
        assert_eq!(risk_level(Capability::NetHttp), RiskLevel::High);
    }

    #[test]
    fn process_spawn_is_high() {
        assert_eq!(risk_level(Capability::ProcessSpawn), RiskLevel::High);
    }

    #[test]
    fn ipc_call_is_high() {
        assert_eq!(risk_level(Capability::IpcCall), RiskLevel::High);
    }

    #[test]
    fn risk_level_is_copy_and_eq() {
        let a = RiskLevel::High;
        let b = a;
        assert_eq!(a, b);
    }
}
