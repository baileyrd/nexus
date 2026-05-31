//! BL-099 — plugin manifest signing verification.
//!
//! Adds an opt-in cryptographic provenance layer to plugin manifests.
//! When a manifest carries a `[signature]` block, the loader verifies
//! that the named signer key signed the canonical manifest bytes (the
//! manifest with the `[signature]` block stripped). When
//! `KernelConfig::require_signatures = true` (default `false`), an
//! unsigned community plugin fails to load.
//!
//! ## File layout
//!
//! ```text
//! ~/.nexus/keys/community.json   # public key ring (KeyringFile below)
//! ~/.nexus/keys/revoked.json     # CRL (RevocationList below)
//! ```
//!
//! Both files are JSON arrays/lists; absence is equivalent to "no
//! trusted keys / no revocations" and produces a deterministic
//! `KeyNotTrusted` failure if a manifest references a missing key.
//!
//! ## Marketplace gate
//!
//! BL-099 is a marketplace prerequisite, not an end-user shipping
//! gate. Defaulting `require_signatures = false` keeps every existing
//! local-development flow working unchanged; community plugins
//! published through a future marketplace ship with `[signature]`
//! pre-populated, and operators flip the kernel-config flag once the
//! distribution channel exists.

use std::path::{Path, PathBuf};

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::manifest::PluginSignature;

/// Errors from signature verification.
#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    /// Manifest declares a signer key that isn't in `community.json`.
    #[error("signer key '{0}' is not in the trusted community keyring")]
    KeyNotTrusted(String),

    /// Signer key id appears in the revocation list.
    #[error("signer key '{0}' has been revoked")]
    KeyRevoked(String),

    /// Manifest declares an unknown signature algorithm.
    #[error("unsupported signature algorithm '{0}' (only 'ed25519' is supported)")]
    UnsupportedAlgorithm(String),

    /// Public key bytes are not a valid ed25519 key.
    #[error("malformed public key for '{key_id}': {reason}")]
    MalformedPublicKey {
        /// The signer key id whose public key failed to decode.
        key_id: String,
        /// Underlying decode reason.
        reason: String,
    },

    /// Signature bytes are not a valid ed25519 signature.
    #[error("malformed signature: {0}")]
    MalformedSignature(String),

    /// Signature did not verify against the canonical manifest bytes.
    #[error("signature verification failed for signer '{0}'")]
    VerificationFailed(String),

    /// Filesystem / JSON error reading the keyring or CRL.
    #[error("keyring i/o: {0}")]
    KeyringIo(String),

    /// `require_signatures = true` but the manifest carries no signature.
    #[error("plugin '{plugin_id}' has no signature but require_signatures is enabled")]
    SignatureRequired {
        /// The plugin id that failed the gate.
        plugin_id: String,
    },
}

/// One trusted signer entry (`community.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedKey {
    /// Reverse-DNS-style identifier (e.g. `com.example.author`).
    pub key_id: String,
    /// Base64-encoded ed25519 public key (32 bytes).
    pub public_key: String,
    /// Optional human-readable note for operators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Top-level shape of `~/.nexus/keys/community.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyringFile {
    /// Trusted signer entries.
    #[serde(default)]
    pub keys: Vec<TrustedKey>,
}

/// Top-level shape of `~/.nexus/keys/revoked.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevocationList {
    /// `key_id` values that should be rejected even if they appear
    /// in the keyring.
    #[serde(default)]
    pub revoked_key_ids: Vec<String>,
}

/// Plugin manifest signature verifier.
///
/// Construct via [`Self::with_keys_dir`] (production path) or
/// [`Self::from_inline_keys`] (tests). Reuse the verifier across
/// loads — keyring + CRL files are read once at construction.
pub struct PluginSignatureVerifier {
    keyring: KeyringFile,
    revocations: RevocationList,
}

impl PluginSignatureVerifier {
    /// Load the verifier from `<keys_dir>/community.json` and
    /// `<keys_dir>/revoked.json`. Missing files are treated as
    /// empty (no trusted keys / no revocations) — a manifest
    /// referencing a missing key still fails verification, just with
    /// a deterministic `KeyNotTrusted` instead of an i/o error.
    ///
    /// # Errors
    /// Returns `KeyringIo` on JSON parse failure or unreadable files.
    pub fn with_keys_dir(keys_dir: &Path) -> Result<Self, SignatureError> {
        let keyring = read_optional_json(&keys_dir.join("community.json"))?;
        let revocations = read_optional_json(&keys_dir.join("revoked.json"))?;
        Ok(Self {
            keyring,
            revocations,
        })
    }

