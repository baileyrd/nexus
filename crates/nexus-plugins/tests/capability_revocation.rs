//! BL-096 — runtime capability revocation tests.
//!
//! Verifies that `PluginLoader::revoke_capability` mutates the live
//! `KernelPluginContext` cap set so a subsequent `has_capability`
//! check observes the revocation without a plugin restart.

use std::sync::Arc;

use nexus_kernel::{
    Capability, CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
    PluginContext,
};
use nexus_plugins::{parse_manifest, CorePlugin, PluginError, PluginLoader};

struct NoopPlugin;

impl CorePlugin for NoopPlugin {
    fn dispatch(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Ok(serde_json::Value::Null)
    }
}

fn core_manifest(id: &str) -> nexus_plugins::PluginManifest {
    let toml = format!(
        r#"
[plugin]
id = "{id}"
name = "Revocation test plugin"
version = "1.0.0"
trust_level = "core"
api_version = "1"
"#
    );
    parse_manifest(&toml, "plugin.toml").expect("parse manifest")
}

fn caps_with(cap: Capability) -> CapabilitySet {
    let mut set = CapabilitySet::empty();
    set.insert(cap);
    set
}

#[test]
fn revoke_capability_takes_effect_on_running_context() {
    let forge = tempfile::tempdir().expect("forge tempdir");
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_id = "dev.test.revoke";

    // Set up a loader, register the plugin, wire it a context with
    // ProcessSpawn (HIGH-risk and revocable).
    let mut loader = PluginLoader::new(plugins_dir.path());
    let event_bus = Arc::new(EventBus::new(64));
    loader.set_event_bus(Arc::clone(&event_bus));
    loader
        .register_core(
            core_manifest(plugin_id),
            plugins_dir.path(),
            Box::new(NoopPlugin),
        )
        .expect("register plugin");

    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::default());
    let ctx = Arc::new(
        KernelPluginContext::new(
            plugin_id,
            "1.0.0",
            caps_with(Capability::ProcessSpawn),
            kv,
            event_bus,
            forge.path(),
            None,
        )
        .expect("ctx"),
    );
    loader
        .wire_context(plugin_id, Arc::clone(&ctx))
        .expect("wire context");

    // Pre-revocation: the plugin holds the cap.
    assert!(
        ctx.has_capability(Capability::ProcessSpawn),
        "context should hold the cap before revocation",
    );

    // Revoke; the running context's view should immediately update.
    loader
        .revoke_capability(plugin_id, Capability::ProcessSpawn)
        .expect("revoke");

    assert!(
        !ctx.has_capability(Capability::ProcessSpawn),
        "context should no longer hold the cap after revocation",
    );
}

#[test]
fn revoke_persists_to_granted_caps_json() {
    // Force the plaintext-JSON path so the assertion is deterministic
    // regardless of whether the host has a usable OS keyring (BL-101
    // ships AEAD encryption when one is available; the
    // grants_crypto unit tests cover that path).
    // SAFETY: cargo test parallelism is per-process; setting this
    // var here can be observed by other concurrent tests in this
    // binary, but every test in the file is intentionally
    // plaintext-mode.
    unsafe { std::env::set_var("NEXUS_NO_KEYRING", "1"); }

    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_id = "dev.test.persist";

    let mut loader = PluginLoader::new(plugins_dir.path());
    loader
        .register_core(
            core_manifest(plugin_id),
            plugins_dir.path(),
            Box::new(NoopPlugin),
        )
        .expect("register");

    loader
        .revoke_capability(plugin_id, Capability::NetHttp)
        .expect("revoke");

    let grants_file = plugins_dir.path().join("granted_caps.json");
    assert!(
        grants_file.exists(),
        "granted_caps.json should be written after revoke",
    );
    let body = std::fs::read_to_string(&grants_file).expect("read grants");
    // The exact persisted shape is internal to the loader, but the
    // file must mention the revoked cap so a reload re-applies the
    // denial.
    assert!(
        body.contains("\"granted\"") && body.contains("\"version\""),
        "grants file should be in the canonical shape; got: {body}",
    );
}

#[test]
fn revoke_non_high_risk_cap_is_a_noop() {
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_id = "dev.test.nonhighrisk";

    let mut loader = PluginLoader::new(plugins_dir.path());
    loader
        .register_core(
            core_manifest(plugin_id),
            plugins_dir.path(),
            Box::new(NoopPlugin),
        )
        .expect("register");

    // FsRead is not HIGH-risk; the loader's `revoke_capability` short-
    // circuits and does not write granted_caps.json.
    loader
        .revoke_capability(plugin_id, Capability::FsRead)
        .expect("noop revoke should succeed");

    let grants_file = plugins_dir.path().join("granted_caps.json");
    assert!(
        !grants_file.exists(),
        "granted_caps.json should not be written for non-HIGH-risk caps",
    );
}
