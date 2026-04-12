//! Generic credential vault over the OS keyring.
//!
//! Thin wrapper around `keyring-rs`. Credentials are stored as key-value
//! pairs with a dot-namespaced naming convention (e.g., `"ai.anthropic"`).
//!
//! Hard-fail policy (ADR 0009): `available()` is called at startup. If
//! the keyring is unavailable and `NEXUS_NO_KEYRING` is not set, Nexus
//! refuses to start. If `NEXUS_NO_KEYRING=1`, credential operations
//! return `SecurityError::KeyringDisabled`.

use crate::SecurityError;

/// Service name used for all keyring entries.
const SERVICE_NAME: &str = "nexus";

/// Generic credential vault backed by the OS keyring.
#[derive(Debug)]
pub struct CredentialVault {
    disabled: bool,
}

impl CredentialVault {
    /// Create a new credential vault.
    ///
    /// Checks the `NEXUS_NO_KEYRING` environment variable. If set to `"1"`,
    /// the vault operates in disabled mode: `available()` returns `Ok(())`
    /// but all credential operations return `SecurityError::KeyringDisabled`.
    #[must_use]
    pub fn new() -> Self {
        let disabled = std::env::var("NEXUS_NO_KEYRING")
            .map(|v| v == "1")
            .unwrap_or(false);
        Self { disabled }
    }

    /// Check whether the OS keyring is accessible.
    ///
    /// In disabled mode (`NEXUS_NO_KEYRING=1`), returns `Ok(())` — the
    /// startup check passes, but individual operations will fail with
    /// `KeyringDisabled`.
    ///
    /// # Errors
    /// Returns `SecurityError::KeyringUnavailable` with a platform-specific
    /// hint if the keyring cannot be accessed.
    pub fn available(&self) -> Result<(), SecurityError> {
        if self.disabled {
            return Ok(());
        }

        // Probe the keyring by attempting to get a non-existent entry.
        // keyring-rs returns NoEntry (not a platform error) if the keyring
        // works but the entry doesn't exist — that's a success for our probe.
        let entry = keyring::Entry::new(SERVICE_NAME, "__nexus_probe__")
            .map_err(|e| platform_error(e.to_string()))?;

        match entry.get_password() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()), // keyring works, entry just missing
            Err(e) => Err(platform_error(e.to_string())),
        }
    }

    /// Store a credential in the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialStoreFailed` if the keyring operation fails.
    pub fn store(&self, name: &str, value: &str) -> Result<(), SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "store");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialStoreFailed(e.to_string()))?;

        entry
            .set_password(value)
            .map_err(|e| SecurityError::CredentialStoreFailed(e.to_string()))
    }

    /// Retrieve a credential from the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialNotFound` if the credential doesn't exist.
    pub fn retrieve(&self, name: &str) -> Result<String, SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "retrieve");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialNotFound(e.to_string()))?;

        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecurityError::CredentialNotFound(name.to_string()),
            other => SecurityError::CredentialNotFound(other.to_string()),
        })
    }

    /// Delete a credential from the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialNotFound` if the credential doesn't exist.
    pub fn delete(&self, name: &str) -> Result<(), SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "delete");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialNotFound(e.to_string()))?;

        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecurityError::CredentialNotFound(name.to_string()),
            other => SecurityError::CredentialNotFound(other.to_string()),
        })
    }

    /// Whether the vault is in disabled mode (`NEXUS_NO_KEYRING=1`).
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl Default for CredentialVault {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a `KeyringUnavailable` error with a platform-specific hint.
fn platform_error(reason: String) -> SecurityError {
    let platform_hint = if cfg!(target_os = "linux") {
        "On Linux, ensure D-Bus and a Secret Service provider (e.g., gnome-keyring or KWallet) are running.".to_string()
    } else if cfg!(target_os = "macos") {
        "On macOS, ensure Keychain Access is unlocked.".to_string()
    } else if cfg!(target_os = "windows") {
        "On Windows, ensure Credential Manager is accessible.".to_string()
    } else {
        "Ensure your platform's credential storage is configured and accessible.".to_string()
    };

    SecurityError::KeyringUnavailable {
        reason,
        platform_hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a vault in disabled mode by temporarily setting the env var.
    fn disabled_vault() -> CredentialVault {
        // SAFETY: tests run single-threaded via nextest; no concurrent env reads.
        unsafe {
            std::env::set_var("NEXUS_NO_KEYRING", "1");
        }
        let vault = CredentialVault::new();
        // SAFETY: same as above.
        unsafe {
            std::env::remove_var("NEXUS_NO_KEYRING");
        }
        vault
    }

    #[test]
    fn disabled_vault_is_disabled() {
        let vault = disabled_vault();
        assert!(vault.is_disabled());
    }

    #[test]
    fn disabled_available_returns_ok() {
        let vault = disabled_vault();
        assert!(vault.available().is_ok());
    }

    #[test]
    fn disabled_store_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.store("ai.test", "secret123").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn disabled_retrieve_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.retrieve("ai.test").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn disabled_delete_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.delete("ai.test").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn default_vault_not_disabled_when_env_unset() {
        // SAFETY: single-threaded nextest process; no concurrent env reads.
        unsafe {
            std::env::remove_var("NEXUS_NO_KEYRING");
        }
        let vault = CredentialVault::new();
        assert!(!vault.is_disabled());
    }

    #[test]
    fn env_value_other_than_1_is_not_disabled() {
        // SAFETY: single-threaded nextest process; no concurrent env reads.
        unsafe {
            std::env::set_var("NEXUS_NO_KEYRING", "true");
        }
        let vault = CredentialVault::new();
        // SAFETY: same as above.
        unsafe {
            std::env::remove_var("NEXUS_NO_KEYRING");
        }
        assert!(!vault.is_disabled(), "only '1' should disable the keyring");
    }

    #[test]
    fn platform_error_contains_platform_hint() {
        let err = platform_error("test reason".to_string());
        match err {
            SecurityError::KeyringUnavailable {
                reason,
                platform_hint,
            } => {
                assert_eq!(reason, "test reason");
                assert!(!platform_hint.is_empty());
            }
            other => panic!("expected KeyringUnavailable, got {other:?}"),
        }
    }
}
