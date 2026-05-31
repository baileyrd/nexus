//! Security error types.

use nexus_kernel::Capability;
use std::path::PathBuf;

/// Errors from the security subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// OS keyring is unavailable (no D-Bus, locked Keychain, etc.).
    #[error("OS keyring unavailable: {reason}\n  {platform_hint}")]
    KeyringUnavailable {
        /// The underlying error from keyring-rs.
        reason: String,
        /// Platform-specific remediation hint.
        platform_hint: String,
    },

    /// Keyring disabled via `NEXUS_NO_KEYRING=1`.
    #[error("keyring disabled (NEXUS_NO_KEYRING=1): credential operations unavailable")]
    KeyringDisabled,

    /// Requested credential not found in keyring.
    #[error("credential not found: {0}")]
    CredentialNotFound(String),

    /// Failed to store a credential in the keyring.
    #[error("failed to store credential: {0}")]
    CredentialStoreFailed(String),

    /// Path traversal attempt detected — resolved path escapes forge root.
    #[error("path traversal denied: {} escapes forge root", .0.display())]
    PathTraversal(PathBuf),

    /// Path contains invalid characters (null bytes, etc.).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A plugin attempted an operation without the required capability.
    #[error("capability denied: {0}")]
    CapabilityDenied(Capability),

    /// TLS pin mismatch (BL-102). The leaf certificate's
    /// SubjectPublicKeyInfo SHA-256 hash did not match any pin
    /// configured for `host`. Connection is aborted before any
    /// request bytes are sent.
    #[error("TLS pin mismatch for {host}: expected one of {expected:?}, got {actual}")]
    CertificatePinMismatch {
        /// The hostname that triggered the verifier.
        host: String,
        /// The configured pins for this host (hex-encoded SHA-256).
        expected: Vec<String>,
        /// The leaf certificate's actual SPKI SHA-256 (hex).
        actual: String,
    },

    /// `tls_pinning_enabled` is on but the host has no configured
    /// pins. Fail loud rather than silently allowing the connection.
    #[error(
        "TLS pinning is enabled but no pins are configured for {host} \
         — set pins in nexus-security/src/tls_pins.rs or set \
         tls_pinning_enabled = false to opt out"
    )]
    NoPinsConfigured {
        /// The hostname that lacks pins.
        host: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_unavailable_displays_reason_and_hint() {
        let err = SecurityError::KeyringUnavailable {
            reason: "D-Bus not running".to_string(),
            platform_hint: "Ensure gnome-keyring is running.".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("D-Bus not running"));
        assert!(msg.contains("gnome-keyring"));
    }

    #[test]
    fn keyring_disabled_display() {
        let err = SecurityError::KeyringDisabled;
        let msg = format!("{err}");
        assert!(msg.contains("NEXUS_NO_KEYRING=1"));
    }

    #[test]
    fn credential_not_found_displays_name() {
        let err = SecurityError::CredentialNotFound("ai.anthropic".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("ai.anthropic"));
    }

    #[test]
    fn credential_store_failed_displays_reason() {
        let err = SecurityError::CredentialStoreFailed("permission denied".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn path_traversal_displays_path() {
        let err = SecurityError::PathTraversal(PathBuf::from("/etc/passwd"));
        let msg = format!("{err}");
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("escapes forge root"));
    }

    #[test]
    fn invalid_path_displays_reason() {
        let err = SecurityError::InvalidPath("contains null byte".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("null byte"));
    }

    #[test]
    fn capability_denied_displays_cap_name() {
        let err = SecurityError::CapabilityDenied(Capability::FsRead);
        let msg = format!("{err}");
        assert!(msg.contains("fs.read"));
    }
}
