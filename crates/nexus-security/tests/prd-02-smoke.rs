//! PRD 02 §12 smoke test: verifies the `nexus-security` public API surface
//! compiles, types are accessible, and basic behavior is correct.
//!
//! This test does NOT exercise the real OS keyring — it only tests the
//! disabled-mode path and non-keyring modules.

use nexus_kernel::Capability;
use nexus_security::{
    audit, risk_level, CredentialVault, ForgePathValidator, RiskLevel, SecurityError,
};
use std::path::Path;

#[test]
fn risk_level_covers_all_capabilities() {
    for &cap in Capability::ALL {
        let level = risk_level(cap);
        // Every capability must map to a valid risk level
        assert!(
            matches!(level, RiskLevel::Low | RiskLevel::Medium | RiskLevel::High),
            "unexpected risk level for {cap}: {level}"
        );
    }
}

#[test]
fn risk_level_high_caps_match_spec() {
    let high_caps = [
        Capability::FsReadExternal,
        Capability::FsWriteExternal,
        Capability::NetHttp,
        Capability::ProcessSpawn,
        Capability::IpcCall,
    ];
    for cap in high_caps {
        assert!(
            risk_level(cap).is_high(),
            "{cap} should be HIGH risk"
        );
    }
}

#[test]
fn credential_vault_disabled_mode() {
    // SAFETY: single-threaded nextest process; no concurrent env reads.
    unsafe {
        std::env::set_var("NEXUS_NO_KEYRING", "1");
    }
    let vault = CredentialVault::new();
    unsafe {
        std::env::remove_var("NEXUS_NO_KEYRING");
    }

    assert!(vault.is_disabled());
    assert!(vault.available().is_ok());
    assert!(matches!(
        vault.store("test", "val").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
    assert!(matches!(
        vault.retrieve("test").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
    assert!(matches!(
        vault.delete("test").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
}

#[test]
fn forge_path_validator_blocks_traversal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ok.txt"), "safe").unwrap();

    let validator = ForgePathValidator::new(dir.path()).unwrap();

    // Valid path succeeds
    assert!(validator.validate(Path::new("ok.txt")).is_ok());

    // Traversal is blocked
    assert!(validator.validate(Path::new("../../../etc/passwd")).is_err());
}

#[test]
fn audit_functions_callable() {
    // Just verify these compile and don't panic when called without a subscriber.
    // In production, a tracing subscriber captures the output.
    audit::log_capability_granted("com.test.smoke", "fs.read");
    audit::log_capability_denied("com.test.smoke", "net.http");
    audit::log_plugin_lifecycle("com.test.smoke", "started");
    audit::log_credential_access("ai.test", "retrieve");
    audit::log_path_traversal_denied(
        "com.test.smoke",
        Path::new("../escape"),
        Path::new("/forge/root"),
    );
}

#[test]
fn security_error_display_is_human_readable() {
    let err = SecurityError::CapabilityDenied(Capability::NetHttp);
    let msg = format!("{err}");
    assert!(msg.contains("net.http"), "error should show capability name: {msg}");

    let err = SecurityError::PathTraversal(std::path::PathBuf::from("/bad/path"));
    let msg = format!("{err}");
    assert!(msg.contains("escapes forge root"), "error should mention traversal: {msg}");
}

#[test]
fn public_type_surface_is_accessible() {
    // Verify all public types are importable and constructible where applicable
    let _level: RiskLevel = RiskLevel::High;
    let _level: RiskLevel = RiskLevel::Medium;
    let _level: RiskLevel = RiskLevel::Low;

    // CredentialVault::new() works
    unsafe {
        std::env::remove_var("NEXUS_NO_KEYRING");
    }
    let _vault = CredentialVault::new();

    // ForgePathValidator requires a real directory
    let dir = tempfile::tempdir().unwrap();
    let _validator = ForgePathValidator::new(dir.path()).unwrap();

    // SecurityError variants are constructible
    let _err = SecurityError::KeyringDisabled;
    let _err = SecurityError::CredentialNotFound("test".to_string());
    let _err = SecurityError::InvalidPath("bad".to_string());
    let _err = SecurityError::CapabilityDenied(Capability::FsRead);
}
