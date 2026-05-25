# nexus-fuzz

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no · As of: 2026-05-25

## Overview

`nexus-fuzz` is the workspace's **security fuzz-target crate (BL-103)**. It does not implement any product feature; it exists to harden the kernel's *untrusted-input surfaces* — the boundaries where attacker-controlled bytes (a path from a community plugin, an event `type_id`, a capability string, a plugin manifest) cross into trusted Rust code. Its `Cargo.toml` carries `publish = false`, so it never ships to crates.io; it is a workspace member purely so `cargo test --workspace` and CI exercise it.

Each fuzz target is written as a **pure `&[u8] → ()` contract**: bytes in, no panic out, plus a target-specific *invariant assertion* (e.g. a validated path must stay inside the forge root). This shape lets two consumers drive the *same* function. The first is a **stable-Rust smoke runner** (`tests/smoke.rs`) that calls each target thousands of times against a deterministic seeded RNG plus hand-crafted attack-pattern corpora under `corpus/<target>/`; it runs on every `cargo test` (sub-second, "fast-fuzz gate"). The second is **`cargo-fuzz` / libFuzzer**: thin `#![no_main]` shims under `fuzz_targets/` wire each function into coverage-guided fuzzing for operators on a nightly toolchain. The shims are deliberately *not* part of the Cargo package (not built by `cargo build` / `cargo test`); they are only invoked by cargo-fuzz's separate harness.

The chosen split — stable smoke runner for CI, nightly cargo-fuzz for deep campaigns — is a pragmatic answer to "fuzzing needs nightly + libFuzzer." The cheap path-finds-obvious-panics gate lives in the standard test matrix, while expensive coverage-guided work stays operator-side. One target, `fuzz_wasm_instantiation`, ships *only* as a cargo-fuzz shim (and even that shim is left as an explicit TODO stub): random bytes-as-WASM reach panics inside wasmtime at depths a uniform-random loop can't find within a stable-test budget, so calling it from the smoke runner would burn CI time without finding anything.

## Position in the dependency graph

- **Direct nexus-* dependencies:** `nexus-kernel`, `nexus-plugin-api`, `nexus-plugins`, `nexus-types`. These are exactly the crates that own the four fuzzed surfaces. Note this is a *consumer* crate pointing into kernel/leaf crates; it never inverts the microkernel layering because nothing depends back on it.
- **Notable external dependencies (+ why):**
  - `rand` *(dev-dependency)* — `StdRng` seeded from a fixed constant, for deterministic random input generation in the smoke runner.
  - `tempfile` *(dev-dependency)* — creates a throwaway forge-root directory so `ForgePathValidator::new` (which requires an existing root) can be constructed during the path-validator smoke test and shim.
  - libFuzzer integration (`libfuzzer_sys`) is referenced *only inside the `fuzz_targets/*.rs` shims*, which cargo-fuzz builds with its own dependency set; it is **not** declared in `Cargo.toml` and is not a dependency of the package proper.
- **Crates that depend on this one:** none. `nexus-fuzz` is a leaf consumer; no other crate references it (it is only listed as a workspace member in the root `Cargo.toml`).

## Public API surface

`src/lib.rs` is the entire public surface — four free functions, one per shipped target. There are no harness types, traits, or structs; each function constructs whatever state it needs internally.

- `pub fn fuzz_path_validator(forge_root: &Path, data: &[u8])` — builds a `ForgePathValidator` rooted at `forge_root`, interprets `data` (lossily as UTF-8) as a path, and drives both `validate` and `validate_for_write`. On any `Ok`, asserts the returned canonical path `starts_with` the canonical forge root.
- `pub fn fuzz_event_type_id(plugin_id: &str, type_id: &str)` — calls `nexus_kernel::type_id_in_namespace(type_id, plugin_id)`; when it returns `true`, asserts the contract holds (exact match or dotted-suffix extension).
- `pub fn fuzz_capability_set(data: &[u8])` — drives `Capability::from_str` and asserts an `as_str` → `from_str` round-trip on any parsed capability, then round-trips every entry of `Capability::ALL`.
- `pub fn fuzz_manifest_parse(data: &[u8])` — drives `nexus_plugins::parse_manifest(s, "fuzz.toml")`; the only contract is "no panic" (every failure must surface as a `PluginError`).

