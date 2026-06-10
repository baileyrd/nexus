//! BL-102 — TLS pinning verifier for outbound HTTPS connections.
//!
//! Wraps `rustls::client::WebPkiServerVerifier` so that, when
//! `tls_pinning_enabled` is on, every TLS handshake to a known host
//! must present a leaf certificate whose SHA-256 hash matches one of
//! the pins configured in [`crate::tls_pins`]. Mismatch returns
//! `rustls::Error::General`, which `reqwest` surfaces as a
//! connection error before any request bytes leave the client.
//!
//! ## Hash domain
//!
//! The shipped implementation pins the **full leaf certificate DER**
//! (`SHA-256(cert_bytes)`). The PRD's original wording referenced
//! SPKI pinning, which is more resilient to certificate renewal when
//! the keypair is reused. The full-cert hash was chosen here because
//! it requires no ASN.1 parsing — switching to SPKI is a planned
//! follow-up once an operator has captured live fingerprints and
//! validated the rotation cadence. The on-disk pin format
//! (`tls_pins.rs`) is the same in either case (lowercase hex
//! SHA-256), so the migration is a one-line change in
//! [`leaf_fingerprint`] plus re-seeding the pins file.
//!
//! ## Default-off shipping posture
//!
//! `KernelConfig::tls_pinning_enabled` defaults to `false`. With the
//! flag off no verifier is installed and behaviour is identical to a
//! stock reqwest client. With the flag on, an empty pin list for the
//! target host is *not* treated as "no pinning" — it fails the
//! connection with [`rustls::Error::General`] so an operator who
//! opts in cannot accidentally run with weaker security than they
//! think.

use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::tls_pins;

/// Pinned-certificate `ServerCertVerifier` (BL-102).
///
/// Delegates chain validation to the standard `WebPkiServerVerifier`
/// (Mozilla root store via `webpki-roots`); on success, additionally
/// requires the leaf certificate's SHA-256 to match one of the
/// configured pins for the requested hostname.
#[derive(Debug)]
pub struct PinnedServerCertVerifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl PinnedServerCertVerifier {
    /// Construct a pinning verifier seeded with the standard
    /// Mozilla root store.
    ///
    /// # Panics
    /// Panics if the rustls webpki builder rejects the empty
    /// crypto provider — should not occur with the `ring`
    /// feature enabled in `Cargo.toml`.
    #[must_use]
    pub fn new_with_webpki_roots() -> Arc<Self> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let inner = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .expect("rustls webpki verifier build");
        Arc::new(Self { inner })
    }
}

/// Lowercase hex SHA-256 of `bytes`.
fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Hash domain for pin comparison. Switching SPKI pinning later only
/// requires changing this function.
fn leaf_fingerprint(cert: &CertificateDer<'_>) -> String {
    hex_sha256(cert.as_ref())
}

impl ServerCertVerifier for PinnedServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        // Standard webpki chain validation first — pinning is in
        // *addition to* CA trust, never a replacement for it.
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;

        let host = match server_name {
            ServerName::DnsName(d) => d.as_ref().to_string(),
            other => {
                return Err(Error::General(format!(
                    "BL-102: TLS pinning only supports DNS server names, got {other:?}"
                )));
            }
        };

        let pins = tls_pins::pins_for_host(&host);
        if pins.is_empty() {
            return Err(Error::General(format!(
                "BL-102: TLS pinning is enabled but no pins are configured for {host}"
            )));
        }

        let actual = leaf_fingerprint(end_entity);
        if pins.iter().any(|p| p.eq_ignore_ascii_case(&actual)) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General(format!(
                "BL-102: TLS pin mismatch for {host}: actual={actual} expected_one_of={pins:?}"
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Build a `rustls::ClientConfig` that pins outbound connections to
/// the [`tls_pins::HOST_PINS`] table. Hand the result to
/// `reqwest::ClientBuilder::use_preconfigured_tls`.
///
/// Installs `ring` as the process-wide default crypto provider on
/// first call (idempotent — `install_default` is best-effort and a
/// duplicate install is silently ignored). This avoids the
/// `CryptoProvider not selected` panic on builds where multiple
/// rustls feature combinations land in the dep graph.
#[must_use]
pub fn pinned_client_config() -> rustls::ClientConfig {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let verifier = PinnedServerCertVerifier::new_with_webpki_roots();
    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth()
}

/// TCP connect deadline for outbound provider HTTPS (V4,
/// `repo-review-2026-06-10.md`). Without one, a half-open connection
/// hangs the caller until the OS TCP timeout fires — minutes. This is
/// the load-bearing timeout; keep it tight.
pub const OUTBOUND_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-read-operation deadline for outbound provider HTTPS (V4).
/// Deliberately generous: streaming chat completions emit periodic
/// SSE chunks well inside this window, but non-streaming work
/// (audio STT on long files) can sit silent for minutes before the
/// body arrives. This is a hang backstop, not a latency budget — an
/// overall `.timeout()` would cut off long streamed generations and
/// is intentionally *not* set here.
pub const OUTBOUND_READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Base outbound client builder: timeouts only, no TLS customisation.
fn outbound_builder() -> reqwest::ClientBuilder {
    reqwest::ClientBuilder::new()
        .connect_timeout(OUTBOUND_CONNECT_TIMEOUT)
        .read_timeout(OUTBOUND_READ_TIMEOUT)
}

/// Build an outbound `reqwest::Client`, optionally with TLS pinning
/// enabled via [`pinned_client_config`]. Both variants carry
/// [`OUTBOUND_CONNECT_TIMEOUT`] and [`OUTBOUND_READ_TIMEOUT`].
///
/// `NEXUS_TLS_PINNING=1` in the environment also enables pinning
/// regardless of the flag — useful when operators want to opt in
/// without editing every per-subsystem config.
///
/// A construction failure (e.g. the rustls graph missing a crypto
/// provider) falls back to a stock client with a `tracing::warn!`
/// so a misconfigured pin pipeline never leaves the caller without
/// an HTTP client at all. Shared by `nexus-ai` and `nexus-audio`
/// so chat + audio funnel through one pin policy.
#[must_use]
pub fn build_pinned_client(tls_pinning_enabled: bool) -> reqwest::Client {
    let env_opt_in = std::env::var("NEXUS_TLS_PINNING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if !tls_pinning_enabled && !env_opt_in {
        return match outbound_builder().build() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "V4: failed to build outbound reqwest client with timeouts; falling back to stock client",
                );
                reqwest::Client::new()
            }
        };
    }
    let cfg = pinned_client_config();
    match outbound_builder().use_preconfigured_tls(cfg).build() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_sha256_lowercase_hex_zero_padded() {
        let s = hex_sha256(b"");
        assert_eq!(
            s,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn pinned_client_config_constructs_without_panic() {
        // Smoke: the builder must succeed with the bundled
        // webpki-roots + ring crypto provider.
        let _cfg = pinned_client_config();
    }
}
