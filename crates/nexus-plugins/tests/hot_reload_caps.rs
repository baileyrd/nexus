//! Regression test for issue #74 — hot-reload silently preserved the
//! previously-loaded `CapabilitySet` instead of re-evaluating
//! `granted_caps.json`.
//!
//! Before the fix, an operator who edited `granted_caps.json` to
//! revoke a HIGH-risk capability and then triggered a reload (e.g. by
//! re-saving the `.wasm`) kept the *old* grant on the new sandbox.
//! The denial only took effect at full process restart — exactly the
//! "edit grants → reload to deny" flow operators expect to work.
//!
//! The fix: `PluginManager::reload_plugin` now calls
//! `PluginLoader::refresh_capabilities`, which re-runs
//! `build_capabilities(manifest, plugin_dir)` and re-reads
//! `granted_caps.json`. This integration test loads a plugin with a
//! HIGH-risk cap granted, revokes it on disk, triggers reload, and
//! asserts the cap is gone from the loader's cached `PluginInfo`.

use std::path::Path;

use nexus_plugins::{PluginManager, PluginManagerConfig};

const HIGH_RISK_CAP: &str = "net.http";

fn no_reload_config() -> PluginManagerConfig {
    PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    }
}

fn write_granted_caps(plugin_dir: &Path, version: &str, granted: &[&str]) {
    let granted_strs: Vec<String> = granted.iter().map(|s| (*s).to_string()).collect();
    let body = serde_json::json!({
        "version": version,
        "granted": granted_strs,
    });
    std::fs::write(
        plugin_dir.join("granted_caps.json"),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
}

fn setup_plugin_with_cap(
    plugin_id: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join(plugin_id);
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let wasm_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal-plugin.wasm");
    let wasm_dst = plugin_dir.join("test.wasm");
    std::fs::copy(&wasm_src, &wasm_dst).unwrap();

    let manifest = format!(
        r#"
[plugin]
id = "{plugin_id}"
name = "Hot-reload Cap Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["{HIGH_RISK_CAP}"]

[wasm]
module = "test.wasm"

[lifecycle]
on_init = false
on_start = false
on_stop = false
"#
    );
    std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

    (tmp, plugin_dir, wasm_dst)
}

#[test]
fn hot_reload_picks_up_revoked_high_risk_cap() {
    let (tmp, plugin_dir, wasm_path) = setup_plugin_with_cap("com.test.hot_reload_revoke");

    // 1. Grant the HIGH-risk cap, then load.
    write_granted_caps(&plugin_dir, "1.0.0", &[HIGH_RISK_CAP]);
    let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
    let info = mgr
        .load(&plugin_dir)
        .expect("load with grant should succeed");
    assert!(
        info.capabilities
            .iter()
            .any(|c| c.as_str() == HIGH_RISK_CAP),
        "pre-reload sandbox must have the granted HIGH-risk cap"
    );

    // 2. Revoke on disk.
    write_granted_caps(&plugin_dir, "1.0.0", &[]);

    // 3. Trigger reload.
    mgr.reload_plugin("com.test.hot_reload_revoke", &wasm_path)
        .expect("reload should succeed");

    // 4. Reloaded plugin must NOT carry the revoked cap forward.
    let post = mgr
        .get("com.test.hot_reload_revoke")
        .expect("plugin still loaded after reload");
    assert!(
        !post
            .capabilities
            .iter()
            .any(|c| c.as_str() == HIGH_RISK_CAP),
        "post-reload sandbox must not retain the revoked HIGH-risk cap; \
         caps were: {:?}",
        post.capabilities
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn hot_reload_picks_up_granted_high_risk_cap() {
    // The reverse: a previously-denied HIGH-risk cap that the operator
    // grants between load and reload must take effect on reload — same
    // mechanism, opposite direction. Confirms the helper re-reads disk
    // rather than using a one-way "subtract revoked" shortcut.
    let (tmp, plugin_dir, wasm_path) = setup_plugin_with_cap("com.test.hot_reload_grant");

    // 1. Load with no grants — HIGH-risk cap should be filtered out.
    write_granted_caps(&plugin_dir, "1.0.0", &[]);
    let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();
    let info = mgr
        .load(&plugin_dir)
        .expect("load without grant should succeed");
    assert!(
        !info
            .capabilities
            .iter()
            .any(|c| c.as_str() == HIGH_RISK_CAP),
        "pre-reload sandbox must NOT have the ungranted HIGH-risk cap"
    );

    // 2. Grant on disk.
    write_granted_caps(&plugin_dir, "1.0.0", &[HIGH_RISK_CAP]);

    // 3. Trigger reload.
    mgr.reload_plugin("com.test.hot_reload_grant", &wasm_path)
        .expect("reload should succeed");

    // 4. Reloaded plugin must now carry the freshly-granted cap.
    let post = mgr
        .get("com.test.hot_reload_grant")
        .expect("plugin still loaded after reload");
    assert!(
        post.capabilities
            .iter()
            .any(|c| c.as_str() == HIGH_RISK_CAP),
        "post-reload sandbox must respect the freshly-granted HIGH-risk cap"
    );
}