    /// Equivalent of [`Self::with_keys_dir`] rooted at the user's
    /// `~/.nexus/keys` directory. Returns an empty verifier if the
    /// home directory cannot be resolved.
    #[must_use]
    pub fn from_user_home() -> Self {
        let dir = dirs::home_dir()
            .map(|h| h.join(".nexus").join("keys"))
            .unwrap_or_else(|| PathBuf::from(".nexus/keys"));
        Self::with_keys_dir(&dir).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "BL-099: failed to load community keyring; treating as empty");
            Self {
                keyring: KeyringFile::default(),
                revocations: RevocationList::default(),
            }
        })
    }

    /// Construct directly from inline keys (test-only and for the
    /// `nexus plugin verify` CLI command which lets the operator
    /// pass an explicit keyring path).
    #[must_use]
    pub fn from_inline_keys(keyring: KeyringFile, revocations: RevocationList) -> Self {
        Self {
            keyring,
            revocations,
        }
    }

    fn lookup_key(&self, key_id: &str) -> Result<VerifyingKey, SignatureError> {
        if self.revocations.revoked_key_ids.iter().any(|k| k == key_id) {
            return Err(SignatureError::KeyRevoked(key_id.to_string()));
        }
        let entry = self
            .keyring
            .keys
            .iter()
            .find(|k| k.key_id == key_id)
            .ok_or_else(|| SignatureError::KeyNotTrusted(key_id.to_string()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(entry.public_key.as_bytes())
            .map_err(|e| SignatureError::MalformedPublicKey {
                key_id: key_id.to_string(),
                reason: format!("base64: {e}"),
            })?;
        if bytes.len() != 32 {
            return Err(SignatureError::MalformedPublicKey {
                key_id: key_id.to_string(),
                reason: format!("expected 32 bytes, got {}", bytes.len()),
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        VerifyingKey::from_bytes(&arr).map_err(|e| SignatureError::MalformedPublicKey {
            key_id: key_id.to_string(),
            reason: e.to_string(),
        })
    }

    /// Verify that `signature` is a valid ed25519 signature by
    /// `signature.signer_key_id` over `canonical_manifest_bytes`.
    ///
    /// `canonical_manifest_bytes` should be the manifest with any
    /// `[signature]` block stripped (see
    /// [`canonicalize_manifest_for_signing`]).
    ///
    /// # Errors
    /// See [`SignatureError`] variants.
    pub fn verify(
        &self,
        canonical_manifest_bytes: &[u8],
        signature: &PluginSignature,
    ) -> Result<(), SignatureError> {
        if !signature.algorithm.eq_ignore_ascii_case("ed25519") {
            return Err(SignatureError::UnsupportedAlgorithm(
                signature.algorithm.clone(),
            ));
        }
        let key = self.lookup_key(&signature.signer_key_id)?;
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(signature.signature.as_bytes())
            .map_err(|e| SignatureError::MalformedSignature(format!("base64: {e}")))?;
        if sig_bytes.len() != 64 {
            return Err(SignatureError::MalformedSignature(format!(
                "expected 64 bytes, got {}",
                sig_bytes.len()
            )));
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let sig = Signature::from_bytes(&sig_arr);
        key.verify_strict(canonical_manifest_bytes, &sig)
            .map_err(|_| SignatureError::VerificationFailed(signature.signer_key_id.clone()))
    }
}

/// Strip the `[signature]` block from a TOML manifest string,
/// returning the canonical bytes that the signer signed. The
/// stripping rule: drop everything from the line that begins
/// `[signature]` (any whitespace tolerated) through the end of the
/// section (the next bare `[` line, or EOF).
///
/// The signer must produce signatures over exactly this canonical
/// form; any deviation (whitespace fiddling, key reordering inside
/// the signature block) does not affect verification because the
/// block is excluded entirely.
#[must_use]
pub fn canonicalize_manifest_for_signing(manifest_text: &str) -> String {
    let mut out = String::with_capacity(manifest_text.len());
    let mut skipping = false;
    for line in manifest_text.lines() {
        let trimmed = line.trim_start();
        if skipping {
            // End the skipped block at the next top-level section
            // header. Inline tables / array headers (e.g.
            // `[[registrations.ipc_command]]`) start with `[[`, which
            // is also a section boundary.
            if trimmed.starts_with('[') && !trimmed.starts_with("[signature]") {
                skipping = false;
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if trimmed.starts_with("[signature]") {
            skipping = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn read_optional_json<T: Default + serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, SignatureError> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| SignatureError::KeyringIo(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(SignatureError::KeyringIo(format!(
            "{}: {e}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn make_key() -> (SigningKey, String) {
        // Deterministic seed so the test is reproducible.
        let seed = [7u8; 32];
        let signing = SigningKey::from_bytes(&seed);
        let public = signing.verifying_key();
        let pk_b64 = base64::engine::general_purpose::STANDARD.encode(public.to_bytes());
        (signing, pk_b64)
    }

    fn verifier_with_key(key_id: &str, pk_b64: &str) -> PluginSignatureVerifier {
        PluginSignatureVerifier::from_inline_keys(
            KeyringFile {
                keys: vec![TrustedKey {
                    key_id: key_id.to_string(),
                    public_key: pk_b64.to_string(),
                    note: None,
                }],
            },
            RevocationList::default(),
        )
    }

    #[test]
    fn verify_round_trip_succeeds_on_canonical_bytes() {
        let (sk, pk_b64) = make_key();
        let key_id = "com.example.author";
        let v = verifier_with_key(key_id, &pk_b64);

        let manifest = b"id = \"com.example.x\"\nname = \"X\"\n";
        let sig = sk.sign(manifest);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());

        let plugin_sig = PluginSignature {
            algorithm: "ed25519".to_string(),
            signer_key_id: key_id.to_string(),
            signature: sig_b64,
        };
        v.verify(manifest, &plugin_sig).expect("verify");
    }

    #[test]
    fn verify_fails_on_tampered_manifest() {
        let (sk, pk_b64) = make_key();
        let key_id = "com.example.author";
        let v = verifier_with_key(key_id, &pk_b64);
        let manifest = b"original";
        let sig = sk.sign(manifest);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());

        let plugin_sig = PluginSignature {
            algorithm: "ed25519".to_string(),
            signer_key_id: key_id.to_string(),
            signature: sig_b64,
        };
        assert!(matches!(
            v.verify(b"tampered", &plugin_sig),
            Err(SignatureError::VerificationFailed(_))
        ));
    }

    #[test]
    fn untrusted_key_fails_loud() {
        let (sk, _pk_b64) = make_key();
        let v = PluginSignatureVerifier::from_inline_keys(
            KeyringFile::default(),
            RevocationList::default(),
        );
        let sig = sk.sign(b"any");
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let plugin_sig = PluginSignature {
            algorithm: "ed25519".to_string(),
            signer_key_id: "com.unknown".to_string(),
            signature: sig_b64,
        };
        assert!(matches!(
            v.verify(b"any", &plugin_sig),
            Err(SignatureError::KeyNotTrusted(_))
        ));
    }

    #[test]
    fn revoked_key_fails_loud_even_when_present_in_keyring() {
        let (_sk, pk_b64) = make_key();
        let key_id = "com.example.author";
        let v = PluginSignatureVerifier::from_inline_keys(
            KeyringFile {
                keys: vec![TrustedKey {
                    key_id: key_id.to_string(),
                    public_key: pk_b64,
                    note: None,
                }],
            },
            RevocationList {
                revoked_key_ids: vec![key_id.to_string()],
            },
        );
        let plugin_sig = PluginSignature {
            algorithm: "ed25519".to_string(),
            signer_key_id: key_id.to_string(),
            signature: "AAAA".to_string(),
        };
        assert!(matches!(
            v.verify(b"any", &plugin_sig),
            Err(SignatureError::KeyRevoked(_))
        ));
    }

    #[test]
    fn unsupported_algorithm_rejected_before_lookup() {
        let v = PluginSignatureVerifier::from_inline_keys(
            KeyringFile::default(),
            RevocationList::default(),
        );
        let plugin_sig = PluginSignature {
            algorithm: "rsa-pss".to_string(),
            signer_key_id: "k".to_string(),
            signature: "AAAA".to_string(),
        };
        assert!(matches!(
            v.verify(b"x", &plugin_sig),
            Err(SignatureError::UnsupportedAlgorithm(_))
        ));
    }

    #[test]
    fn canonicalize_strips_signature_block_only() {
        let manifest = "\
[plugin]
id = \"com.example.x\"
name = \"X\"

[signature]
algorithm = \"ed25519\"
signer_key_id = \"com.example.author\"
signature = \"AAA==\"

[capabilities]
required = []
";
        let canonical = canonicalize_manifest_for_signing(manifest);
        assert!(!canonical.contains("[signature]"));
        assert!(!canonical.contains("AAA=="));
        assert!(canonical.contains("[plugin]"));
        assert!(canonical.contains("[capabilities]"));
    }

    #[test]
    fn canonicalize_handles_signature_at_end_of_file() {
        let manifest = "[plugin]\nid = \"x\"\n\n[signature]\nalgorithm = \"ed25519\"\nsigner_key_id = \"a\"\nsignature = \"sig\"\n";
        let canonical = canonicalize_manifest_for_signing(manifest);
        assert!(!canonical.contains("[signature]"));
        assert!(canonical.contains("[plugin]"));
    }

    #[test]
    fn missing_keyring_files_yield_empty_verifier() {
        let dir = tempfile::tempdir().unwrap();
        let v = PluginSignatureVerifier::with_keys_dir(dir.path()).unwrap();
        let plugin_sig = PluginSignature {
            algorithm: "ed25519".to_string(),
            signer_key_id: "anything".to_string(),
            signature: "AAA=".to_string(),
        };
        assert!(matches!(
            v.verify(b"x", &plugin_sig),
            Err(SignatureError::KeyNotTrusted(_))
        ));
    }
}
