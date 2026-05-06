//! BL-095 — lifecycle hook timeout watchdog tests.
//!
//! Verifies that `register_core` does not block bootstrap indefinitely
//! when a plugin's `on_init` hook hangs, and that a plugin returning
//! quickly is unaffected.

use std::time::{Duration, Instant};

use nexus_plugins::{parse_manifest, CorePlugin, PluginError, PluginLoader};

struct HangPlugin {
    sleep_for: Duration,
}

impl CorePlugin for HangPlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        std::thread::sleep(self.sleep_for);
        Ok(())
    }

    fn dispatch(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Ok(serde_json::Value::Null)
    }
}

fn manifest_with_init(id: &str) -> nexus_plugins::PluginManifest {
    let toml = format!(
        r#"
[plugin]
id = "{id}"
name = "Lifecycle timeout test"
version = "1.0.0"
trust_level = "core"
api_version = "1"

[lifecycle]
on_init = true
"#
    );
    parse_manifest(&toml, "plugin.toml").expect("parse manifest")
}

#[test]
fn register_core_aborts_when_on_init_exceeds_deadline() {
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let mut loader = PluginLoader::new(plugins_dir.path());
    loader.set_lifecycle_timeout(Duration::from_millis(200));

    let started = Instant::now();
    let result = loader.register_core(
        manifest_with_init("dev.test.hang"),
        plugins_dir.path(),
        Box::new(HangPlugin { sleep_for: Duration::from_secs(60) }),
    );
    let elapsed = started.elapsed();

    match result {
        Err(PluginError::LifecycleTimeout { plugin_id, hook, timeout_secs: _ }) => {
            assert_eq!(plugin_id, "dev.test.hang");
            assert_eq!(hook, "init");
        }
        other => panic!("expected LifecycleTimeout, got {other:?}"),
    }

    // The deadline is 200ms; we should be back in well under the
    // 60s sleep. Use 5s as a conservative ceiling so this passes
    // on slow CI without masking a regression.
    assert!(
        elapsed < Duration::from_secs(5),
        "register_core blocked for {elapsed:?} despite 200ms timeout"
    );
}

#[test]
fn register_core_succeeds_when_on_init_is_fast() {
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let mut loader = PluginLoader::new(plugins_dir.path());
    loader.set_lifecycle_timeout(Duration::from_secs(5));

    loader
        .register_core(
            manifest_with_init("dev.test.fast"),
            plugins_dir.path(),
            Box::new(HangPlugin { sleep_for: Duration::from_millis(10) }),
        )
        .expect("fast on_init should not time out");
}

#[test]
fn zero_timeout_disables_watchdog_and_runs_inline() {
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let mut loader = PluginLoader::new(plugins_dir.path());
    loader.set_lifecycle_timeout(Duration::ZERO);

    // With the watchdog disabled, even a slow hook eventually returns
    // (we keep it short so the test stays fast).
    loader
        .register_core(
            manifest_with_init("dev.test.inline"),
            plugins_dir.path(),
            Box::new(HangPlugin { sleep_for: Duration::from_millis(50) }),
        )
        .expect("inline path should still complete");
}