The crate sets `#![warn(clippy::pedantic)]` with `missing_errors_doc` and `module_name_repetitions` allowed.

### Fuzz target shims (`fuzz_targets/`, cargo-fuzz only — not compiled by the package)

- `fuzz_path_validator.rs` — creates a `tempfile::tempdir()` per invocation, delegates to `fuzz_path_validator`. Carries the fullest operator-workflow comment.
- `fuzz_capability_set.rs` — one-line delegation to `fuzz_capability_set(data)`.
- `fuzz_manifest_parse.rs` — one-line delegation to `fuzz_manifest_parse(data)`.
- `fuzz_event_type_id.rs` — splits `data` at the first `0x00` byte (libFuzzer mutates byte-position separators well) into `(plugin_id, type_id)` before delegating. (Note: the smoke runner instead uses a *tab* separator for its on-disk corpus — see Tests.)
- `fuzz_wasm_instantiation.rs` — `#![no_main]` shim targeting `WasmSandbox::new`, but the body is an intentional empty stub with a TODO directing the operator to wire `WasmSandbox::new(_data, &cfg, plugin_data)` per their threat model. It is **not** mirrored in the smoke runner and is not listed in `lib.rs`'s "targets shipped" table.

## IPC handlers

None. `nexus-fuzz` registers no `CorePlugin` and exposes no `ipc_call` handlers — it is a test/harness library, not a service plugin.

## Capabilities

None declared or requested. The crate exercises the *capability parser* (`Capability::from_str`) as fuzz input but neither declares nor checks capabilities itself.

## Settings / Config

No `.forge/` config file and no `Config` struct. The only tunables are **hardcoded constants in `tests/smoke.rs`**:

| Constant | Value | Meaning |
|----------|-------|---------|
| `PARSER_ITERATIONS` | `10_000` | random iterations per parser target (event_type_id, capability_set, manifest_parse) |
| `PATH_ITERATIONS` | `1_000` | random iterations for the path validator (lower — it does disk I/O via temp dirs) |
| `SEED` | `0xB1_03_F0_22_5E_ED_BEEF` | fixed `StdRng` seed so any failure reproduces with a plain `cargo test -p nexus-fuzz`; flip it to reseed |

Random-input size caps are also hardcoded per target: 4 KiB for paths (length-extension edge cases), 128 B / 64 B for event `type_id` / `plugin_id`, 64 B for capability strings, and up to 2 KiB biased to printable ASCII (bytes `32..=126`) for manifests to raise the TOML-shaped hit rate.

## Events

None. The crate publishes and subscribes to no events; it tests the kernel's `type_id_in_namespace` namespace-membership helper as a pure function, not the live event bus.

## Internals & notable implementation details

**Per-target input space and invariant:**

- **`fuzz_path_validator`** — Input: arbitrary bytes as a path (UTF-8-lossy). Invariant under attack: a successful `validate`/`validate_for_write` must never return a path outside the canonical forge root (path-traversal / confinement escape). It runs *both* validators because `validate_for_write` takes a different code path (TOCTOU-safe canonicalize of the deepest existing ancestor) than `validate`. Failure surfaces as a panicking `assert!` with the escaping path and the root.
- **`fuzz_event_type_id`** — Input: a `(plugin_id, type_id)` string pair. Invariant: `type_id_in_namespace` may return `true` *only* when `type_id == plugin_id` (exact) or `type_id == "<plugin_id>.<suffix>"` (dotted extension). The target re-derives that predicate independently and asserts agreement, specifically to catch **substring-spoof** false positives like `com.foobar.event` claiming to live in `com.foo`'s namespace.
- **`fuzz_capability_set`** — Input: arbitrary bytes as a capability name. Invariants: (1) the parser never panics; (2) any parsed `Capability` round-trips `as_str` → `from_str` to the same variant. It additionally walks all of `Capability::ALL` every call so the bidirectional name table itself is continuously verified (constant but cheap work; an `ALL` entry that fails to re-parse panics immediately).
- **`fuzz_manifest_parse`** — Input: arbitrary bytes as TOML. Invariant: termination without panic — every malformed manifest must come back as a `PluginError`, never an unwind. No structural assertion beyond no-panic.

