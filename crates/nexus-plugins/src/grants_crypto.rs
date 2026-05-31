//! BL-101 — at-rest encryption for `granted_caps.json`.
//!
//! The plain-JSON file recorded which HIGH-risk capabilities the user
//! had consented to grant a community plugin. A user (or malware with
//! file access) could trivially edit it to grant `process.spawn` /
//! `net.http` / etc. to any plugin without going through the consent
//! dialog. This module wraps grants in a ChaCha20-Poly1305 AEAD blob
//! whose key lives in the OS keyring, so a renderer-side hand-edit no
//! longer translates into silent privilege escalation.
//!
//! ## File format
//!
//! Encrypted blobs start with a 6-byte magic header (`b"NXENC1"`),
//! followed by a 12-byte nonce, then the AEAD ciphertext + 16-byte
//! tag. The whole thing is written verbatim to `granted_caps.json`
//! (no base64 wrapping — the contents are intentionally not
//! human-editable).
//!
//! Plaintext fallback (`NEXUS_NO_KEYRING=1`) writes the legacy JSON
//! shape with a leading `{`. Reads dispatch on the first byte: `N`
//! → encrypted; `{` → plaintext.
//!
//! ## Key management
//!
//! The 32-byte master key is stored under
//! `nexus.plugin_grants:<plugin_id>` in the OS keyring (one key per
//! plugin), encoded as 64 hex characters. The first encrypted write
//! generates a fresh random key and stores it; subsequent reads /
//! writes fetch it. Removing the keyring entry effectively rotates the
//! key — the next read fails to authenticate, the loader logs a
//! warning, and grants reset to deny-all (per the BL-101 DoD: forces
//! re-consent).

use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, ChaCha20Poly1305, Key, Nonce,
};

const MAGIC: &[u8; 6] = b"NXENC1";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const KEYRING_SERVICE: &str = "nexus.plugin_grants";

/// True when `NEXUS_NO_KEYRING=1` is set in the environment. Plain-text
/// mode skips encryption with a `tracing::warn!` so operators see the
/// reduced security level in logs.
fn keyring_disabled() -> bool {
    std::env::var("NEXUS_NO_KEYRING")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Detect the file's on-disk format from the leading bytes. Avoids a
/// version field at the top of the encrypted blob — the magic header
/// itself is the version marker.
pub(crate) fn looks_encrypted(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() && &bytes[..MAGIC.len()] == MAGIC
}

/// Encryption settings for a single plugin's grants file. Keyed by
/// plugin id so two plugins cannot impersonate each other's grants
/// even if their files are swapped.
pub(crate) struct GrantsKey {
    plugin_id: String,
    /// Override for tests. Production uses the keyring.
    test_only_key: Option<[u8; KEY_LEN]>,
}

impl GrantsKey {
    pub(crate) fn for_plugin(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            test_only_key: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_key(plugin_id: impl Into<String>, key: [u8; KEY_LEN]) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            test_only_key: Some(key),
        }
    }

    /// Fetch the per-plugin master key from the OS keyring, generating
    /// a fresh random one on first use. Returns `None` when the
    /// keyring is unreachable or `NEXUS_NO_KEYRING=1` is set — callers
    /// fall back to plain-text writes in that case.
    fn fetch_key(&self) -> Option<[u8; KEY_LEN]> {
        if let Some(test) = self.test_only_key {
            return Some(test);
        }
        if keyring_disabled() {
            return None;
        }
        let entry = match keyring::Entry::new(KEYRING_SERVICE, &self.plugin_id) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    plugin_id = %self.plugin_id,
                    "BL-101: keyring entry construction failed; falling back to plaintext grants",
                );
                return None;
            }
        };
        match entry.get_password() {
            Ok(hex) => decode_hex_key(&hex).or_else(|| {
                tracing::warn!(
                    plugin_id = %self.plugin_id,
                    "BL-101: keyring entry malformed; rotating to a fresh key",
                );
                self.generate_and_store(&entry)
            }),
            Err(keyring::Error::NoEntry) => self.generate_and_store(&entry),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    plugin_id = %self.plugin_id,
                    "BL-101: keyring read failed; falling back to plaintext grants",
                );
                None
            }
        }
    }

    fn generate_and_store(&self, entry: &keyring::Entry) -> Option<[u8; KEY_LEN]> {
        let mut key = [0u8; KEY_LEN];
        if let Err(e) = OsRng.try_fill_bytes(&mut key) {
            tracing::warn!(
                error = %e,
                plugin_id = %self.plugin_id,
                "BL-101: OsRng failed; cannot generate grants key",
            );
            return None;
        }
        let hex = encode_hex_key(&key);
        if let Err(e) = entry.set_password(&hex) {
            tracing::warn!(
                error = %e,
                plugin_id = %self.plugin_id,
                "BL-101: keyring write failed; falling back to plaintext grants",
            );
            return None;
        }
        Some(key)
    }
}

