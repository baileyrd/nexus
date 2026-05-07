//! BL-103 — stable-Rust smoke runner over the fuzz targets.
//!
//! Each test runs its target a fixed number of iterations against
//! deterministic random inputs (seeded RNG) and the hand-crafted
//! corpus seeds under `corpus/<target>/`. Iteration counts are kept
//! low (10k for parsers, 1k for the path validator which does
//! disk I/O) so the run-time stays under one second per test on a
//! laptop — this is the "fast-fuzz gate" the BL-103 DoD describes,
//! catching obvious panics on every CI run.
//!
//! Real coverage-guided fuzzing requires nightly + libFuzzer; the
//! `fuzz_targets/*.rs` shims demonstrate the pattern but are not
//! built by `cargo test`. Operators run them via
//! `cargo +nightly fuzz run <target>` after `cargo install cargo-fuzz`.

use std::fs;
use std::path::Path;

use rand::{Rng, RngCore, SeedableRng};
use rand::rngs::StdRng;

use nexus_fuzz::{
    fuzz_capability_set, fuzz_event_type_id, fuzz_manifest_parse, fuzz_path_validator,
};

const PARSER_ITERATIONS: usize = 10_000;
const PATH_ITERATIONS: usize = 1_000;
const SEED: u64 = 0xB1_03_F0_22_5E_ED_BEEF;

fn seed_rng() -> StdRng {
    // Fixed seed so a reproducer of any failure is one `cargo test
    // -p nexus-fuzz` away.
    StdRng::seed_from_u64(SEED)
}

fn random_bytes(rng: &mut StdRng, max_len: usize) -> Vec<u8> {
    let len = rng.random_range(0..=max_len);
    let mut buf = vec![0u8; len];
    rng.fill_bytes(&mut buf);
    buf
}

fn corpus_dir(target: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("corpus");
    p.push(target);
    p
}

fn corpus_inputs(target: &str) -> Vec<Vec<u8>> {
    let dir = corpus_dir(target);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path();
            if p.is_file() {
                fs::read(&p).ok()
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn fuzz_path_validator_smoke() {
    let forge = tempfile::tempdir().expect("forge");
    let mut rng = seed_rng();

    // Corpus seeds — known traversal-attack patterns.
    for input in corpus_inputs("path_validator") {
        fuzz_path_validator(forge.path(), &input);
    }

    // Random bytes — paths can be longer than typical so allow up
    // to 4 KiB to exercise length-extension edge cases.
    for _ in 0..PATH_ITERATIONS {
        let bytes = random_bytes(&mut rng, 4096);
        fuzz_path_validator(forge.path(), &bytes);
    }
}

#[test]
fn fuzz_event_type_id_smoke() {
    let mut rng = seed_rng();

    for input in corpus_inputs("event_type_id") {
        // Each corpus file is `plugin_id\ttype_id` — split on the
        // first tab. Files without a tab are skipped (the cargo-fuzz
        // shim picks its own splitting strategy).
        let s = String::from_utf8_lossy(&input);
        if let Some((pid, tid)) = s.split_once('\t') {
            fuzz_event_type_id(pid, tid);
        }
    }

    for _ in 0..PARSER_ITERATIONS {
        let pid = String::from_utf8_lossy(&random_bytes(&mut rng, 64)).into_owned();
        let tid = String::from_utf8_lossy(&random_bytes(&mut rng, 128)).into_owned();
        fuzz_event_type_id(&pid, &tid);
    }
}

#[test]
fn fuzz_capability_set_smoke() {
    let mut rng = seed_rng();

    for input in corpus_inputs("capability_set") {
        fuzz_capability_set(&input);
    }

    for _ in 0..PARSER_ITERATIONS {
        let bytes = random_bytes(&mut rng, 64);
        fuzz_capability_set(&bytes);
    }
}

#[test]
fn fuzz_manifest_parse_smoke() {
    let mut rng = seed_rng();

    for input in corpus_inputs("manifest_parse") {
        fuzz_manifest_parse(&input);
    }

    // Manifests are larger than other inputs; allow up to 2 KiB
    // and bias toward ASCII for higher TOML-shaped hit rate.
    for _ in 0..PARSER_ITERATIONS {
        let len = rng.random_range(0..=2048);
        let mut buf = vec![0u8; len];
        for byte in &mut buf {
            *byte = rng.random_range(32u8..=126); // printable ASCII bias
        }
        fuzz_manifest_parse(&buf);
    }
}

#[test]
fn corpus_directories_exist() {
    // Forward guard — make sure the corpus tree stays structured so
    // crash reproducers go to the right place per the BL-103 DoD.
    for target in [
        "path_validator",
        "event_type_id",
        "capability_set",
        "manifest_parse",
    ] {
        let dir = corpus_dir(target);
        assert!(
            dir.is_dir(),
            "BL-103 corpus dir missing: {} — recreate with \
             `mkdir -p {}`",
            dir.display(),
            dir.display(),
        );
    }
}

// Suppress the unused-helper warning when this file is built without
// the corpus path being walked (defensive: `corpus_inputs` reads at
// runtime, not at compile time).
#[allow(dead_code)]
fn _ensure_path_used(_p: &Path) {}
