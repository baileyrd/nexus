//! C81: end-to-end proof that `host::http_request` enforces both of its
//! gates through the real wasmtime host-fn dispatch — `Capability::NetHttp`
//! first, then the injected `NetworkPolicy` (closed by default).
//!
//! Mirrors `wasm_capability_denial.rs`'s harness (lower-level wasmtime API,
//! not `WasmSandbox::dispatch`, for the same reason: a single-function
//! probe returning an i32 doesn't need the `nexus_alloc`/`nexus_dispatch`
//! ABI ceremony).

use std::path::PathBuf;

use nexus_kernel::{Capability, CapabilitySet};
use nexus_plugins::__testing::{register_host_fns, HOST_CAPABILITY_DENIED, HOST_ERROR};
use nexus_plugins::{NetworkPolicy, PluginData};
use wasmtime::{Engine, Instance, Linker, Module, Store};

const PROBE_WAT: &str = include_str!("fixtures/http_request_probe.wat");

const TEST_PLUGIN_ID: &str = "com.nexus.test.http-request-probe";

fn build_probe(caps: CapabilitySet, network_policy: NetworkPolicy) -> (Store<PluginData>, Instance) {
    let wasm_bytes = wat::parse_str(PROBE_WAT).expect("parse http_request_probe.wat");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("compile probe module");

    let data = PluginData {
        plugin_id: TEST_PLUGIN_ID.to_string(),
        forge_root: PathBuf::new(),
        capabilities: caps,
        network_policy,
        ..Default::default()
    };

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

#[test]
fn http_request_denied_without_net_http_capability() {
    // Empty CapabilitySet → host::http_request must short-circuit at the
    // capability check before ever looking at the request JSON or policy.
    let (mut store, instance) = build_probe(CapabilitySet::empty(), NetworkPolicy::default());
    let result = call_probe(&mut store, &instance);
    assert_eq!(
        result, HOST_CAPABILITY_DENIED,
        "host::http_request must return HOST_CAPABILITY_DENIED when net.http is absent"
    );
}

#[test]
fn http_request_refused_when_policy_is_closed() {
    // NetHttp granted, but the injected NetworkPolicy is still the default
    // (closed) — the policy gate must refuse before any network I/O.
    let mut caps = CapabilitySet::empty();
    caps.insert(Capability::NetHttp);
    let (mut store, instance) = build_probe(caps, NetworkPolicy::default());
    let result = call_probe(&mut store, &instance);
    assert_eq!(
        result, HOST_ERROR,
        "host::http_request must refuse when NetworkPolicy.enabled is false"
    );
}

#[test]
fn http_request_refused_when_host_not_allowlisted() {
    // NetHttp granted, policy enabled, but api.example.com (the probe's
    // fixed URL) is not on the allowlist.
    let mut caps = CapabilitySet::empty();
    caps.insert(Capability::NetHttp);
    let policy = NetworkPolicy {
        enabled: true,
        allowed_hosts: vec!["other.example.com".to_string()],
        ..Default::default()
    };
    let (mut store, instance) = build_probe(caps, policy);
    let result = call_probe(&mut store, &instance);
    assert_eq!(
        result, HOST_ERROR,
        "host::http_request must refuse a host off the allowlist"
    );
}