/// Encrypt `plaintext` using the plugin-scoped key. Returns `None`
/// when the keyring path is unavailable; the caller should fall back
/// to plaintext + a `tracing::warn!`.
pub(crate) fn encrypt_blob(key_src: &GrantsKey, plaintext: &[u8]) -> Option<Vec<u8>> {
    let key_bytes = key_src.fetch_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher.encrypt(&nonce, plaintext).ok()?;

    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Some(out)
}

/// Reverse of [`encrypt_blob`]. Returns `None` for any failure
/// (truncated header, missing keyring, AEAD authentication failure).
/// Callers treat that as deny-all and emit a warning per the BL-101
/// DoD ("decryption failure → log warning + clear grants").
pub(crate) fn decrypt_blob(key_src: &GrantsKey, blob: &[u8]) -> Option<Vec<u8>> {
    if !looks_encrypted(blob) {
        return None;
    }
    if blob.len() < MAGIC.len() + NONCE_LEN {
        return None;
    }
    let key_bytes = key_src.fetch_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    let nonce_bytes = &blob[MAGIC.len()..MAGIC.len() + NONCE_LEN];
    let nonce = Nonce::from_slice(nonce_bytes);
    let ciphertext = &blob[MAGIC.len() + NONCE_LEN..];
    cipher.decrypt(nonce, ciphertext).ok()
}

fn encode_hex_key(bytes: &[u8; KEY_LEN]) -> String {
    let mut s = String::with_capacity(KEY_LEN * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn decode_hex_key(s: &str) -> Option<[u8; KEY_LEN]> {
    if s.len() != KEY_LEN * 2 {
        return None;
    }
    let mut out = [0u8; KEY_LEN];
    for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Whether the keyring path is currently usable for the plugin. Used
/// by `write_grant` to decide between encrypted and plaintext output.
pub(crate) fn key_available(key_src: &GrantsKey) -> bool {
    key_src.fetch_key().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_key() -> [u8; KEY_LEN] {
        let mut k = [0u8; KEY_LEN];
        for (i, slot) in k.iter_mut().enumerate() {
            *slot = (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        k
    }

    #[test]
    fn roundtrip_plaintext_recovered_after_decrypt() {
        let key = GrantsKey::with_test_key("dev.test", fixed_key());
        let payload = br#"{"version":"1.0.0","granted":["process.spawn"]}"#;
        let blob = encrypt_blob(&key, payload).expect("encrypt");
        assert!(looks_encrypted(&blob));
        let recovered = decrypt_blob(&key, &blob).expect("decrypt");
        assert_eq!(recovered.as_slice(), payload.as_slice());
    }

    #[test]
    fn looks_encrypted_rejects_json_prefix() {
        let json = b"{ \"version\": \"1.0.0\" }";
        assert!(!looks_encrypted(json));
    }

    #[test]
    fn decrypt_returns_none_on_tampered_ciphertext() {
        let key = GrantsKey::with_test_key("dev.test", fixed_key());
        let mut blob = encrypt_blob(&key, b"{}").expect("encrypt");
        // Flip a byte in the ciphertext region.
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(decrypt_blob(&key, &blob).is_none());
    }

    #[test]
    fn decrypt_returns_none_on_wrong_key() {
        let payload = b"{}";
        let blob = encrypt_blob(&GrantsKey::with_test_key("dev.test", fixed_key()), payload)
            .expect("encrypt");
        let mut other_key = fixed_key();
        other_key[0] ^= 0x01;
        let wrong = GrantsKey::with_test_key("dev.test", other_key);
        assert!(decrypt_blob(&wrong, &blob).is_none());
    }

    #[test]
    fn decrypt_short_blob_is_safe() {
        let key = GrantsKey::with_test_key("dev.test", fixed_key());
        assert!(decrypt_blob(&key, b"NXENC1").is_none());
        assert!(decrypt_blob(&key, b"NXENC1\x00\x01").is_none());
    }

    #[test]
    fn hex_codec_roundtrips() {
        let k = fixed_key();
        let s = encode_hex_key(&k);
        assert_eq!(s.len(), KEY_LEN * 2);
        let back = decode_hex_key(&s).expect("decode");
        assert_eq!(back, k);
    }
}
