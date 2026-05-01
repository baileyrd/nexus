//! Audit-2026-05-01 P1-1: end-to-end proof that the WASM sandbox denies
//! `host::read_file` to a plugin without `Capability::FsRead`, and emits
//! a structured `capability denied` audit event when it does so.
//!
//! The capability gate at
//! `crates/nexus-plugins/src/host_fns.rs:636-638` was previously verified
//! by hand reading. This test locks the property end-to-end so a future
//! refactor that drops the `caller.data().capabilities.contains(...)`
//! check fails CI.
//!
//! Companion positive test confirms the gate isn't accidentally
//! denying a plugin that *does* hold the capability — without it, a
//! regression that always-denies would still pass the negative test.
//!
//! ## Why the lower-level wasmtime API instead of [`WasmSandbox`]?
//!
//! `WasmSandbox::dispatch` requires the WASM module to implement the
//! `nexus_alloc` / `nexus_dispatch` ABI and JSON-encode return values.
//! For a single-function denial probe that returns an i32, building
//! the wasmtime `Engine` / `Linker` / `Instance` directly is far less
//! ceremony, and exercises the same `register_host_fns` codepath.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nexus_kernel::{Capability, CapabilitySet};
use nexus_plugins::__testing::{register_host_fns, HOST_CAPABILITY_DENIED};
use nexus_plugins::PluginData;
use tracing_subscriber::layer::SubscriberExt;
use wasmtime::{Engine, Instance, Linker, Module, Store};

const PROBE_WAT: &str =
    include_str!("fixtures/denial_probe.wat");

const TEST_PLUGIN_ID: &str = "com.nexus.test.denial-probe";

/// Build an instantiated probe sandbox. `caps` controls the granted
/// capabilities; `forge_root` is whatever the FS gate should treat as
/// the root (for the positive test we point at a tempdir we own).
fn build_probe(caps: CapabilitySet, forge_root: PathBuf) -> (Store<PluginData>, Instance) {
    let wasm_bytes = wat::parse_str(PROBE_WAT).expect("parse denial_probe.wat");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("compile probe module");

    let mut data = PluginData {
        plugin_id: TEST_PLUGIN_ID.to_string(),
        forge_root,
        ..Default::default()
    };
    data.capabilities = caps;

    let mut store = Store::new(&engine, data);
    let mut linker: Linker<PluginData> = Linker::new(&engine);
    register_host_fns(&mut linker).expect("register host fns");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate probe");
    (store, instance)
}

fn call_probe(store: &mut Store<PluginData>, instance: &Instance) -> i32 {
    let probe = instance
        .get_typed_func::<(), i32>(&mut *store, "probe")
        .expect("probe export");
    probe.call(&mut *store, ()).expect("probe call ok")
}

// ─── Tracing-event capture (duplicated from kernel `audit::test_support`) ──

/// `audit::test_support::with_captured_events` lives behind `pub(crate)`
/// and is therefore unreachable from an integration test in another
/// crate. Re-implement the same `tracing_subscriber::Layer` here so the
/// test can assert on emitted audit events.
fn with_captured_events<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
    struct CaptureLayer {
        events: Arc<Mutex<Vec<String>>>,
    }

    struct StringVisitor(String);

    impl tracing::field::Visit for StringVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={:?} ", field.name(), value);
        }

        fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut visitor = StringVisitor(String::new());
            event.record(&mut visitor);
            self.events.lock().unwrap().push(visitor.0);
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let layer = CaptureLayer {
        events: Arc::clone(&events),
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    let result = tracing::subscriber::with_default(subscriber, f);
    let captured = events.lock().unwrap().clone();
    (result, captured)
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[test]
fn read_file_denied_without_fs_read_capability() {
    // Empty CapabilitySet → host::read_file must short-circuit at the
    // capability check before touching the filesystem. Forge root is
    // irrelevant because the gate fires before path resolution.
    let (result, events) = with_captured_events(|| {
        let (mut store, instance) = build_probe(CapabilitySet::empty(), PathBuf::new());
        call_probe(&mut store, &instance)
    });

    assert_eq!(
        result, HOST_CAPABILITY_DENIED,
        "host::read_file must return HOST_CAPABILITY_DENIED when fs.read is absent"
    );

    // Audit event must include `capability denied`, the plugin id, and
    // the offending capability name.
    let denial = events
        .iter()
        .find(|line| line.contains("capability denied"))
        .unwrap_or_else(|| panic!("no `capability denied` audit event in {events:#?}"));
    assert!(
        denial.contains("audit=true"),
        "denial event missing audit field: {denial}"
    );
    assert!(
        denial.contains(&format!("plugin_id={TEST_PLUGIN_ID}")),
        "denial event missing plugin_id: {denial}"
    );
    assert!(
        denial.contains("capability=fs.read"),
        "denial event missing capability=fs.read: {denial}"
    );
}

#[test]
fn read_file_succeeds_with_fs_read_capability() {
    // Without this counterpart a regression that always-denies would
    // still pass the negative test above. Seed `test.md` inside a
    // tempdir, point the sandbox at it, and assert the gate lets the
    // call through and returns the correct byte count.
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("test.md"), b"hello").expect("write fixture file");

    // Resolve to canonical form because host::read_file canonicalizes
    // before checking forge-root containment.
    let forge_root = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    let mut caps = CapabilitySet::empty();
    caps.insert(Capability::FsRead);

    let (mut store, instance) = build_probe(caps, forge_root);
    let result = call_probe(&mut store, &instance);

    assert_eq!(
        result, 5,
        "expected 5 bytes (\"hello\") read; got result code {result}"
    );
}