**Smoke-runner loop** (`tests/smoke.rs`): each `*_smoke` test (1) replays every file under `corpus/<target>/` through the target, then (2) runs the fixed-iteration random loop with a freshly seeded `StdRng`. `corpus_inputs` reads the corpus dir at runtime via `CARGO_MANIFEST_DIR`/`corpus/<target>` and silently returns empty if the dir is missing. A separate `corpus_directories_exist` test is a forward guard asserting all four corpus dirs are present, so crash reproducers always have a home per the BL-103 DoD.

**Corpus tree** — checked-in seed inputs, one file per attack pattern:
- `corpus/path_validator/` — `01_traversal_dotdot` (`../etc/passwd`), `02_absolute_unix`, `03_mid_path_dotdot`, `04_null_byte`, `05_windows_absolute`, `06_long_path`.
- `corpus/event_type_id/` — `01_exact_match`, `02_dotted_suffix`, `03_substring_spoof`, `04_trailing_dot`, `05_empty_plugin_id`, `06_empty_type_id`. Each file is `plugin_id<TAB>type_id`; the smoke runner splits on the **first tab** and skips tab-less files.
- `corpus/capability_set/` — `empty`, `fs_read` (`fs.read`), `fs_write`, `ipc_call`, `kv_read`, `net_http`, `process_spawn`, `unknown_extension`, `uppercase` (`FS.READ`, exercising case-sensitivity).
- `corpus/manifest_parse/` — `01_minimal_core`, `02_with_signature`, `03_empty`, `04_garbage` (`this is not toml = = =`), `05_missing_required_fields`.

**Crash protocol (BL-103 DoD).** Any crash reproducer becomes a P1 bug, lands as a checked-in `#[cfg(test)]` unit test in the *owning* crate (`nexus-types`, `nexus-kernel`, `nexus-plugin-api`, or `nexus-plugins`), and the bug is fixed *before* a minimised reproducer is added under `corpus/<target>/`. cargo-fuzz crash artifacts in `fuzz/artifacts/` are not committed directly.

**Verified upstream signatures (as of 2026-05-25):**
- `nexus_types::ForgePathValidator::new(&Path)`, `.forge_root()`, `.validate(&Path)`, `.validate_for_write(&Path)` — `crates/nexus-types/src/path_validator.rs`.
- `nexus_kernel::type_id_in_namespace(type_id: &str, plugin_id: &str) -> bool` — `crates/nexus-kernel/src/event_bus.rs:37`.
- `nexus_plugin_api::Capability::{from_str, as_str, ALL}` — `crates/nexus-plugin-api/src/capability.rs` (`ALL` currently has 33 entries).
- `nexus_plugins::parse_manifest(&str, &str) -> Result<PluginManifest, PluginError>` — `crates/nexus-plugins/src/manifest.rs:1256`.

## Tests

`tests/smoke.rs` is the entire stable test surface, run by `cargo test -p nexus-fuzz` (and `cargo test --workspace` / CI):

- `fuzz_path_validator_smoke` — corpus replay + 1,000 random iterations against a `tempfile` forge root.
- `fuzz_event_type_id_smoke` — corpus replay (tab-split) + 10,000 random `(plugin_id, type_id)` pairs.
- `fuzz_capability_set_smoke` — corpus replay + 10,000 random capability strings.
- `fuzz_manifest_parse_smoke` — corpus replay + 10,000 ASCII-biased random TOML blobs (≤2 KiB).
- `corpus_directories_exist` — structural guard that the four corpus dirs are present.

Coverage-guided fuzzing is operator-side and **not** part of CI: `cargo install cargo-fuzz` then e.g. `cargo +nightly fuzz run fuzz_path_validator -p nexus-fuzz -- -max_total_time=60` (the 60-second smoke-fuzz gate the BL-103 DoD describes). New coverage-expanding inputs are written back to `corpus/<target>/` and committed; crash artifacts under `fuzz/artifacts/` are not.
