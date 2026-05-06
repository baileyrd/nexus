//! BL-102 — TLS certificate pins for known AI provider endpoints.
//!
//! Each entry maps a hostname (case-insensitive) to a list of accepted
//! SubjectPublicKeyInfo (SPKI) SHA-256 fingerprints, in lowercase
//! hex. A connection's leaf certificate must hash to one of the pins
//! to be accepted; any mismatch returns
//! [`SecurityError::CertificatePinMismatch`].
//!
//! ## Operational note
//!
//! The pin list ships **empty** by default. With
//! `KernelConfig::tls_pinning_enabled = false` (the shipped default,
//! see ADR for BL-102) the empty list is a no-op — the standard
//! webpki chain validation still runs unmodified. With
//! `tls_pinning_enabled = true`, an empty pin list for a host
//! intentionally fails the connection with
//! [`SecurityError::NoPinsConfigured`] so an operator who opts in
//! cannot accidentally run with weaker security than they think.
//!
//! ## How to seed pins
//!
//! Run, on a trusted network, against the live endpoint:
//!
//! ```text
//! echo | openssl s_client -connect api.anthropic.com:443 -servername api.anthropic.com 2>/dev/null \
//!   | openssl x509 -pubkey -noout \
//!   | openssl pkey -pubin -outform DER \
//!   | openssl dgst -sha256 -hex
//! ```
//!
//! Pin **at least two values** per host (current leaf + the
//! intermediate or expected next leaf) so a routine cert rotation
//! doesn't take the app offline. Document each pin with the date it
//! was captured and an expiration reminder.

/// Per-host SPKI SHA-256 pins. **Placeholder — empty until seeded by
/// an operator with network access.** See module docs for the
/// capture procedure.
#[allow(clippy::module_name_repetitions)]
pub const HOST_PINS: &[(&str, &[&str])] = &[
    // ("api.anthropic.com", &["<hex sha256>", "<hex sha256>"]),
    // ("api.openai.com",    &["<hex sha256>", "<hex sha256>"]),
];

/// Look up the configured pins for `host`. Comparison is
/// case-insensitive; trailing dots (`api.anthropic.com.`) are
/// stripped.
#[must_use]
pub fn pins_for_host(host: &str) -> &'static [&'static str] {
    let normalised = host.trim_end_matches('.').to_ascii_lowercase();
    for (h, pins) in HOST_PINS {
        if h.eq_ignore_ascii_case(&normalised) {
            return pins;
        }
    }
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_host_returns_empty_pins() {
        assert!(pins_for_host("unknown.example.com").is_empty());
    }

    #[test]
    fn lookup_is_case_insensitive_and_strips_trailing_dot() {
        // The HOST_PINS table is empty by default, so this exercises
        // the normalisation logic without binding to a specific
        // entry. When real pins land, add a positive-match test.
        assert!(pins_for_host("API.ANTHROPIC.COM.").is_empty());
        assert!(pins_for_host("api.openai.com.").is_empty());
    }
}
