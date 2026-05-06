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
        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)?;

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
