//! BL-099 — end-to-end manifest signing tests via the loader.

use std::fs;
use std::sync::Arc;

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use nexus_plugins::signing::{
    canonicalize_manifest_for_signing, KeyringFile, PluginSignatureVerifier, RevocationList,
    TrustedKey,
};
use nexus_plugins::PluginLoader;

fn fixed_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[42u8; 32])
}

fn make_keyring(key_id: &str, sk: &SigningKey) -> KeyringFile {
    let pk = sk.verifying_key();
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk.to_bytes());
    KeyringFile {
        keys: vec![TrustedKey {
            key_id: key_id.to_string(),
            public_key: pk_b64,
            note: None,
        }],
    }
}

fn unsigned_community_manifest(id: &str) -> String {
    format!(
        r#"
[plugin]
id = "{id}"
name = "Sig Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "plugin.wasm"
memory_mb = 16
fuel = 1000000
max_execution_ms = 5000
"#
    )
}

fn write_signed_manifest(dir: &std::path::Path, id: &str, key_id: &str, sk: &SigningKey) {
    // Build the file with a placeholder signature, canonicalize the
    // *whole file*, then sign that canonical form. This is exactly
    // what the verifier on the read side does, so the two sides
    // operate on identical bytes regardless of how `canonicalize`
    // handles whitespace around the [signature] block.
    let unsigned = unsigned_community_manifest(id);
    let with_placeholder = format!(
        "{unsigned}\n[signature]\nalgorithm = \"ed25519\"\nsigner_key_id = \"{key_id}\"\nsignature = \"placeholder\"\n"
    );
    let canonical = canonicalize_manifest_for_signing(&with_placeholder);
    let sig = sk.sign(canonical.as_bytes());
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
    let final_text = format!(
        "{unsigned}\n[signature]\nalgorithm = \"ed25519\"\nsigner_key_id = \"{key_id}\"\nsignature = \"{sig_b64}\"\n"
    );
    fs::write(dir.join("manifest.toml"), final_text).expect("write manifest");
    fs::write(dir.join("plugin.wasm"), &[0u8; 0]).expect("write wasm");
}

#[test]
fn signed_manifest_verifies_with_trusted_key() {
    let key_id = "com.example.author";
    let sk = fixed_signing_key();
    let plugin_dir = tempfile::tempdir().expect("tempdir");
    write_signed_manifest(plugin_dir.path(), "com.example.signed", key_id, &sk);

    let raw = fs::read_to_string(plugin_dir.path().join("manifest.toml")).unwrap();
    let canonical = canonicalize_manifest_for_signing(&raw);
    let manifest = nexus_plugins::parse_manifest(&raw, "manifest.toml").expect("parse");
    let sig = manifest.signature.as_ref().expect("signature parsed");

    let verifier = PluginSignatureVerifier::from_inline_keys(
        make_keyring(key_id, &sk),
        RevocationList::default(),
    );
    verifier
        .verify(canonical.as_bytes(), sig)
        .expect("signature should verify against the canonical bytes");
}

#[test]
fn loader_rejects_unsigned_when_require_signatures_is_on() {
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path().join("unsigned");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("manifest.toml"),
        unsigned_community_manifest("com.example.unsigned"),
    )
    .unwrap();
    fs::write(plugin_dir.join("plugin.wasm"), &[0u8; 0]).unwrap();

    let mut loader = PluginLoader::new(plugins_dir.path());
    loader.set_require_signatures(true);

    let err = loader
        .load(&plugin_dir)
        .expect_err("load should fail without a signature when require_signatures is on");
    let msg = format!("{err}");
    assert!(
        msg.contains("BL-099") && msg.contains("no signature"),
        "expected BL-099 SignatureRequired-style error, got: {msg}"
    );
}

#[test]
fn loader_rejects_signed_manifest_with_untrusted_key() {
    // Off-keyring signer — the loader's default verifier reads
    // ~/.nexus/keys/community.json which is empty in CI, so a
    // signature signed by a key the loader doesn't know fails
    // even with require_signatures = false (the rule is "if a
    // signature is present, it must verify").
    let key_id = "com.example.rogue";
    let sk = fixed_signing_key();
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path().join("signed-by-rogue");
    fs::create_dir_all(&plugin_dir).unwrap();
    write_signed_manifest(&plugin_dir, "com.example.rogue.plugin", key_id, &sk);

    let mut loader = PluginLoader::new(plugins_dir.path());
    // Install an explicit empty verifier so we don't depend on the
    // user's home directory in CI.
    loader.set_signature_verifier(Some(Arc::new(PluginSignatureVerifier::from_inline_keys(
        KeyringFile::default(),
        RevocationList::default(),
    ))));

    let err = loader
        .load(&plugin_dir)
        .expect_err("load should fail when the signer key isn't trusted");
    let msg = format!("{err}");
    assert!(
        msg.contains("BL-099") && msg.contains("not in the trusted"),
        "expected BL-099 KeyNotTrusted error, got: {msg}"
    );
}

#[test]
fn loader_accepts_signed_manifest_with_trusted_key() {
    let key_id = "com.example.trusted";
    let sk = fixed_signing_key();
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path().join("signed-trusted");
    fs::create_dir_all(&plugin_dir).unwrap();
    write_signed_manifest(&plugin_dir, "com.example.trusted.plugin", key_id, &sk);

    let mut loader = PluginLoader::new(plugins_dir.path());
    loader.set_signature_verifier(Some(Arc::new(PluginSignatureVerifier::from_inline_keys(
        make_keyring(key_id, &sk),
        RevocationList::default(),
    ))));

    // We expect signature verification to PASS. The load may still
    // fail downstream (wasm bytes are empty) but that error must be
    // distinct from a BL-099 signature failure.
    let result = loader.load(&plugin_dir);
    if let Err(e) = result {
        let msg = format!("{e}");
        assert!(
            !msg.contains("BL-099"),
            "signature verification should succeed; BL-099 leaked through: {msg}"
        );
    }
}
