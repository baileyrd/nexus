# nexus-fuzz — security fuzz targets (BL-103)

Five fuzz targets covering the kernel's untrusted-input surfaces:

| Target                      | Surface                                                        |
|-----------------------------|----------------------------------------------------------------|
| `fuzz_path_validator`       | `nexus_types::ForgePathValidator::validate` / `_for_write`     |
| `fuzz_event_type_id`        | `nexus_kernel::type_id_in_namespace`                           |
| `fuzz_capability_set`       | `nexus_plugin_api::Capability::from_str` round-trip            |
| `fuzz_manifest_parse`       | `nexus_plugins::parse_manifest`                                |
| `fuzz_wasm_instantiation`   | `nexus_plugins::sandbox::WasmSandbox::new` (cargo-fuzz only)   |

## Two ways to drive them

### 1. Stable-Rust smoke runner — runs on every `cargo test`

`tests/smoke.rs` calls each target a few thousand times with
deterministic random inputs (fixed-seed `StdRng`) plus the
hand-crafted attack-pattern files under `corpus/<target>/`. Catches
obvious panics and contract violations on every CI run; ~0.3 s total
runtime so the cost is bounded.

```bash
cargo test -p nexus-fuzz
```

The seed is `SEED` in `tests/smoke.rs` — flip it to reseed the run.
Any failure is reproducible by re-running the test (the seed is
constant); a corresponding minimal corpus file lives at
`corpus/<target>/<n>_<short_label>` once the underlying bug is fixed.

### 2. Coverage-guided fuzzing — operator-side, requires nightly

```bash
rustup install nightly
cargo install cargo-fuzz

# Each target is a thin shim under fuzz_targets/. Run one:
cargo +nightly fuzz run fuzz_path_validator -p nexus-fuzz

# 60-second smoke-fuzz gate (matches the BL-103 DoD):
cargo +nightly fuzz run fuzz_path_validator -p nexus-fuzz -- -max_total_time=60
```

The first run seeds itself from `corpus/<target>/`. New inputs that
expand coverage are written back to that directory; commit them.
Reproducer files for crashes land in
`fuzz/artifacts/<target>/crash-<hash>` per cargo-fuzz convention —
do **not** commit these directly.

## Crash protocol (BL-103 DoD §"Any crash reproducer")

1. Capture the reproducer from `fuzz/artifacts/<target>/crash-*`.
2. File a P1 bug on the relevant subsystem (`nexus-types`,
   `nexus-plugins`, `nexus-kernel`, `nexus-plugin-api`).
3. Add a deterministic unit test in the relevant crate's normal
   test suite (`crates/<subsystem>/src/**/*.rs` `#[cfg(test)]`
   block) that locks in the fix.
4. Add a *minimised* version of the reproducer to
   `corpus/<target>/` so the smoke runner regression-tests it on
   every `cargo test`.
5. Ship the fix; the reproducer crash file is purged from the
   `fuzz/artifacts/` tree.

## Why the WASM sandbox target only ships as a cargo-fuzz shim

Random byte sequences as `wasm_bytes` reach panics inside wasmtime at
depths that uniform random sampling can't find within a stable-Rust
test budget. The fuzz_targets shim is laid down so an operator with
nightly + cargo-fuzz can drive it; the smoke runner intentionally
does not call it (would burn CI time without finding anything).
