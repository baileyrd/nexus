//! BL-103 — security fuzz targets.
//!
//! Each `fuzz_*` function in this crate is a *pure* contract — `&[u8]`
//! in, no panics out. They are written so two consumers can drive
//! them:
//!
//! 1. **Stable-Rust smoke runner** — `tests/smoke.rs` calls each
//!    function thousands of times with deterministic random inputs +
//!    hand-crafted attack-pattern corpora under `corpus/<target>/`.
//!    Catches regressions on every `cargo test` run; ships as part
//!    of the regular CI matrix.
//! 2. **`cargo-fuzz` / libFuzzer** — operators with a nightly
//!    toolchain wire each function into a thin
//!    `fuzz_targets/<target>.rs` shim and run
//!    `cargo +nightly fuzz run <target>`. The thin shims under
//!    `fuzz_targets/` in this crate's directory show the pattern;
//!    they're not built by `cargo build` because they aren't in the
//!    package, only invoked by `cargo-fuzz`'s separate harness.
//!
//! ## Targets shipped
//!
//! | Target               | Surface                                            |
//! |----------------------|----------------------------------------------------|
//! | `fuzz_path_validator`| `nexus_types::ForgePathValidator::validate`        |
//! | `fuzz_event_type_id` | `nexus_kernel::type_id_in_namespace`               |
//! | `fuzz_capability_set`| `nexus_plugin_api::Capability::from_str`           |
//! | `fuzz_manifest_parse`| `nexus_plugins::parse_manifest`                    |
//!
//! ## Targets *not* shipped
//!
//! - `fuzz_wasm_instantiation` — fuzzing `WasmSandbox::new` requires
//!   real coverage-guided fuzzing (libFuzzer + nightly) because
//!   wasmtime can panic on malformed input at depths that aren't
//!   reachable from a random-bytes loop. The
//!   `fuzz_targets/fuzz_wasm_instantiation.rs` shim is laid down for
//!   the operator who runs cargo-fuzz, and not exposed here.
//!
//! ## Crash protocol
//!
//! Per BL-103 DoD, any crash reproducer becomes a P1 bug, lands as a
//! checked-in unit test in the relevant crate's normal test suite,
//! and the underlying bug ships before the reproducer is added to
//! the corpus tree.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::path::Path;

use nexus_kernel::type_id_in_namespace;
use nexus_plugin_api::Capability;
use nexus_plugins::parse_manifest;
use nexus_types::ForgePathValidator;

/// Drive `ForgePathValidator::validate` with arbitrary bytes
/// interpreted as a path. The validator must terminate without
/// panicking and never return a path outside the configured forge
/// root.
pub fn fuzz_path_validator(forge_root: &Path, data: &[u8]) {
    let validator = match ForgePathValidator::new(forge_root) {
        Ok(v) => v,
        Err(_) => return,
    };
    let canonical_root = validator.forge_root().to_path_buf();

    // Treat the input as a path; tolerate non-UTF-8 by lossy
    // conversion (the validator accepts &Path which on Unix is
    // already &OsStr-equivalent).
    let s = String::from_utf8_lossy(data);
    let path = Path::new(s.as_ref());

    if let Ok(canonical) = validator.validate(path) {
        assert!(
            canonical.starts_with(&canonical_root),
            "BL-103 invariant violated: validate returned {} which is \
             outside forge root {}",
            canonical.display(),
            canonical_root.display()
        );
    }
    // `validate_for_write` exercises a different code path (TOCTOU
    // closure via deepest-existing-ancestor canonicalize). Run it
    // too, with the same guarantee.
    if let Ok(canonical) = validator.validate_for_write(path) {
        assert!(
            canonical.starts_with(&canonical_root),
            "BL-103 invariant violated (write path): {} escapes {}",
            canonical.display(),
            canonical_root.display(),
        );
    }
}

/// Drive `type_id_in_namespace` with arbitrary `(plugin_id,
/// type_id)` byte slices. The contract is that `true` only when
/// `type_id == plugin_id` or `type_id == "<plugin_id>.<suffix>"`
/// — anything else (including substring spoofs like
/// `"foobar.event"` claiming to be in `"foo"`'s namespace) must
/// return `false`. The function must terminate without panic.
pub fn fuzz_event_type_id(plugin_id: &str, type_id: &str) {
    let result = type_id_in_namespace(type_id, plugin_id);
    if result {
        // If we said yes, prove the contract holds in this direction.
        let exact = type_id == plugin_id;
        let suffix = type_id
            .strip_prefix(plugin_id)
            .map(|rest| rest.starts_with('.'))
            .unwrap_or(false);
        assert!(
            exact || suffix,
            "BL-103 invariant violated: type_id_in_namespace returned \
             true for type_id={type_id:?} plugin_id={plugin_id:?} \
             which is neither equal nor a dotted-suffix extension",
        );
    }
}

/// Drive `Capability::from_str` with arbitrary bytes. Two
/// invariants the fuzz target enforces:
///
/// 1. The parser must terminate without panic on any input.
/// 2. A parsed `Capability` must round-trip through `as_str` →
///    `from_str` to the same variant (catching a regression in
///    either direction of the bidirectional table).
pub fn fuzz_capability_set(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let parsed = Capability::from_str(s.as_ref());
    if let Ok(cap) = parsed {
        let back_str = cap.as_str();
        match Capability::from_str(back_str) {
            Ok(reparsed) => assert_eq!(
                cap.as_str(),
                reparsed.as_str(),
                "BL-103 invariant violated: cap round-trip produced \
                 different name {:?} != {:?}",
                cap.as_str(),
                reparsed.as_str(),
            ),
            Err(e) => panic!(
                "BL-103 invariant violated: as_str() of parsed cap {:?} \
                 failed to re-parse: {e}",
                cap.as_str()
            ),
        }
    }
    // Also drive ALL declared variants through the round-trip so
    // the table itself is fuzzed (constant work but cheap).
    for &cap in Capability::ALL {
        let back = Capability::from_str(cap.as_str()).expect("ALL entry must parse");
        assert_eq!(cap.as_str(), back.as_str());
    }
}

/// Drive `parse_manifest` with arbitrary TOML strings. The parser
/// must terminate without panic regardless of input — every failure
/// must surface as a `PluginError`.
pub fn fuzz_manifest_parse(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let _ = parse_manifest(s.as_ref(), "fuzz.toml");
}
