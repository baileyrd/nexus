//! Provider HTTP client construction (BL-102).
//!
//! Re-exports [`nexus_security::tls::build_pinned_client`] under the
//! historical name `build_client` so existing nexus-ai callers compile
//! unchanged. nexus-audio uses the same builder directly from
//! `nexus-security` — having both subsystems land in one place keeps
//! the TLS-pinning gate honest.

/// Build a `reqwest::Client` for outbound provider HTTPS. See
/// [`nexus_security::tls::build_pinned_client`] for the full contract.
#[must_use]
pub fn build_client(tls_pinning_enabled: bool) -> reqwest::Client {
    nexus_security::tls::build_pinned_client(tls_pinning_enabled)
}
