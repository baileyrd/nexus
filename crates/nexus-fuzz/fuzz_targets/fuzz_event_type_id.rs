// BL-103 — cargo-fuzz / libFuzzer shim. See sibling
// `fuzz_path_validator.rs` for the operator workflow.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // Split at the first 0x00 byte to derive (plugin_id, type_id).
    // libFuzzer's mutator works well with byte-position separators.
    let cut = data.iter().position(|&b| b == 0).unwrap_or(data.len() / 2);
    let (a, b) = data.split_at(cut);
    let pid = String::from_utf8_lossy(a);
    let tid = String::from_utf8_lossy(if b.is_empty() { b } else { &b[1..] });
    nexus_fuzz::fuzz_event_type_id(pid.as_ref(), tid.as_ref());
});
