// BL-103 — cargo-fuzz / libFuzzer shim. See sibling
// `fuzz_path_validator.rs` for the operator workflow.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    nexus_fuzz::fuzz_manifest_parse(data);
});
