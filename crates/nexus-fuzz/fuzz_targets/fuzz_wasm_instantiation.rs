// BL-103 — cargo-fuzz / libFuzzer shim for wasmtime instantiation.
//
// This shim is **not** mirrored in the stable `tests/smoke.rs`
// runner because random-bytes-as-WASM panics wasmtime at depths
// that aren't reachable without a coverage-guided fuzzer. Run with:
//
//   cargo install cargo-fuzz   # one-time
//   cargo +nightly fuzz run fuzz_wasm_instantiation \
//       -p nexus-fuzz \
//       fuzz_targets/fuzz_wasm_instantiation.rs
//
// The shim delegates to a thin wrapper in `nexus_plugins::sandbox`
// that constructs a default `WasmConfig` + `PluginData` and tries
// to instantiate. The contract: the sandbox must surface every
// failure as a `PluginError` rather than panicking the host.

#![no_main]

libfuzzer_sys::fuzz_target!(|_data: &[u8]| {
    // Operator wires up `WasmSandbox::new(_data, &cfg, plugin_data)`
    // here. Left as an explicit todo so this file does not silently
    // ship a sandbox-fuzz harness without the wiring it needs to be
    // useful.
    //
    // See crates/nexus-plugins/src/sandbox.rs for the constructor
    // signature; the operator running cargo-fuzz is expected to
    // populate `cfg` + `plugin_data` per their threat-model
    // assumptions (e.g., default fuel limits, no granted caps).
});
