//! Provider HTTP client construction (BL-102).
//!
//! All AI providers' `reqwest::Client` instances funnel through
//! [`build_client`] so the TLS-pinning gate is in one place. With
//! pinning off (the shipped default while
//! `nexus_security::tls_pins::HOST_PINS` is empty) this is exactly
//! `reqwest::Client::new()` — no extra cost, no behaviour change. With
//! pinning on, the verifier from
//! [`nexus_security::tls::pinned_client_config`] is installed and any
//! handshake whose leaf cert doesn't match a configured pin is rejected
//! before request bytes are sent.

/// Build a `reqwest::Client` for outbound provider HTTPS, optionally
/// pinning the server certificate to one of the SHA-256 hashes in
/// [`nexus_security::tls_pins::HOST_PINS`]. A construction failure
/// (e.g. the rustls graph missing a crypto provider) falls back to a
/// stock client with a `tracing::warn!` so a misconfigured pin
/// pipeline never leaves the system without an HTTP client at all.
///
/// `NEXUS_TLS_PINNING=1` in the environment also enables pinning, so
/// operators can opt in without editing AI config — useful for the
/// shipped `KernelConfig::tls_pinning_enabled = false` default while
/// pins are being seeded.
#[must_use]
pub fn build_client(tls_pinning_enabled: bool) -> reqwest::Client {
    let env_opt_in = std::env::var("NEXUS_TLS_PINNING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if !tls_pinning_enabled && !env_opt_in {
        return reqwest::Client::new();
    }
    let cfg = nexus_security::tls::pinned_client_config();
    match reqwest::ClientBuilder::new()
        .use_preconfigured_tls(cfg)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "BL-102: failed to build TLS-pinned reqwest client; falling back to stock client",
            );
            reqwest::Client::new()
        }
    }
}
