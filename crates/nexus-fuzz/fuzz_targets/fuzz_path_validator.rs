// BL-103 — cargo-fuzz / libFuzzer shim. Built only when an operator
// runs `cargo +nightly fuzz run fuzz_path_validator`. Not part of the
// regular `cargo build`; the stable test suite drives the same target
// function via `tests/smoke.rs`.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let forge = tempfile::tempdir().expect("forge tempdir");
    nexus_fuzz::fuzz_path_validator(forge.path(), data);
});
