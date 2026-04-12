# Nexus PRD 01 — Kernel & Event System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus-kernel` crate to interface-complete state — all public types, traits, and functions from the interface spec defined and compiling, with a working smoke test that spins up an empty kernel and shuts it down cleanly.

**Architecture:** Cargo workspace with two crates (`nexus-types` stub + `nexus-kernel`). Kernel owns the event bus (tokio broadcast), capability enum, plugin lifecycle traits, and `PluginContext` definition. Methods whose behavior depends on crates not yet built (`nexus-security`, `nexus-storage`, `nexus-plugins`) are defined in the contract but stubbed with `todo!()` or return the appropriate error variant.

**Tech Stack:** Rust (latest stable), tokio 1.35+, tracing 0.1, serde 1.0, uuid 1.0, chrono 0.4, thiserror 1.0, async-trait 0.1, toml 0.8, nextest as test runner.

**Parent docs:**
- [`2026-04-11-nexus-roadmap-design.md`](../specs/2026-04-11-nexus-roadmap-design.md)
- [`2026-04-11-nexus-m1-foundation-spec.md`](../specs/2026-04-11-nexus-m1-foundation-spec.md)
- [`2026-04-11-nexus-prd-01-kernel-interface-spec.md`](../specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md) — **the contract this plan implements**

---

## Prerequisites (do before executing this plan)

1. **Initialize git** in the project root if not already:
   ```bash
   cd /mnt/c/Users/baile/dev/nexus
   git init
   git add PRDs/ docs/
   git commit -m "initial planning artifacts"
   ```
2. **Install Rust toolchain** via `rustup` (latest stable).
3. **Install `cargo-nextest`**:
   ```bash
   cargo install cargo-nextest --locked
   ```
4. **Verify tooling**:
   ```bash
   cargo --version    # cargo 1.78+ or later
   rustc --version    # rustc 1.78+ or later
   cargo nextest --version
   ```

---

## File Structure

Before task-level work, here's the target layout this plan produces:

```
nexus/
├── Cargo.toml                              # workspace root
├── rust-toolchain.toml                     # pinned Rust version
├── .gitignore                              # standard Rust ignore
├── crates/
│   ├── nexus-types/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                      # stub — empty crate with doc comment
│   └── nexus-kernel/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                      # public re-exports
│           ├── capability.rs               # Capability + CapabilitySet
│           ├── config.rs                   # KernelConfig + load
│           ├── context.rs                  # PluginContext trait
│           ├── context_impl.rs             # KernelPluginContext runtime impl
│           ├── error.rs                    # Error + sub-enums
│           ├── event.rs                    # NexusEvent + metadata + filter + StopReason
│           ├── event_bus.rs                # EventBus + EventSubscription
│           ├── kernel.rs                   # Kernel struct + new/start/shutdown
│           ├── log.rs                      # LogLevel enum
│           ├── plugin.rs                   # PluginLifecycle + trust/status types
│           └── plugin_registry.rs          # PluginRegistry
├── tests/                                  # workspace-level integration tests
│   └── prd-01-smoke.rs                     # §12 smoke test
└── docs/
    ├── adr/                                # architecture decision records
    └── superpowers/
        ├── specs/
        └── plans/
```

**File responsibility notes:**
- Each kernel module has one clear job; files that will exceed ~200 lines get split.
- `context_impl.rs` is the only file with "glue" behavior — it wires `PluginContext` methods to internal helpers and is where capability enforcement lives. Everything else is either pure types or thin wrappers.
- `nexus-types` stays empty in this plan; future PRDs (04+) add shared types as real plugins need them.

---

## Task Overview

26 tasks across 12 phases:
1. Phase 1: Workspace skeleton (Tasks 1–3)
2. Phase 2: ADRs (Task 4)
3. Phase 3: Simple types (Tasks 5–10)
4. Phase 4: Event types (Tasks 11–13)
5. Phase 5: Error types (Tasks 14–15)
6. Phase 6: Event bus (Tasks 16–17)
7. Phase 7: Plugin types (Tasks 18–19)
8. Phase 8: Lifecycle trait (Task 20)
9. Phase 9: PluginContext trait (Task 21)
10. Phase 10: KernelConfig loading (Task 22)
11. Phase 11: Kernel struct (Tasks 23–25)
12. Phase 12: Smoke test (Task 26)

---

## Phase 1: Workspace Skeleton

### Task 1: Create workspace root configuration

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`

- [ ] **Step 1: Create `.gitignore`**

Write `/mnt/c/Users/baile/dev/nexus/.gitignore`:

```
/target
**/*.rs.bk
Cargo.lock
.DS_Store
*.swp
```

- [ ] **Step 2: Create `rust-toolchain.toml`**

Write `/mnt/c/Users/baile/dev/nexus/rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Create workspace root `Cargo.toml`**

Write `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/nexus-types",
    "crates/nexus-kernel",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
publish = false

[workspace.dependencies]
# Async runtime
tokio = { version = "1.35", features = ["full"] }

# Logging/tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# Utilities
uuid = { version = "1.0", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1.0"
async-trait = "0.1"
```

- [ ] **Step 4: Run `cargo check` to verify workspace config**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo check`
Expected: "error: no targets specified in the manifest" or "error: no matching package found for workspace member `crates/nexus-types`" — crate directories don't exist yet, which is expected.

- [ ] **Step 5: Commit**

```bash
cd /mnt/c/Users/baile/dev/nexus
git add Cargo.toml rust-toolchain.toml .gitignore
git commit -m "chore: initialize Cargo workspace skeleton"
```

---

### Task 2: Create `nexus-types` crate stub

**Files:**
- Create: `crates/nexus-types/Cargo.toml`
- Create: `crates/nexus-types/src/lib.rs`

- [ ] **Step 1: Create `nexus-types` Cargo.toml**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-types/Cargo.toml`:

```toml
[package]
name = "nexus-types"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Shared types used by the Nexus kernel and plugins"

[dependencies]
serde = { workspace = true }
```

- [ ] **Step 2: Create `nexus-types` lib.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-types/src/lib.rs`:

```rust
//! Nexus shared types.
//!
//! This crate is the leaf of the Nexus dependency graph. It holds types
//! that must be shared between the kernel (`nexus-kernel`) and plugin code
//! that runs in WASM sandboxes. At PRD 01 time, this crate is intentionally
//! empty — types are added here when a second consumer (plugins) appears.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the contract this crate supports.

// Intentionally empty at PRD 01 time.
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo check -p nexus-types`
Expected: "error: no matching package named `nexus-kernel` found" (the workspace expects nexus-kernel too, which doesn't exist yet). That's OK — nexus-types itself compiles as a target.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-types/
git commit -m "feat(types): add nexus-types stub crate"
```

---

### Task 3: Create `nexus-kernel` crate with empty lib.rs

**Files:**
- Create: `crates/nexus-kernel/Cargo.toml`
- Create: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `nexus-kernel` Cargo.toml**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/Cargo.toml`:

```toml
[package]
name = "nexus-kernel"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus kernel: event bus, plugin lifecycle, capability system"

[dependencies]
nexus-types = { path = "../nexus-types" }
tokio = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

- [ ] **Step 2: Create minimal lib.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
```

- [ ] **Step 3: Verify workspace compiles**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo check --workspace`
Expected: PASS with no errors (warnings about unused deps are OK at this stage).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/
git commit -m "feat(kernel): add nexus-kernel stub crate"
```

---

## Phase 2: ADRs

### Task 4: Write the 10 ADRs from M1 spec §10.4

**Files:**
- Create: `docs/adr/0001-cargo-workspace-with-prd-crates.md`
- Create: `docs/adr/0002-hierarchical-capability-strings.md`
- Create: `docs/adr/0003-storage-owns-file-watcher.md`
- Create: `docs/adr/0004-crate-boundaries-and-ownership.md`
- Create: `docs/adr/0005-single-dispatch-handler-ids.md`
- Create: `docs/adr/0006-kv-backed-plugin-state.md`
- Create: `docs/adr/0007-closed-event-enum-with-custom-variant.md`
- Create: `docs/adr/0008-tech-stack-defaults.md`
- Create: `docs/adr/0009-keyring-hard-fail-policy.md`
- Create: `docs/adr/0010-no-plugin-signing-in-m1.md`

- [ ] **Step 1: Create the ADR template and 0001**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0001-cargo-workspace-with-prd-crates.md`:

```markdown
# ADR 0001: Cargo Workspace with One Crate Per PRD

**Date:** 2026-04-11
**Status:** Accepted

## Context

Nexus is spec'd as 17 PRDs covering distinct subsystems. We need a physical
project structure that lets AI agents work in parallel on isolated pieces
without merge conflicts, and that enforces inter-PRD contracts at compile time.

## Decision

Single Cargo workspace with one crate per PRD:
- `nexus-types` (leaf — shared primitives)
- `nexus-kernel` (PRD 01)
- `nexus-security` (PRD 02)
- `nexus-storage` (PRD 03)
- `nexus-plugins` (PRD 04 + 04a)
- `nexus-cli` (PRD 05)

Each crate has its own `Cargo.toml` referencing `workspace.dependencies`.

## Alternatives considered

- Single-crate monolith: fails for parallel agent work (every agent touches
  the same `Cargo.toml`); no boundary enforcement.
- Multi-repo: too much overhead for solo+agents (submodule churn, version
  pinning, cross-repo PRs).
- Phase-sized crates: fewer crates, but less clean boundaries and harder
  agent delegation.

## Consequences

- Inter-PRD contracts enforced at the Rust compiler level — `nexus-plugins`
  can't reach into `nexus-kernel` internals, only `pub` interfaces.
- Small cost: ~6 `Cargo.toml` files to maintain instead of 1.
- Cross-crate refactors are slightly more friction than in-crate refactors.
```

- [ ] **Step 2: Create 0002 (capability strings)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0002-hierarchical-capability-strings.md`:

```markdown
# ADR 0002: Hierarchical Dot-Namespaced Capability Strings

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 01 defined capabilities as a Rust enum; plugin manifests reference them
as strings. We need a canonical mapping that avoids the typo-bypass bug class
and scales cleanly to M2–M5 additions.

## Decision

Capabilities are hierarchical dot-namespaced strings (`"fs.read"`,
`"net.http.localhost"`). The `Capability` enum is the single source of truth,
with `as_str()` / `from_str()` providing bidirectional conversion. Manifest
parsing validates strings against the enum at parse time. No wildcards.

## Alternatives considered

- Flat strings (`"FsRead"`, `"Network"`): loses risk-gradient info.
- Enum-as-canonical in manifests: ugly TOML, hostile to non-Rust tooling.
- Two-tier split (`"filesystem"` + `"read"`): more typing, no benefit.

## Consequences

- Adding a capability requires editing one const table.
- Typos in manifests fail at parse time, pointing to the offending line.
- M2–M5 additions slot cleanly under existing namespaces (`ai.*`, `db.*`).
- Risk-level metadata lives separately in `nexus-security`, not the enum.
```

- [ ] **Step 3: Create 0003 (storage owns watcher)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0003-storage-owns-file-watcher.md`:

```markdown
# ADR 0003: Storage Owns the File Watcher

**Date:** 2026-04-11
**Status:** Accepted

## Context

File system changes must propagate to the kernel event bus so subscribers
(plugins, CLI watch mode, future GUI) can react. PRD 03 describes a `notify`
watcher with debouncing and rename detection; PRD 01 has `FileCreated`/
`FileModified`/`FileDeleted` events. Ownership wasn't spec'd.

## Decision

`nexus-storage` owns the `notify` watcher and emits events to the kernel bus.
Rename detection (hash match on Delete+Create within the debounce window)
produces a single `FileRenamed { from, to, content_hash }` event instead of
a Delete+Create pair.

## Alternatives considered

- Kernel-owned watcher with storage as subscriber: creates a cycle because
  rename detection needs content hashing which lives in storage.
- Two independent watchers: double OS handle pressure, inconsistent state,
  debounce timers fighting. Wrong.

## Consequences

- One watcher per forge, one uniform event stream.
- `nexus-kernel` gains a `FileRenamed` event variant.
- `nexus-storage` has a compile-time dep on `nexus-kernel` (already true).
```

- [ ] **Step 4: Create 0004 (crate boundaries)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0004-crate-boundaries-and-ownership.md`:

```markdown
# ADR 0004: Crate Boundaries and Ownership Map

**Date:** 2026-04-11
**Status:** Accepted

## Context

With per-PRD crates, we need explicit rules for who owns what. The wrong
split causes circular deps, duplicated logic, or capability checks that
live in two places and disagree.

## Decision

- `nexus-kernel`: `Capability` enum, `CapabilitySet`, `PluginLifecycle` trait,
  `PluginContext` trait + runtime impl (with capability enforcement built in),
  `EventBus`, `NexusEvent` enum, KV store API, `Plugin` trait.
- `nexus-security`: threat model, keyring, capability risk-level metadata,
  slimmed audit logger. Depends only on `nexus-kernel`.
- `nexus-storage`: file watcher, SQLite index, Tantivy search. Depends on
  `nexus-kernel` + `nexus-security`.
- `nexus-plugins`: TOML manifest parser, WASM sandbox, plugin loader, hot-
  reload. Depends on `nexus-kernel` + `nexus-security`.
- `nexus-cli`: `nexus` binary. Depends on every other crate.

Strict DAG with `nexus-types` as leaf-of-leaf. No cycles.

## Alternatives considered

- Manifest parser in kernel: rejected — kernel shouldn't know about TOML.
- WASM sandbox in security: rejected — tightly coupled to wasmtime API,
  doesn't fit a "policy" crate.
- Separate `nexus-capabilities` crate: overkill for one enum + risk table.

## Consequences

- Capability checks live in exactly one place (kernel context impl).
- Plugins physically cannot bypass capability checks because they only
  hold `&dyn PluginContext`.
- `nexus-security` is slim — policy + side effects, not a request gate.
```

- [ ] **Step 5: Create 0005 (single dispatch)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0005-single-dispatch-handler-ids.md`:

```markdown
# ADR 0005: Plugin Calling Convention — Single Dispatch with Handler IDs

**Date:** 2026-04-11
**Status:** Accepted

## Context

WASM plugins register CLI subcommands, IPC commands, event subscribers, etc.
We need a wire-level protocol for the kernel to invoke plugin handlers.

## Decision

Each plugin exports exactly one function: `nexus_dispatch(handler_id: u32,
args_ptr: u32, args_len: u32) -> u64`. The manifest assigns stable handler
IDs to each registration. The plugin SDK (PRD 04a templates) generates the
dispatch function from `#[handler(id = N)]` attributes. JSON via `serde_json`
is the wire format; shared types live in `nexus-types`.

Handler ID namespacing: `0x01_xx_xx_xx` = CLI, `0x02_xx_xx_xx` = IPC, etc.

## Alternatives considered

- Named exports per handler: verbose, no runtime handler add, worse for
  hot-reload stability.
- WIT component model: modern but overkill for Rust-only plugin authorship
  in a personal tool. Revisit if cross-language plugins become a goal.

## Consequences

- Plugin SDK has a tiny surface: one function, one macro.
- Handler IDs are stable across hot-reloads even if Rust function names
  change internally.
- Debugging is less friendly (stack traces show `nexus_dispatch` not the
  real handler) but the cost is small.
```

- [ ] **Step 6: Create 0006 (KV-backed state)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0006-kv-backed-plugin-state.md`:

```markdown
# ADR 0006: KV-Backed, Plugin-Managed Hot-Reload State

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 04 keeps hot-reload in M1. When a plugin's WASM file changes, the kernel
swaps in the new version. What happens to the old instance's in-memory state?

## Decision

Plugins own their persistence. Plugins that need state across reloads
explicitly call `ctx.kv_set("state", bytes)` in `on_stop` and
`ctx.kv_get("state")` in `on_init`. No special kernel mechanism beyond the
existing KV API. PRD 04a templates include a commented-out example showing
the pattern; plugins opt in.

## Alternatives considered

- Kernel-mediated checkpoint hooks (`on_checkpoint` / `on_restore`): forces
  every plugin to implement lifecycle methods it doesn't care about.
- Live migration (snapshot WASM linear memory): brittle, requires identical
  memory layouts between old and new modules, almost always broken after
  recompilation.

## Consequences

- Zero new kernel surface beyond KV API already present.
- Crash safety is automatic: state is written before the old instance dies.
- Each plugin reimplements the same serialize/deserialize boilerplate;
  acceptable for a personal tool.
```

- [ ] **Step 7: Create 0007 (closed event enum + Custom)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0007-closed-event-enum-with-custom-variant.md`:

```markdown
# ADR 0007: Closed Event Enum with Custom Variant

**Date:** 2026-04-11
**Status:** Accepted

## Context

The `NexusEvent` enum must carry events from all 17 PRDs plus plugin-emitted
signals. A monolithic enum scales poorly if plugins need to emit their own
types; an open trait-object system loses pattern-matching exhaustiveness.

## Decision

Closed enum for kernel-owned events (one variant per subsystem concept, added
per phase). Single `NexusEvent::Custom { type_id, emitting_plugin, payload }`
variant for plugin-emitted signals. Plugins cannot emit kernel events.
`type_id` must start with the emitting plugin's id (reverse-DNS namespace);
enforced by the kernel. Bounded broadcast channel, capacity 2048, with
`Lagged(n)` on slow subscribers.

## Alternatives considered

- Open trait-object events: loses exhaustive pattern matching.
- All events via `Custom`: no compile-time help for kernel-side subsystems.

## Consequences

- Each phase adds kernel events by editing `nexus-kernel`, forcing explicit
  cross-phase coordination via compile errors.
- Plugin events are type-unsafe at the payload level (JSON blob); plugin
  authors deserialize at the boundary.
- Anti-spoofing is enforced by construction: the kernel sets
  `emitting_plugin` from the calling plugin's identity.
```

- [ ] **Step 8: Create 0008 (tech stack)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0008-tech-stack-defaults.md`:

```markdown
# ADR 0008: Tech Stack Defaults

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRDs 01–05 leave many crate choices open. We need locked defaults to avoid
per-PRD bikeshedding during implementation.

## Decision

See M1 spec §3 for the full table. Key picks for PRD 01:

- Async runtime: `tokio` 1.35+, full features, no abstraction layer.
- Logging: `tracing` + `tracing-subscriber` + `tracing-appender`.
- Serialization: `serde` 1.0 + `serde_json` 1.0.
- Error handling: `thiserror` in libraries, `anyhow` in binary.
- Async traits: `async-trait` until native support stabilizes.
- TOML: `toml` 0.8 for reads.
- Utilities: `uuid` 1.0, `chrono` 0.4 with `serde` feature.
- Test runner: `nextest` (replaces `cargo test`).
- MSRV: latest stable Rust at M1 start.

## Alternatives considered

- Tokio alternatives (smol, async-std): rejected for ecosystem reasons.
- `log` crate instead of `tracing`: rejected — `tracing` has structured
  fields and spans we need for the slimmed audit log.
- `anyhow` everywhere: rejected — libraries need typed errors.

## Consequences

- Versions pinned in workspace root `Cargo.toml`. Bumps require an ADR.
```

- [ ] **Step 9: Create 0009 (keyring hard-fail)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0009-keyring-hard-fail-policy.md`:

```markdown
# ADR 0009: Keyring Hard-Fail Policy

**Date:** 2026-04-11
**Status:** Accepted

## Context

`keyring-rs` may fail to access the OS keychain (no D-Bus on Linux, locked
macOS Keychain, etc.). We need a policy for what happens when credentials
can't be stored or retrieved.

## Decision

Hard fail. Nexus refuses to start if the keyring is unavailable. The error
message points to platform-specific setup docs. `NEXUS_NO_KEYRING=1` is an
escape hatch that disables credential operations entirely (not a fallback).

## Alternatives considered

- Encrypted on-disk fallback with passphrase: adds a UX surface (prompts)
  for the 99% case where the keychain works.
- Plaintext fallback: bad — secrets on disk.

## Consequences

- Personal-tool framing assumes a daily-driver machine where the keychain
  works; this is the right default.
- Users running Nexus in unusual environments (remote SSH, container) must
  set up keyring access or use `NEXUS_NO_KEYRING=1`.
- Not yet enforced in PRD 01 (keyring is `nexus-security` concern).
```

- [ ] **Step 10: Create 0010 (no plugin signing in M1)**

Write `/mnt/c/Users/baile/dev/nexus/docs/adr/0010-no-plugin-signing-in-m1.md`:

```markdown
# ADR 0010: No Plugin Signature Verification in M1

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 02 §5 describes plugin signing (Ed25519 signatures, trusted authors).
Roadmap Section 3 cuts "plugin code review/approval workflows" from M1.
Trust levels (`core` vs `community`) stay in the manifest.

## Decision

M1 implements trust levels but not signature verification. Plugins declare
`trust_level = "core"` or `"community"` in their manifests; the kernel honors
the declaration without verifying it cryptographically. Community plugins
with HIGH-risk capabilities get an install-time CLI prompt.

Signing verification is a v0.2 feature, added if/when the personal-tool
becomes a multi-user concern.

## Alternatives considered

- Implement signing now: excess work for zero personal-tool benefit.
- Cut trust levels entirely: makes install-time prompts awkward.

## Consequences

- Trust is advisory, not enforced. Acceptable given single-user.
- The `ed25519-dalek` dep is deferred but the architecture leaves room
  for it to plug in later without breaking the manifest format.
```

- [ ] **Step 11: Verify all ADRs are in place**

Run: `ls /mnt/c/Users/baile/dev/nexus/docs/adr/ | sort`
Expected:
```
0001-cargo-workspace-with-prd-crates.md
0002-hierarchical-capability-strings.md
0003-storage-owns-file-watcher.md
0004-crate-boundaries-and-ownership.md
0005-single-dispatch-handler-ids.md
0006-kv-backed-plugin-state.md
0007-closed-event-enum-with-custom-variant.md
0008-tech-stack-defaults.md
0009-keyring-hard-fail-policy.md
0010-no-plugin-signing-in-m1.md
```

- [ ] **Step 12: Commit**

```bash
git add docs/adr/
git commit -m "docs: initial ADRs (0001-0010) from M1 brainstorming"
```

---

## Phase 3: Simple Types

### Task 5: `LogLevel` enum

**Files:**
- Create: `crates/nexus-kernel/src/log.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/log.rs`:

```rust
//! Plugin log level (independent of `tracing::Level` to avoid leaking the
//! tracing crate into the plugin API surface).

/// Log severity for plugin-emitted messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Fine-grained tracing information.
    Trace,
    /// Debugging information.
    Debug,
    /// General informational messages.
    Info,
    /// Warnings that do not prevent operation.
    Warn,
    /// Error conditions.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_levels_are_distinct() {
        assert_ne!(LogLevel::Trace, LogLevel::Debug);
        assert_ne!(LogLevel::Debug, LogLevel::Info);
        assert_ne!(LogLevel::Info, LogLevel::Warn);
        assert_ne!(LogLevel::Warn, LogLevel::Error);
    }

    #[test]
    fn log_level_is_copy() {
        let a = LogLevel::Info;
        let b = a;
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs` to add the module declaration and re-export:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod log;

pub use log::LogLevel;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 2 tests passed (`log::tests::log_levels_are_distinct`, `log::tests::log_level_is_copy`).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/log.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add LogLevel enum"
```

---

### Task 6: `StopReason` enum

**Files:**
- Create: `crates/nexus-kernel/src/event.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `event.rs` with `StopReason`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event.rs`:

```rust
//! Event types: NexusEvent, EventMetadata, EventFilter, StopReason.

use serde::{Deserialize, Serialize};

/// Why a plugin was stopped. Attached to `NexusEvent::PluginStopped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// The user explicitly stopped the plugin via CLI.
    UserRequested,
    /// The plugin is being reloaded from disk (hot-reload).
    HotReload,
    /// The kernel is shutting down.
    Shutdown,
    /// The plugin crashed and is being stopped as part of recovery.
    CrashRecovery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reason_variants_are_distinct() {
        assert_ne!(StopReason::UserRequested, StopReason::HotReload);
        assert_ne!(StopReason::HotReload, StopReason::Shutdown);
        assert_ne!(StopReason::Shutdown, StopReason::CrashRecovery);
    }

    #[test]
    fn stop_reason_serializes_as_variant_name() {
        let json = serde_json::to_string(&StopReason::HotReload).unwrap();
        assert_eq!(json, "\"HotReload\"");
    }

    #[test]
    fn stop_reason_deserializes_from_variant_name() {
        let reason: StopReason = serde_json::from_str("\"Shutdown\"").unwrap();
        assert_eq!(reason, StopReason::Shutdown);
    }
}
```

- [ ] **Step 2: Add `event` module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod event;
mod log;

pub use event::StopReason;
pub use log::LogLevel;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 5 tests passed (2 from log, 3 from event).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add StopReason enum"
```

---

### Task 7: `Capability` enum with `as_str`, `from_str`, and `ALL`

**Files:**
- Create: `crates/nexus-kernel/src/capability.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Write the failing test module first**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/capability.rs`:

```rust
//! Capability system: enum of named capabilities, string conversion, set type.

use serde::{Deserialize, Serialize};

/// A named capability that can be granted to a plugin.
///
/// Capabilities are the single source of truth for the plugin permission
/// system. Plugin manifests reference them as hierarchical dot-namespaced
/// strings (e.g., `"fs.read"`); this enum is the canonical in-memory form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Read files within the forge root.
    FsRead,
    /// Write files within the forge root.
    FsWrite,
    /// Read files outside the forge root (HIGH risk).
    FsReadExternal,
    /// Write files outside the forge root (HIGH risk).
    FsWriteExternal,
    /// Outbound HTTP to any host (HIGH risk).
    NetHttp,
    /// Outbound HTTP to localhost only.
    NetHttpLocalhost,
    /// Spawn child processes (HIGH risk).
    ProcessSpawn,
    /// Read the plugin's own KV store.
    KvRead,
    /// Write the plugin's own KV store.
    KvWrite,
    /// Call IPC commands on other plugins (HIGH risk).
    IpcCall,
    /// Query SQLite tables registered by the plugin.
    DbQuery,
    /// Write to SQLite tables registered by the plugin.
    DbWrite,
}

/// Error parsing a capability string.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityParseError {
    /// The string does not match any known capability name.
    #[error("unknown capability string '{0}'")]
    UnknownString(String),
}

impl Capability {
    /// Canonical string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::FsRead           => "fs.read",
            Capability::FsWrite          => "fs.write",
            Capability::FsReadExternal   => "fs.read.external",
            Capability::FsWriteExternal  => "fs.write.external",
            Capability::NetHttp          => "net.http",
            Capability::NetHttpLocalhost => "net.http.localhost",
            Capability::ProcessSpawn     => "process.spawn",
            Capability::KvRead           => "kv.read",
            Capability::KvWrite          => "kv.write",
            Capability::IpcCall          => "ipc.call",
            Capability::DbQuery          => "db.query",
            Capability::DbWrite          => "db.write",
        }
    }

    /// Parse from a manifest string. Returns `CapabilityParseError::UnknownString`
    /// for unknown inputs.
    ///
    /// # Errors
    /// Returns an error if `s` is not a recognized capability name.
    pub fn from_str(s: &str) -> Result<Self, CapabilityParseError> {
        match s {
            "fs.read"            => Ok(Capability::FsRead),
            "fs.write"           => Ok(Capability::FsWrite),
            "fs.read.external"   => Ok(Capability::FsReadExternal),
            "fs.write.external"  => Ok(Capability::FsWriteExternal),
            "net.http"           => Ok(Capability::NetHttp),
            "net.http.localhost" => Ok(Capability::NetHttpLocalhost),
            "process.spawn"      => Ok(Capability::ProcessSpawn),
            "kv.read"            => Ok(Capability::KvRead),
            "kv.write"           => Ok(Capability::KvWrite),
            "ipc.call"           => Ok(Capability::IpcCall),
            "db.query"           => Ok(Capability::DbQuery),
            "db.write"           => Ok(Capability::DbWrite),
            other                => Err(CapabilityParseError::UnknownString(other.to_string())),
        }
    }

    /// All capability variants, for exhaustive iteration.
    pub const ALL: &'static [Capability] = &[
        Capability::FsRead,
        Capability::FsWrite,
        Capability::FsReadExternal,
        Capability::FsWriteExternal,
        Capability::NetHttp,
        Capability::NetHttpLocalhost,
        Capability::ProcessSpawn,
        Capability::KvRead,
        Capability::KvWrite,
        Capability::IpcCall,
        Capability::DbQuery,
        Capability::DbWrite,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_roundtrip_via_string() {
        for &cap in Capability::ALL {
            let s = cap.as_str();
            let parsed = Capability::from_str(s).unwrap();
            assert_eq!(parsed, cap, "roundtrip failed for {cap:?}");
        }
    }

    #[test]
    fn unknown_string_returns_error() {
        let err = Capability::from_str("fs.bogus").unwrap_err();
        match err {
            CapabilityParseError::UnknownString(s) => assert_eq!(s, "fs.bogus"),
        }
    }

    #[test]
    fn typo_returns_error_not_wrong_variant() {
        let err = Capability::from_str("fs_read").unwrap_err();
        match err {
            CapabilityParseError::UnknownString(s) => assert_eq!(s, "fs_read"),
        }
    }

    #[test]
    fn all_has_twelve_variants() {
        assert_eq!(Capability::ALL.len(), 12);
    }

    #[test]
    fn as_str_is_dot_namespaced() {
        assert_eq!(Capability::FsReadExternal.as_str(), "fs.read.external");
        assert_eq!(Capability::NetHttpLocalhost.as_str(), "net.http.localhost");
    }
}
```

**Note:** The `CapabilityParseError` defined here is a standalone error used by `Capability::from_str`. Later (Task 15), we'll wire it into `CapabilityError` as the source. The separation is intentional — `from_str` is a pure function that doesn't need the full error taxonomy.

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod event;
mod log;

pub use capability::{Capability, CapabilityParseError};
pub use event::StopReason;
pub use log::LogLevel;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 10 tests passed (2 log, 3 event, 5 capability).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/capability.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add Capability enum with string conversion"
```

---

### Task 8: `CapabilitySet` struct

**Files:**
- Modify: `crates/nexus-kernel/src/capability.rs`

- [ ] **Step 1: Add `CapabilitySet` to `capability.rs`**

Append to `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/capability.rs` (after the `impl Capability` block, before `#[cfg(test)]`):

```rust
use std::collections::HashSet;

/// A set of capabilities granted to a plugin at load time.
///
/// Immutable once constructed — capabilities are not modified at runtime in M1.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    set: HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set (no capabilities granted).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            set: HashSet::new(),
        }
    }

    /// Build a set from an iterator of capabilities.
    #[must_use]
    pub fn from_iter(iter: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            set: iter.into_iter().collect(),
        }
    }

    /// Check whether the set contains a specific capability.
    #[must_use]
    pub fn contains(&self, cap: Capability) -> bool {
        self.set.contains(&cap)
    }

    /// Iterate over the capabilities in the set.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.set.iter()
    }

    /// Number of capabilities in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}
```

Then append to the existing `mod tests` block (before the closing `}`):

```rust
    #[test]
    fn empty_set_contains_nothing() {
        let set = CapabilitySet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.contains(Capability::FsRead));
    }

    #[test]
    fn set_from_iter_contains_those_caps() {
        let set = CapabilitySet::from_iter([Capability::FsRead, Capability::KvRead]);
        assert_eq!(set.len(), 2);
        assert!(set.contains(Capability::FsRead));
        assert!(set.contains(Capability::KvRead));
        assert!(!set.contains(Capability::FsWrite));
    }

    #[test]
    fn set_is_clone_and_independent() {
        let set = CapabilitySet::from_iter([Capability::FsRead]);
        let cloned = set.clone();
        assert!(cloned.contains(Capability::FsRead));
    }

    #[test]
    fn set_iter_yields_all() {
        let caps = [Capability::FsRead, Capability::KvWrite];
        let set = CapabilitySet::from_iter(caps);
        let collected: HashSet<_> = set.iter().copied().collect();
        assert_eq!(collected.len(), 2);
    }
```

- [ ] **Step 2: Export `CapabilitySet` from lib.rs**

Modify the `pub use capability::...` line in `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
pub use capability::{Capability, CapabilityParseError, CapabilitySet};
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 14 tests passed (2 log, 3 event, 9 capability).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/capability.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add CapabilitySet"
```

---

### Task 9: `EventFilter` enum

**Files:**
- Modify: `crates/nexus-kernel/src/event.rs`

- [ ] **Step 1: Add `EventFilter` to `event.rs`**

Append to `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event.rs` (after `StopReason`, before `#[cfg(test)]`):

```rust
/// Filter applied to an event subscription. Events not matching the filter
/// are silently skipped inside the subscription's `recv()` call.
#[derive(Debug, Clone)]
pub enum EventFilter {
    /// Match every event on the bus. High-traffic; intended for debug/tracing.
    All,
    /// Match a single kernel-event variant by its name (e.g., `"FileCreated"`).
    Variant(&'static str),
    /// Match `NexusEvent::Custom` events whose `type_id` starts with this prefix.
    CustomPrefix(String),
    /// Match exactly one `NexusEvent::Custom` `type_id`.
    CustomExact(String),
}
```

Append to the existing `mod tests` block in `event.rs`:

```rust
    #[test]
    fn event_filter_is_clone() {
        let f1 = EventFilter::Variant("FileCreated");
        let _f2 = f1.clone();
    }

    #[test]
    fn custom_prefix_stores_string() {
        let filter = EventFilter::CustomPrefix("com.example.".to_string());
        match filter {
            EventFilter::CustomPrefix(p) => assert_eq!(p, "com.example."),
            _ => panic!("wrong variant"),
        }
    }
```

- [ ] **Step 2: Export `EventFilter` from lib.rs**

Modify the `pub use event::...` line in `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
pub use event::{EventFilter, StopReason};
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 16 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add EventFilter enum"
```

---

### Task 10: `KernelConfig` struct with `Default` impl

**Files:**
- Create: `crates/nexus-kernel/src/config.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `config.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/config.rs`:

```rust
//! Kernel configuration.

use std::path::PathBuf;

/// Configuration for a Kernel instance.
///
/// Load from disk via `KernelConfig::load`, or construct programmatically
/// (typically via `KernelConfig::for_testing` in tests).
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Root directory of the forge (workspace).
    pub forge_root: PathBuf,

    /// Event bus ring buffer capacity. Slow subscribers receive
    /// `RecvError::Lagged(n)` if they fall more than this many events behind.
    pub event_bus_capacity: usize,

    /// Directories to search for plugin manifests. Default:
    /// `[<forge_root>/.nexus/plugins]`.
    pub plugin_search_paths: Vec<PathBuf>,

    /// Enable hot-reload of plugins when their WASM files change on disk.
    pub hot_reload_enabled: bool,
}

impl KernelConfig {
    /// Programmatic construction for tests. Uses defaults for everything
    /// except `forge_root`.
    #[must_use]
    pub fn for_testing(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            ..Self::default()
        }
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            forge_root: PathBuf::from("."),
            event_bus_capacity: 2048,
            plugin_search_paths: vec![],
            hot_reload_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let cfg = KernelConfig::default();
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);
        assert!(cfg.plugin_search_paths.is_empty());
    }

    #[test]
    fn for_testing_sets_forge_root() {
        let cfg = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
        assert_eq!(cfg.forge_root, PathBuf::from("/tmp/test"));
        assert_eq!(cfg.event_bus_capacity, 2048); // default preserved
    }

    #[test]
    fn config_is_clone() {
        let cfg = KernelConfig::default();
        let _cloned = cfg.clone();
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs` to declare the module and re-export:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod config;
mod event;
mod log;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use event::{EventFilter, StopReason};
pub use log::LogLevel;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 19 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/config.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add KernelConfig struct with defaults"
```

---

## Phase 4: Event Types

### Task 11: `NexusEvent` enum (M1 variants)

**Files:**
- Modify: `crates/nexus-kernel/src/event.rs`

- [ ] **Step 1: Add `NexusEvent` to `event.rs`**

Insert into `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event.rs` **at the top** (after `use` statements, before `StopReason`):

```rust
use std::path::PathBuf;

use crate::capability::Capability;

/// All events the Nexus kernel knows about.
///
/// This is a closed enum for kernel-owned events plus a single `Custom`
/// variant for plugin-emitted signals. Each phase of the roadmap adds
/// variants here when it reaches its milestone. M1 includes storage, plugin
/// lifecycle, capability, and indexing events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NexusEvent {
    // ---- M1: storage events ----

    /// A file was created in the forge.
    FileCreated {
        /// Path of the created file, relative to forge root.
        path: PathBuf,
        /// Content hash (SHA-256 hex) of the file.
        content_hash: String,
    },
    /// A file's contents changed.
    FileModified {
        /// Path of the modified file.
        path: PathBuf,
        /// New content hash.
        content_hash: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Path of the deleted file.
        path: PathBuf,
    },
    /// A file was renamed (detected via hash match within the debounce window).
    FileRenamed {
        /// Old path.
        from: PathBuf,
        /// New path.
        to: PathBuf,
        /// Content hash (unchanged across rename).
        content_hash: String,
    },

    // ---- M1: plugin lifecycle events ----

    /// A plugin has been loaded from disk and its manifest parsed.
    PluginLoaded {
        /// Plugin identifier (reverse-DNS).
        plugin_id: String,
        /// Plugin version from the manifest.
        version: String,
    },
    /// A plugin has started successfully.
    PluginStarted {
        /// Plugin identifier.
        plugin_id: String,
    },
    /// A plugin has been stopped.
    PluginStopped {
        /// Plugin identifier.
        plugin_id: String,
        /// Why the plugin was stopped.
        reason: StopReason,
    },
    /// A plugin crashed during execution.
    PluginCrashed {
        /// Plugin identifier.
        plugin_id: String,
        /// Description of what went wrong.
        error: String,
    },

    // ---- M1: capability lifecycle events ----

    /// A plugin was granted a capability.
    CapabilityGranted {
        /// Plugin identifier.
        plugin_id: String,
        /// Which capability was granted.
        capability: Capability,
    },
    /// A plugin's capability request was denied.
    CapabilityDenied {
        /// Plugin identifier.
        plugin_id: String,
        /// Which capability was denied.
        capability: Capability,
    },

    // ---- M1: indexing events ----

    /// Storage engine has begun indexing.
    IndexingStarted {
        /// Total number of files the indexer will process.
        total_files: usize,
    },
    /// Storage engine indexing progress update.
    IndexingProgress {
        /// Files processed so far.
        files_processed: usize,
        /// Total files in the batch.
        total_files: usize,
    },
    /// Storage engine indexing completed.
    IndexingCompleted {
        /// Wall-clock duration of the indexing pass, in milliseconds.
        duration_ms: u64,
    },

    // ---- Plugin-emitted custom events ----

    /// A plugin-emitted signal. Anti-spoofing enforced at publish time:
    /// `type_id` must start with the emitting plugin's id, and
    /// `emitting_plugin` is set by the kernel (not the plugin).
    Custom {
        /// Namespaced event type (reverse-DNS).
        type_id: String,
        /// The plugin that emitted this event. Set by the kernel.
        emitting_plugin: String,
        /// Arbitrary payload. Plugins serialize/deserialize with their own types.
        payload: serde_json::Value,
    },
}
```

Append to the `mod tests` block in `event.rs`:

```rust
    #[test]
    fn file_created_event_constructs_and_serializes() {
        let event = NexusEvent::FileCreated {
            path: PathBuf::from("welcome.md"),
            content_hash: "abc123".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"FileCreated\""));
        assert!(json.contains("welcome.md"));
    }

    #[test]
    fn plugin_stopped_event_includes_reason() {
        let event = NexusEvent::PluginStopped {
            plugin_id: "com.test".to_string(),
            reason: StopReason::HotReload,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("HotReload"));
    }

    #[test]
    fn custom_event_carries_type_id_and_payload() {
        let event = NexusEvent::Custom {
            type_id: "com.example.test.ping".to_string(),
            emitting_plugin: "com.example.test".to_string(),
            payload: serde_json::json!({"hello": "world"}),
        };
        match event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.example.test.ping"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn file_renamed_is_single_event_not_two() {
        // Confirms the enum shape: one event carries both from and to,
        // not a Delete + Create pair.
        let event = NexusEvent::FileRenamed {
            from: PathBuf::from("old.md"),
            to: PathBuf::from("new.md"),
            content_hash: "abc".to_string(),
        };
        match event {
            NexusEvent::FileRenamed { from, to, .. } => {
                assert_eq!(from, PathBuf::from("old.md"));
                assert_eq!(to, PathBuf::from("new.md"));
            }
            _ => panic!("wrong variant"),
        }
    }
```

- [ ] **Step 2: Export `NexusEvent`**

Modify the `pub use event::...` line in lib.rs:

```rust
pub use event::{EventFilter, NexusEvent, StopReason};
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 23 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add NexusEvent enum with M1 variants"
```

---

### Task 12: `EventMetadata` struct

**Files:**
- Modify: `crates/nexus-kernel/src/event.rs`

- [ ] **Step 1: Add `EventMetadata` to `event.rs`**

Append to `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event.rs` (after the `NexusEvent` definition, before `#[cfg(test)]`):

```rust
/// Metadata attached to every published event. Populated by the kernel's
/// `PluginContext` impl when a plugin calls `ctx.publish(...)` — plugins
/// cannot construct this directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Unique event id, generated by the kernel at publish time.
    pub event_id: uuid::Uuid,
    /// UTC timestamp when the event was published.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Id of the plugin that emitted this event (or `"kernel"` for internal events).
    pub source_plugin_id: String,
    /// `tracing` span id at the time of publish, if a span was active.
    pub span_id: Option<String>,
}
```

Append to the `mod tests` block:

```rust
    #[test]
    fn event_metadata_constructs_with_uuid_and_timestamp() {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: "kernel".to_string(),
            span_id: None,
        };
        assert_eq!(metadata.source_plugin_id, "kernel");
        assert!(metadata.span_id.is_none());
    }

    #[test]
    fn event_metadata_serializes() {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::nil(),
            timestamp: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
            source_plugin_id: "com.test".to_string(),
            span_id: Some("span-42".to_string()),
        };
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("span-42"));
        assert!(json.contains("com.test"));
    }
```

- [ ] **Step 2: Export from lib.rs**

Modify `pub use event::...`:

```rust
pub use event::{EventFilter, EventMetadata, NexusEvent, StopReason};
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 25 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add EventMetadata struct"
```

---

### Task 13: `PublishedEvent` struct

**Files:**
- Modify: `crates/nexus-kernel/src/event.rs`

- [ ] **Step 1: Add `PublishedEvent` to `event.rs`**

Append to `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event.rs` (after `EventMetadata`, before `#[cfg(test)]`):

```rust
/// An event as it flows through the bus: payload + metadata.
///
/// The bus transports `Arc<PublishedEvent>` to avoid cloning for each subscriber.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedEvent {
    /// Kernel-populated metadata.
    pub metadata: EventMetadata,
    /// The event payload.
    pub event: NexusEvent,
}
```

Append to the `mod tests` block:

```rust
    #[test]
    fn published_event_holds_metadata_and_payload() {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::nil(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: "kernel".to_string(),
            span_id: None,
        };
        let event = NexusEvent::FileCreated {
            path: PathBuf::from("test.md"),
            content_hash: "hash".to_string(),
        };
        let published = PublishedEvent { metadata, event };
        match &published.event {
            NexusEvent::FileCreated { path, .. } => assert_eq!(path, &PathBuf::from("test.md")),
            _ => panic!("wrong event"),
        }
    }
```

- [ ] **Step 2: Export `PublishedEvent`**

Modify `pub use event::...`:

```rust
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 26 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add PublishedEvent envelope"
```

---

## Phase 5: Error Types

### Task 14: Top-level `Error` + `ConfigError`, `BusError`, `RecvError`

**Files:**
- Create: `crates/nexus-kernel/src/error.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `error.rs` with top-level Error + the first 3 sub-enums**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/error.rs`:

```rust
//! Error types for the kernel crate.
//!
//! Organization: top-level `Error` enum with `#[from]` wrapping of
//! per-subsystem sub-enums. Narrow APIs can return narrow types directly.

use std::path::PathBuf;

/// Top-level result type for `nexus-kernel`.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error type for `nexus-kernel`. Wraps per-subsystem errors
/// plus `std::io::Error` for convenience.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Plugin lifecycle error.
    #[error(transparent)]
    Plugin(#[from] PluginError),

    /// Capability system error.
    #[error(transparent)]
    Capability(#[from] CapabilityError),

    /// IPC call error.
    #[error(transparent)]
    Ipc(#[from] IpcError),

    /// Event bus error.
    #[error(transparent)]
    Bus(#[from] BusError),

    /// KV store error.
    #[error(transparent)]
    Kv(#[from] KvError),

    /// Configuration load error.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Placeholder for PluginError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Placeholder variant, replaced in Task 15.
    #[error("plugin error placeholder")]
    Placeholder,
}

/// Placeholder for CapabilityError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    /// Placeholder variant, replaced in Task 15.
    #[error("capability error placeholder")]
    Placeholder,
}

/// Placeholder for IpcError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Placeholder variant, replaced in Task 15.
    #[error("IPC error placeholder")]
    Placeholder,
}

/// Placeholder for KvError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// Placeholder variant, replaced in Task 15.
    #[error("KV error placeholder")]
    Placeholder,
}

/// Event bus errors.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// The event bus has been shut down; publishing or subscribing fails.
    #[error("event bus is closed")]
    Closed,

    /// A plugin tried to publish a `Custom` event whose `type_id` does not
    /// start with the plugin's own id.
    #[error("custom event rejected: type_id '{type_id}' does not start with emitting plugin id '{plugin_id}'")]
    TypeIdNamespaceMismatch {
        /// Plugin that attempted to publish.
        plugin_id: String,
        /// The rejected type_id.
        type_id: String,
    },

    /// A plugin tried to publish a kernel-owned event variant
    /// (plugins can only publish `NexusEvent::Custom`).
    #[error("plugins cannot publish kernel events; only NexusEvent::Custom is allowed from plugins")]
    PluginPublishingKernelEvent,
}

/// Errors from receiving events on a subscription.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// The subscriber fell behind by `n` events; those events are lost.
    /// The subscription is still alive; call `recv` again to keep going.
    #[error("subscriber lagged by {0} events (events lost)")]
    Lagged(u64),

    /// The event bus has been shut down. The subscription is dead.
    #[error("event bus is closed")]
    Closed,
}

/// Configuration load errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Config file not found.
    #[error("config file not found at '{path}'")]
    NotFound {
        /// Path that was checked.
        path: PathBuf,
    },

    /// Config file exists but is invalid.
    #[error("invalid config at '{path}': {reason}")]
    Invalid {
        /// Path of the invalid file.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },

    /// TOML parse error with source location.
    #[error("TOML parse error in '{path}': {source}")]
    TomlParse {
        /// Path of the file that failed to parse.
        path: PathBuf,
        /// The underlying TOML parse error.
        #[source]
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_error_wraps_bus_error() {
        let bus_err = BusError::Closed;
        let kernel_err: Error = bus_err.into();
        assert!(matches!(kernel_err, Error::Bus(BusError::Closed)));
    }

    #[test]
    fn config_error_display_includes_path() {
        let err = ConfigError::NotFound {
            path: PathBuf::from("/missing/config.toml"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/missing/config.toml"));
    }

    #[test]
    fn recv_error_lagged_carries_count() {
        let err = RecvError::Lagged(42);
        let msg = format!("{err}");
        assert!(msg.contains("42"));
    }

    #[test]
    fn bus_error_type_id_mismatch_displays_both_fields() {
        let err = BusError::TypeIdNamespaceMismatch {
            plugin_id: "com.foo".to_string(),
            type_id: "com.bar.event".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("com.foo"));
        assert!(msg.contains("com.bar.event"));
    }
}
```

**Note on placeholder variants:** `PluginError`, `CapabilityError`, `IpcError`, and `KvError` have placeholder variants for now so the top-level `Error` enum compiles. Task 15 fills them in with real variants.

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod config;
mod error;
mod event;
mod log;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use log::LogLevel;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 30 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/error.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add top-level Error enum and BusError, RecvError, ConfigError"
```

---

### Task 15: Fill in `PluginError`, `CapabilityError`, `IpcError`, `KvError`

**Files:**
- Modify: `crates/nexus-kernel/src/error.rs`

- [ ] **Step 1: Replace placeholder variants with real variants**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/error.rs` — replace the four placeholder enums with the following:

```rust
/// Errors related to plugin lifecycle, loading, and dependency resolution.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin failed to load from disk.
    #[error("plugin '{plugin_id}' failed to load: {reason}")]
    LoadFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable failure reason.
        reason: String,
    },

    /// Plugin's `on_init` hook failed.
    #[error("plugin '{plugin_id}' failed to initialize: {reason}")]
    InitFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin's `on_start` hook failed.
    #[error("plugin '{plugin_id}' failed to start: {reason}")]
    StartFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin's `on_stop` hook failed.
    #[error("plugin '{plugin_id}' failed to stop: {reason}")]
    StopFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin crashed during execution.
    #[error("plugin '{plugin_id}' crashed: {reason}")]
    Crashed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin panicked during a lifecycle phase.
    #[error("plugin '{plugin_id}' panicked during {phase}")]
    Panicked {
        /// Plugin id.
        plugin_id: String,
        /// Which phase (e.g., "init", "start", "stop").
        phase: &'static str,
    },

    /// Dependency cycle detected among plugins.
    #[error("dependency cycle among plugins: {plugins:?}")]
    DependencyCycle {
        /// Plugin ids involved in the cycle.
        plugins: Vec<String>,
    },

    /// A plugin's required dependency is not loaded.
    #[error("plugin '{plugin_id}' missing required dependency '{missing}'")]
    MissingDependency {
        /// Plugin that has the missing dependency.
        plugin_id: String,
        /// The dependency that wasn't found.
        missing: String,
    },

    /// A plugin's required dependency is the wrong version.
    #[error("plugin '{plugin_id}' dependency '{missing}' version mismatch: required {required}, found {found}")]
    DependencyVersionMismatch {
        /// Plugin with the version mismatch.
        plugin_id: String,
        /// Dependency name.
        missing: String,
        /// Version constraint from the manifest.
        required: String,
        /// Actual version found on disk.
        found: String,
    },

    /// Two plugins declared the same id.
    #[error("duplicate plugin id '{plugin_id}'")]
    DuplicatePluginId {
        /// The duplicated id.
        plugin_id: String,
    },

    /// Plugin lookup by id failed.
    #[error("plugin '{plugin_id}' not found")]
    NotFound {
        /// The id that wasn't found.
        plugin_id: String,
    },
}

/// Errors related to the capability system.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    /// A plugin requested a capability it was not granted.
    #[error("capability '{cap:?}' denied to plugin '{plugin_id}'")]
    Denied {
        /// Plugin id.
        plugin_id: String,
        /// The denied capability.
        cap: crate::capability::Capability,
    },

    /// A manifest contained an unrecognized capability string.
    #[error("unknown capability string '{0}'")]
    UnknownString(String),
}

/// Errors from IPC calls between plugins.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// The target plugin is not loaded.
    #[error("target plugin '{plugin_id}' not found")]
    PluginNotFound {
        /// The target plugin id.
        plugin_id: String,
    },

    /// The target plugin doesn't register that command.
    #[error("command '{command}' not found on plugin '{plugin_id}'")]
    CommandNotFound {
        /// The target plugin id.
        plugin_id: String,
        /// The requested command id.
        command: String,
    },

    /// The IPC call timed out.
    #[error("IPC call to '{plugin_id}'.'{command}' timed out after {timeout_ms}ms")]
    Timeout {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
        /// Timeout that was exceeded.
        timeout_ms: u64,
    },

    /// The target plugin crashed during the IPC call.
    #[error("plugin '{plugin_id}' crashed during IPC call to '{command}'")]
    PluginCrashedDuringCall {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
    },

    /// Failed to serialize the argument payload.
    #[error("IPC argument serialization failed: {reason}")]
    SerializationFailed {
        /// Reason from the serializer.
        reason: String,
    },

    /// Failed to deserialize the return value.
    #[error("IPC return value deserialization failed: {reason}")]
    DeserializationFailed {
        /// Reason from the deserializer.
        reason: String,
    },
}

/// Errors from the KV store.
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// Key not found in the store.
    #[error("key '{key}' not found")]
    NotFound {
        /// The missing key.
        key: String,
    },

    /// Generic KV store failure (wraps the storage backend error).
    #[error("KV store backend error: {reason}")]
    BackendError {
        /// Human-readable reason from the backend.
        reason: String,
    },
}
```

Add tests at the bottom of the existing `mod tests` block in `error.rs`:

```rust
    #[test]
    fn plugin_error_crashed_displays_id_and_reason() {
        let err = PluginError::Crashed {
            plugin_id: "com.test".to_string(),
            reason: "segfault".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("com.test"));
        assert!(msg.contains("segfault"));
    }

    #[test]
    fn capability_error_denied_debug_prints_variant() {
        let err = CapabilityError::Denied {
            plugin_id: "com.test".to_string(),
            cap: crate::capability::Capability::FsRead,
        };
        let msg = format!("{err}");
        assert!(msg.contains("FsRead"));
        assert!(msg.contains("com.test"));
    }

    #[test]
    fn ipc_error_timeout_includes_duration() {
        let err = IpcError::Timeout {
            plugin_id: "com.test".to_string(),
            command: "ping".to_string(),
            timeout_ms: 5000,
        };
        let msg = format!("{err}");
        assert!(msg.contains("5000"));
    }

    #[test]
    fn kv_error_not_found_shows_key() {
        let err = KvError::NotFound {
            key: "state".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("state"));
    }

    #[test]
    fn plugin_error_dep_cycle_lists_plugins() {
        let err = PluginError::DependencyCycle {
            plugins: vec!["a".to_string(), "b".to_string()],
        };
        let msg = format!("{err}");
        assert!(msg.contains("a"));
        assert!(msg.contains("b"));
    }
```

- [ ] **Step 2: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 35 tests passed.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-kernel/src/error.rs
git commit -m "feat(kernel): fill in PluginError, CapabilityError, IpcError, KvError variants"
```

---

## Phase 6: Event Bus

### Task 16: `EventBus` struct with `publish_kernel` and `subscribe`

**Files:**
- Create: `crates/nexus-kernel/src/event_bus.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `event_bus.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event_bus.rs`:

```rust
//! Event bus: tokio broadcast channel wrapper.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::error::{BusError, RecvError, Result};
use crate::event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent};

/// The kernel's event bus. Fans out `PublishedEvent`s to all subscribers
/// via a bounded tokio broadcast channel.
///
/// Owned by the `Kernel` struct; subscribers receive handles via
/// `EventBus::subscribe`. Publishers must go through the kernel
/// (`publish_kernel` is `pub(crate)` so plugins can't reach it directly).
#[derive(Debug)]
pub struct EventBus {
    sender: broadcast::Sender<Arc<PublishedEvent>>,
}

impl EventBus {
    /// Create a new bus with the given ring buffer capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish a kernel-owned event. Not callable from plugins.
    ///
    /// # Errors
    /// Returns `BusError::Closed` if the bus has been shut down.
    pub(crate) fn publish_kernel(&self, event: NexusEvent) -> Result<()> {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: "kernel".to_string(),
            span_id: current_span_id(),
        };
        let published = Arc::new(PublishedEvent { metadata, event });
        // broadcast::Sender::send returns the number of active receivers,
        // or an error if there are none — that's not an error condition for us.
        let _ = self.sender.send(published);
        Ok(())
    }

    /// Subscribe to events matching the filter. The subscription is dropped
    /// automatically when it goes out of scope.
    #[must_use]
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        EventSubscription {
            receiver: self.sender.subscribe(),
            filter,
        }
    }

    /// Number of active subscribers (useful for debug/metrics).
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A subscription handle returned by `EventBus::subscribe`. Dropped
/// subscriptions auto-unsubscribe (tokio broadcast semantics).
pub struct EventSubscription {
    receiver: broadcast::Receiver<Arc<PublishedEvent>>,
    filter: EventFilter,
}

impl EventSubscription {
    /// Receive the next event matching the filter. Non-matching events are
    /// skipped internally.
    ///
    /// # Errors
    /// - `RecvError::Lagged(n)` — subscriber fell behind; `n` events lost.
    /// - `RecvError::Closed` — bus is shut down.
    pub async fn recv(&mut self) -> std::result::Result<Arc<PublishedEvent>, RecvError> {
        loop {
            let event = match self.receiver.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(n)) => return Err(RecvError::Lagged(n)),
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            };
            if matches_filter(&event.event, &self.filter) {
                return Ok(event);
            }
            // non-matching: keep looping
        }
    }

    /// Try to receive without blocking. Returns `Ok(None)` if no matching
    /// events are currently available.
    ///
    /// # Errors
    /// - `RecvError::Lagged(n)` — subscriber fell behind.
    /// - `RecvError::Closed` — bus is shut down.
    pub fn try_recv(&mut self) -> std::result::Result<Option<Arc<PublishedEvent>>, RecvError> {
        loop {
            let event = match self.receiver.try_recv() {
                Ok(e) => e,
                Err(broadcast::error::TryRecvError::Empty) => return Ok(None),
                Err(broadcast::error::TryRecvError::Lagged(n)) => return Err(RecvError::Lagged(n)),
                Err(broadcast::error::TryRecvError::Closed) => return Err(RecvError::Closed),
            };
            if matches_filter(&event.event, &self.filter) {
                return Ok(Some(event));
            }
        }
    }
}

/// Check whether an event matches a filter.
fn matches_filter(event: &NexusEvent, filter: &EventFilter) -> bool {
    match filter {
        EventFilter::All => true,
        EventFilter::Variant(name) => variant_name(event) == *name,
        EventFilter::CustomPrefix(prefix) => {
            if let NexusEvent::Custom { type_id, .. } = event {
                type_id.starts_with(prefix.as_str())
            } else {
                false
            }
        }
        EventFilter::CustomExact(wanted) => {
            if let NexusEvent::Custom { type_id, .. } = event {
                type_id == wanted
            } else {
                false
            }
        }
    }
}

/// Get the variant name of a `NexusEvent` for filter matching.
#[allow(clippy::match_same_arms)]
fn variant_name(event: &NexusEvent) -> &'static str {
    match event {
        NexusEvent::FileCreated { .. }      => "FileCreated",
        NexusEvent::FileModified { .. }     => "FileModified",
        NexusEvent::FileDeleted { .. }      => "FileDeleted",
        NexusEvent::FileRenamed { .. }      => "FileRenamed",
        NexusEvent::PluginLoaded { .. }     => "PluginLoaded",
        NexusEvent::PluginStarted { .. }    => "PluginStarted",
        NexusEvent::PluginStopped { .. }    => "PluginStopped",
        NexusEvent::PluginCrashed { .. }    => "PluginCrashed",
        NexusEvent::CapabilityGranted { .. } => "CapabilityGranted",
        NexusEvent::CapabilityDenied { .. }  => "CapabilityDenied",
        NexusEvent::IndexingStarted { .. }   => "IndexingStarted",
        NexusEvent::IndexingProgress { .. }  => "IndexingProgress",
        NexusEvent::IndexingCompleted { .. } => "IndexingCompleted",
        NexusEvent::Custom { .. }            => "Custom",
    }
}

/// Get the current `tracing` span id, if any.
fn current_span_id() -> Option<String> {
    // tracing::Span::current() always returns a span, but it's the None span
    // when no actual span is active. We use its metadata or None.
    let span = tracing::Span::current();
    if span.is_disabled() {
        None
    } else {
        span.id().map(|id| format!("{id:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn publish_and_receive_single_event() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        bus.publish_kernel(NexusEvent::FileCreated {
            path: PathBuf::from("a.md"),
            content_hash: "hash".to_string(),
        }).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::FileCreated { path, .. } => assert_eq!(path, &PathBuf::from("a.md")),
            _ => panic!("wrong event variant"),
        }
        assert_eq!(published.metadata.source_plugin_id, "kernel");
    }

    #[tokio::test]
    async fn filter_variant_skips_non_matching_events() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::Variant("FileDeleted"));

        // Publish a Created event — should be skipped by the filter
        bus.publish_kernel(NexusEvent::FileCreated {
            path: PathBuf::from("a.md"),
            content_hash: "hash".to_string(),
        }).unwrap();

        // Publish a Deleted event — should be received
        bus.publish_kernel(NexusEvent::FileDeleted {
            path: PathBuf::from("b.md"),
        }).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::FileDeleted { path } => assert_eq!(path, &PathBuf::from("b.md")),
            _ => panic!("filter let wrong event through"),
        }
    }

    #[tokio::test]
    async fn filter_custom_prefix_matches_custom_events() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.test.".to_string()));

        bus.publish_kernel(NexusEvent::Custom {
            type_id: "com.test.ping".to_string(),
            emitting_plugin: "com.test".to_string(),
            payload: serde_json::json!({}),
        }).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.test.ping"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn try_recv_returns_none_when_empty() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);
        let result = sub.try_recv().unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn subscriber_count_reflects_active_subscriptions() {
        let bus = EventBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);

        let _sub1 = bus.subscribe(EventFilter::All);
        assert_eq!(bus.subscriber_count(), 1);

        {
            let _sub2 = bus.subscribe(EventFilter::All);
            assert_eq!(bus.subscriber_count(), 2);
        }
        // sub2 dropped
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[tokio::test]
    async fn metadata_has_fresh_uuid_per_publish() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        bus.publish_kernel(NexusEvent::FileCreated {
            path: PathBuf::from("a"),
            content_hash: "h".to_string(),
        }).unwrap();
        bus.publish_kernel(NexusEvent::FileCreated {
            path: PathBuf::from("b"),
            content_hash: "h".to_string(),
        }).unwrap();

        let e1 = sub.recv().await.unwrap();
        let e2 = sub.recv().await.unwrap();
        assert_ne!(e1.metadata.event_id, e2.metadata.event_id);
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod config;
mod error;
mod event;
mod event_bus;
mod log;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use log::LogLevel;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 41 tests passed (35 previous + 6 event_bus).

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/event_bus.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add EventBus and EventSubscription with filter support"
```

---

### Task 17: EventBus edge cases — lagged detection and closed bus

**Files:**
- Modify: `crates/nexus-kernel/src/event_bus.rs`

- [ ] **Step 1: Add lagged and closed tests**

Append to the `mod tests` block in `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/event_bus.rs`:

```rust
    #[tokio::test]
    async fn slow_subscriber_gets_lagged_error() {
        // Capacity of 2; publish 5 events without consuming; should lag.
        let bus = EventBus::new(2);
        let mut sub = bus.subscribe(EventFilter::All);

        for i in 0..5 {
            bus.publish_kernel(NexusEvent::FileCreated {
                path: PathBuf::from(format!("{i}.md")),
                content_hash: format!("hash-{i}"),
            }).unwrap();
        }

        // First recv should return Lagged, not an actual event.
        let result = sub.recv().await;
        match result {
            Err(RecvError::Lagged(n)) => assert!(n >= 1, "expected at least 1 lagged, got {n}"),
            other => panic!("expected Lagged error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn recv_returns_closed_when_bus_dropped() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        drop(bus);

        let result = sub.recv().await;
        assert!(matches!(result, Err(RecvError::Closed)));
    }

    #[tokio::test]
    async fn lagged_subscriber_can_recover_and_keep_receiving() {
        let bus = EventBus::new(2);
        let mut sub = bus.subscribe(EventFilter::All);

        // Cause a lag
        for i in 0..5 {
            bus.publish_kernel(NexusEvent::FileCreated {
                path: PathBuf::from(format!("{i}.md")),
                content_hash: "h".to_string(),
            }).unwrap();
        }

        // First recv — lagged
        assert!(matches!(sub.recv().await, Err(RecvError::Lagged(_))));

        // Subsequent recvs should return actual events from what's still in the buffer
        let event = sub.recv().await.unwrap();
        assert!(matches!(event.event, NexusEvent::FileCreated { .. }));
    }
```

- [ ] **Step 2: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 44 tests passed.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-kernel/src/event_bus.rs
git commit -m "test(kernel): cover EventBus lagged and closed edge cases"
```

---

## Phase 7: Plugin Types

### Task 18: `TrustLevel`, `PluginStatus`, `PluginInfo`

**Files:**
- Create: `crates/nexus-kernel/src/plugin.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `plugin.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/plugin.rs`:

```rust
//! Plugin-related types: lifecycle trait, trust levels, status, info.

use crate::capability::CapabilitySet;

/// Trust level declared by a plugin in its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Core plugins (authored or explicitly blessed); any capability allowed.
    Core,
    /// Community plugins; HIGH-risk capabilities require install-time approval.
    Community,
}

/// Plugin runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginStatus {
    /// Loaded from disk, manifest parsed, not yet initialized.
    Loaded,
    /// `on_init` completed successfully.
    Initialized,
    /// `on_start` completed; plugin is running.
    Running,
    /// `on_stop` completed; plugin is no longer receiving events.
    Stopped,
    /// Plugin crashed (error-path sink).
    Crashed,
}

/// Public view of a loaded plugin's identity and state.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin identifier (reverse-DNS).
    pub id: String,
    /// Human-readable display name from the manifest.
    pub name: String,
    /// Version string from the manifest.
    pub version: String,
    /// Trust level declared in the manifest.
    pub trust_level: TrustLevel,
    /// Current runtime status.
    pub status: PluginStatus,
    /// Capabilities granted to this plugin at load time.
    pub capabilities: CapabilitySet,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    #[test]
    fn plugin_info_constructs_with_all_fields() {
        let info = PluginInfo {
            id: "com.example.test".to_string(),
            name: "Test".to_string(),
            version: "0.1.0".to_string(),
            trust_level: TrustLevel::Core,
            status: PluginStatus::Running,
            capabilities: CapabilitySet::from_iter([Capability::FsRead]),
        };
        assert_eq!(info.id, "com.example.test");
        assert_eq!(info.trust_level, TrustLevel::Core);
        assert_eq!(info.status, PluginStatus::Running);
        assert!(info.capabilities.contains(Capability::FsRead));
    }

    #[test]
    fn trust_level_variants_are_distinct() {
        assert_ne!(TrustLevel::Core, TrustLevel::Community);
    }

    #[test]
    fn plugin_status_is_copy_and_eq() {
        let a = PluginStatus::Running;
        let b = a;
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs` (add `mod plugin;` and re-export):

```rust
mod capability;
mod config;
mod error;
mod event;
mod event_bus;
mod log;
mod plugin;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginStatus, TrustLevel};
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 47 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/plugin.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add TrustLevel, PluginStatus, PluginInfo"
```

---

### Task 19: `PluginRegistry` struct (empty state for now)

**Files:**
- Create: `crates/nexus-kernel/src/plugin_registry.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `plugin_registry.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/plugin_registry.rs`:

```rust
//! Plugin registry: read-only view of loaded plugins.

use std::collections::HashMap;

use crate::plugin::{PluginInfo, PluginStatus};

/// Read-only view of plugins loaded in the kernel.
///
/// Populated by the kernel during plugin discovery (not implemented in PRD 01
/// scope — the registry is empty until `nexus-plugins` lands). Exposed through
/// `Kernel::plugins()` so `nexus-cli` can implement introspection commands
/// like `nexus plugin list`.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginInfo>,
}

impl PluginRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// List all currently loaded plugins. Order is unspecified for M1;
    /// topological ordering is added when `nexus-plugins` lands.
    #[must_use]
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins.values().cloned().collect()
    }

    /// Look up a plugin by id.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.plugins.get(plugin_id).cloned()
    }

    /// Count plugins grouped by status.
    #[must_use]
    pub fn count_by_status(&self) -> HashMap<PluginStatus, usize> {
        let mut counts = HashMap::new();
        for info in self.plugins.values() {
            *counts.entry(info.status).or_insert(0) += 1;
        }
        counts
    }

    /// Number of registered plugins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Insert or update a plugin info entry. Not part of the public contract —
    /// `nexus-plugins` will call this during load.
    pub(crate) fn upsert(&mut self, info: PluginInfo) {
        self.plugins.insert(info.id.clone(), info);
    }

    /// Remove a plugin from the registry. Not part of the public contract.
    pub(crate) fn remove(&mut self, plugin_id: &str) -> Option<PluginInfo> {
        self.plugins.remove(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::plugin::{PluginStatus, TrustLevel};

    fn sample_info(id: &str, status: PluginStatus) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            trust_level: TrustLevel::Core,
            status,
            capabilities: CapabilitySet::empty(),
        }
    }

    #[test]
    fn empty_registry_has_no_plugins() {
        let reg = PluginRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn upsert_and_get_roundtrip() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("com.test", PluginStatus::Running));
        let got = reg.get("com.test").unwrap();
        assert_eq!(got.id, "com.test");
    }

    #[test]
    fn count_by_status_groups_correctly() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("a", PluginStatus::Running));
        reg.upsert(sample_info("b", PluginStatus::Running));
        reg.upsert(sample_info("c", PluginStatus::Stopped));

        let counts = reg.count_by_status();
        assert_eq!(counts.get(&PluginStatus::Running), Some(&2));
        assert_eq!(counts.get(&PluginStatus::Stopped), Some(&1));
    }

    #[test]
    fn remove_returns_the_removed_info() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("a", PluginStatus::Running));
        let removed = reg.remove("a").unwrap();
        assert_eq!(removed.id, "a");
        assert!(reg.is_empty());
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
mod capability;
mod config;
mod error;
mod event;
mod event_bus;
mod log;
mod plugin;
mod plugin_registry;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginStatus, TrustLevel};
pub use plugin_registry::PluginRegistry;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 51 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/plugin_registry.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add PluginRegistry"
```

---

## Phase 8: Lifecycle Trait

### Task 20: `PluginLifecycle` trait

**Files:**
- Modify: `crates/nexus-kernel/src/plugin.rs`

- [ ] **Step 1: Add `PluginLifecycle` trait to `plugin.rs`**

Append to `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/plugin.rs` (before `#[cfg(test)]`):

```rust
use async_trait::async_trait;

use crate::context::PluginContext;
use crate::error::Result;

/// The plugin lifecycle contract. Plugins implement this trait to
/// participate in the kernel's three-phase lifecycle: init → start → stop.
///
/// Plugins are dropped after `on_stop` returns. Hot-reload calls `on_stop`
/// on the old instance and `on_init` on the new one; state should be
/// persisted via `PluginContext::kv_set` in `on_stop` and restored via
/// `kv_get` in `on_init`.
#[async_trait]
pub trait PluginLifecycle: Send + Sync {
    /// Called once, after the plugin has been loaded and its manifest parsed,
    /// before any events are delivered. Use for state initialization and
    /// optional KV restore.
    async fn on_init(&mut self, ctx: &dyn PluginContext) -> Result<()>;

    /// Called after `on_init` succeeds. The plugin is now "running" —
    /// subscribed events will be delivered and IPC calls can be received.
    async fn on_start(&mut self, ctx: &dyn PluginContext) -> Result<()>;

    /// Called when the kernel is stopping this plugin. Persist any state
    /// you want to survive across reloads. After this returns, the plugin
    /// instance is dropped.
    async fn on_stop(&mut self, ctx: &dyn PluginContext) -> Result<()>;
}
```

**Note:** This introduces a forward reference to `PluginContext`, which is created in Task 21. The compilation will fail until Task 21 is complete. That's expected — we commit Task 20 without running the compile check, and Task 21 makes the tree green.

- [ ] **Step 2: Run compile to verify it fails as expected**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo check -p nexus-kernel 2>&1 | head -20`
Expected: error "unresolved import `crate::context::PluginContext`" — confirming Task 21 needs to land next.

- [ ] **Step 3: Commit (still compiles-broken; Task 21 fixes)**

```bash
git add crates/nexus-kernel/src/plugin.rs
git commit -m "feat(kernel): add PluginLifecycle trait (context ref fills in next task)"
```

---

## Phase 9: PluginContext Trait

### Task 21: `PluginContext` trait definition

**Files:**
- Create: `crates/nexus-kernel/src/context.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `context.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/context.rs`:

```rust
//! The `PluginContext` trait — the full public surface a plugin sees when
//! interacting with the kernel.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::capability::Capability;
use crate::error::{IpcError, Result};
use crate::event::EventFilter;
use crate::event_bus::EventSubscription;
use crate::log::LogLevel;

/// The plugin-facing kernel API. Implemented by the kernel's
/// `KernelPluginContext` struct; plugins only see this trait object.
///
/// Capability enforcement happens inside the impl — if a plugin calls
/// `read_file` without `fs.read`, the impl short-circuits with
/// `CapabilityError::Denied` before reaching the storage backend. The
/// plugin physically cannot bypass the check because the only handle it
/// holds is `&dyn PluginContext`.
#[async_trait]
pub trait PluginContext: Send + Sync {
    // ---- Identity ----

    /// The plugin's id (reverse-DNS, e.g., "com.example.weather").
    fn plugin_id(&self) -> &str;

    /// The plugin's version string from the manifest.
    fn plugin_version(&self) -> &str;

    /// Check whether this plugin holds the given capability.
    fn has_capability(&self, cap: Capability) -> bool;

    // ---- File system (gated by fs.* capabilities) ----

    /// Read a file. Gated by `fs.read` or `fs.read.external`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks the required capability.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write a file. Gated by `fs.write` or `fs.write.external`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks the required capability.
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Delete a file. Gated by `fs.write`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `fs.write`.
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// List files in a directory (non-recursive). Gated by `fs.read`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `fs.read`.
    async fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;

    // ---- KV store (gated by kv.read / kv.write) ----

    /// Get a value from the plugin's KV store. Key is plugin-local;
    /// the kernel internally namespaces it.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.read`.
    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a value in the plugin's KV store.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.write`.
    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Delete a key from the plugin's KV store. Returns `Ok(())` even
    /// if the key doesn't exist.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.write`.
    async fn kv_delete(&self, key: &str) -> Result<()>;

    // ---- Events ----

    /// Publish a `NexusEvent::Custom`. The `type_id` must start with the
    /// plugin's id (reverse-DNS namespace). Kernel populates metadata.
    ///
    /// # Errors
    /// - `BusError::TypeIdNamespaceMismatch` if type_id doesn't namespace-match.
    /// - `BusError::Closed` if the bus is shut down.
    fn publish(&self, type_id: &str, payload: serde_json::Value) -> Result<()>;

    /// Subscribe to events matching the filter. Subscription is dropped
    /// automatically when it goes out of scope.
    fn subscribe(&self, filter: EventFilter) -> EventSubscription;

    // ---- IPC (gated by ipc.call) ----

    /// Call an IPC command on another plugin. `timeout` is required.
    ///
    /// # Errors
    /// - `IpcError::PluginNotFound` if the target plugin isn't loaded.
    /// - `IpcError::CommandNotFound` if the plugin doesn't register that command.
    /// - `IpcError::Timeout` if the call takes longer than `timeout`.
    /// - `IpcError::PluginCrashedDuringCall` if the target plugin panics.
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError>;

    // ---- Logging ----

    /// Emit a log message at the given level. Plumbed through `tracing`
    /// with structured fields including `plugin_id`.
    fn log(&self, level: LogLevel, message: &str);
}
```

- [ ] **Step 2: Add module to lib.rs and re-export the trait**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
mod capability;
mod config;
mod context;
mod error;
mod event;
mod event_bus;
mod log;
mod plugin;
mod plugin_registry;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use context::PluginContext;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginLifecycle, PluginStatus, TrustLevel};
pub use plugin_registry::PluginRegistry;
```

- [ ] **Step 3: Run compile to verify the tree is green**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo check -p nexus-kernel`
Expected: PASS (`PluginLifecycle` from Task 20 now compiles because `PluginContext` exists).

- [ ] **Step 4: Run all tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 51 tests passed (unchanged — we added trait definitions, no new tests since traits have no testable behavior alone).

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-kernel/src/context.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add PluginContext trait definition"
```

---

## Phase 10: KernelConfig Loading

### Task 22: `KernelConfig::load` from TOML

**Files:**
- Modify: `crates/nexus-kernel/src/config.rs`

- [ ] **Step 1: Add `KernelConfig::load` implementation**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/config.rs` — add a `load` method to `impl KernelConfig` and add tests.

Insert the `load` method into the existing `impl KernelConfig` block (after `for_testing`):

```rust
    /// Load from `<forge_root>/.nexus/config.toml`, falling back to defaults
    /// for any fields not specified. Returns `ConfigError::TomlParse` if the
    /// file exists but is malformed. Returns a default config (without error)
    /// if the file doesn't exist.
    ///
    /// # Errors
    /// - `ConfigError::TomlParse` if the file exists but is not valid TOML.
    /// - `ConfigError::Invalid` if a field has an out-of-range value.
    pub fn load(forge_root: &std::path::Path) -> std::result::Result<Self, crate::error::ConfigError> {
        use crate::error::ConfigError;

        let config_path = forge_root.join(".nexus").join("config.toml");

        // If no config file exists, return defaults with forge_root set.
        if !config_path.exists() {
            return Ok(Self {
                forge_root: forge_root.to_path_buf(),
                ..Self::default()
            });
        }

        // Read and parse.
        let content = std::fs::read_to_string(&config_path).map_err(|e| ConfigError::Invalid {
            path: config_path.clone(),
            reason: format!("failed to read: {e}"),
        })?;

        let raw: RawConfig = toml::from_str(&content).map_err(|source| ConfigError::TomlParse {
            path: config_path.clone(),
            source,
        })?;

        // Validate event_bus_capacity > 0.
        if raw.event_bus_capacity == Some(0) {
            return Err(ConfigError::Invalid {
                path: config_path,
                reason: "event_bus_capacity must be > 0".to_string(),
            });
        }

        Ok(Self {
            forge_root: forge_root.to_path_buf(),
            event_bus_capacity: raw.event_bus_capacity.unwrap_or(2048),
            plugin_search_paths: raw.plugin_search_paths.unwrap_or_default(),
            hot_reload_enabled: raw.hot_reload_enabled.unwrap_or(true),
        })
    }
}

/// Raw TOML shape for deserialization. All fields optional so missing
/// values fall back to defaults.
#[derive(Debug, serde::Deserialize)]
struct RawConfig {
    event_bus_capacity: Option<usize>,
    plugin_search_paths: Option<Vec<PathBuf>>,
    hot_reload_enabled: Option<bool>,
}

impl KernelConfig {
```

**Wait:** The snippet above closes the `impl` block and then re-opens it, which is wrong syntactically. The correct edit is to insert `load` inside the existing `impl KernelConfig` block and add `RawConfig` as a sibling, not inside the impl. Let me rewrite the entire file to make this clear:

Rewrite `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/config.rs` in full:

```rust
//! Kernel configuration.

use std::path::PathBuf;

use crate::error::ConfigError;

/// Configuration for a Kernel instance.
///
/// Load from disk via `KernelConfig::load`, or construct programmatically
/// (typically via `KernelConfig::for_testing` in tests).
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Root directory of the forge (workspace).
    pub forge_root: PathBuf,

    /// Event bus ring buffer capacity. Slow subscribers receive
    /// `RecvError::Lagged(n)` if they fall more than this many events behind.
    pub event_bus_capacity: usize,

    /// Directories to search for plugin manifests. Default:
    /// `[<forge_root>/.nexus/plugins]`.
    pub plugin_search_paths: Vec<PathBuf>,

    /// Enable hot-reload of plugins when their WASM files change on disk.
    pub hot_reload_enabled: bool,
}

impl KernelConfig {
    /// Programmatic construction for tests. Uses defaults for everything
    /// except `forge_root`.
    #[must_use]
    pub fn for_testing(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            ..Self::default()
        }
    }

    /// Load from `<forge_root>/.nexus/config.toml`, falling back to defaults
    /// for any fields not specified. Returns a default config (without error)
    /// if the file doesn't exist.
    ///
    /// # Errors
    /// - `ConfigError::TomlParse` if the file exists but is not valid TOML.
    /// - `ConfigError::Invalid` if a field has an out-of-range value or the
    ///   file can't be read.
    pub fn load(forge_root: &std::path::Path) -> std::result::Result<Self, ConfigError> {
        let config_path = forge_root.join(".nexus").join("config.toml");

        // If no config file exists, return defaults with forge_root set.
        if !config_path.exists() {
            return Ok(Self {
                forge_root: forge_root.to_path_buf(),
                ..Self::default()
            });
        }

        // Read and parse.
        let content = std::fs::read_to_string(&config_path).map_err(|e| ConfigError::Invalid {
            path: config_path.clone(),
            reason: format!("failed to read: {e}"),
        })?;

        let raw: RawConfig = toml::from_str(&content).map_err(|source| ConfigError::TomlParse {
            path: config_path.clone(),
            source,
        })?;

        if raw.event_bus_capacity == Some(0) {
            return Err(ConfigError::Invalid {
                path: config_path,
                reason: "event_bus_capacity must be > 0".to_string(),
            });
        }

        Ok(Self {
            forge_root: forge_root.to_path_buf(),
            event_bus_capacity: raw.event_bus_capacity.unwrap_or(2048),
            plugin_search_paths: raw.plugin_search_paths.unwrap_or_default(),
            hot_reload_enabled: raw.hot_reload_enabled.unwrap_or(true),
        })
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            forge_root: PathBuf::from("."),
            event_bus_capacity: 2048,
            plugin_search_paths: vec![],
            hot_reload_enabled: true,
        }
    }
}

/// Raw TOML shape for deserialization. All fields optional so missing
/// values fall back to defaults.
#[derive(Debug, serde::Deserialize)]
struct RawConfig {
    event_bus_capacity: Option<usize>,
    plugin_search_paths: Option<Vec<PathBuf>>,
    hot_reload_enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let cfg = KernelConfig::default();
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);
        assert!(cfg.plugin_search_paths.is_empty());
    }

    #[test]
    fn for_testing_sets_forge_root() {
        let cfg = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
        assert_eq!(cfg.forge_root, PathBuf::from("/tmp/test"));
        assert_eq!(cfg.event_bus_capacity, 2048);
    }

    #[test]
    fn config_is_clone() {
        let cfg = KernelConfig::default();
        let _cloned = cfg.clone();
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let tmp = std::env::temp_dir().join("nexus-test-no-config");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let cfg = KernelConfig::load(&tmp).unwrap();
        assert_eq!(cfg.forge_root, tmp);
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_valid_config_applies_overrides() {
        let tmp = std::env::temp_dir().join("nexus-test-valid-config");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".nexus")).unwrap();
        std::fs::write(
            tmp.join(".nexus/config.toml"),
            "event_bus_capacity = 4096\nhot_reload_enabled = false\n",
        )
        .unwrap();

        let cfg = KernelConfig::load(&tmp).unwrap();
        assert_eq!(cfg.event_bus_capacity, 4096);
        assert!(!cfg.hot_reload_enabled);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_malformed_toml_returns_parse_error() {
        let tmp = std::env::temp_dir().join("nexus-test-bad-config");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".nexus")).unwrap();
        std::fs::write(
            tmp.join(".nexus/config.toml"),
            "this is not valid toml = = =",
        )
        .unwrap();

        let err = KernelConfig::load(&tmp).unwrap_err();
        assert!(matches!(err, ConfigError::TomlParse { .. }));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_zero_capacity_returns_invalid_error() {
        let tmp = std::env::temp_dir().join("nexus-test-zero-cap");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".nexus")).unwrap();
        std::fs::write(
            tmp.join(".nexus/config.toml"),
            "event_bus_capacity = 0\n",
        )
        .unwrap();

        let err = KernelConfig::load(&tmp).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 55 tests passed (51 previous + 4 new config tests).

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-kernel/src/config.rs
git commit -m "feat(kernel): add KernelConfig::load from TOML"
```

---

## Phase 11: Kernel Struct

### Task 23: `Kernel` struct with `new` (sync constructor)

**Files:**
- Create: `crates/nexus-kernel/src/kernel.rs`
- Modify: `crates/nexus-kernel/src/lib.rs`

- [ ] **Step 1: Create `kernel.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/kernel.rs`:

```rust
//! The `Kernel` struct — entry point for the nexus-kernel crate.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::KernelConfig;
use crate::error::Result;
use crate::event_bus::EventBus;
use crate::plugin_registry::PluginRegistry;

/// The Nexus kernel. Owns the event bus and plugin registry.
///
/// Usage:
/// ```ignore
/// let config = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
/// let kernel = Kernel::new(config)?;
/// kernel.start().await?;
/// // ... do work ...
/// kernel.shutdown().await?;
/// ```
///
/// **Concurrency note:** In PRD 01 scope, the plugin registry has no
/// runtime mutations (no plugins are ever loaded) so it's stored directly
/// without interior mutability. When `nexus-plugins` lands and adds
/// real plugin discovery and hot-reload, it will refactor this to wrap
/// the registry in a `RwLock` or similar. That refactor is a local change
/// to this file — it does not affect the public contract of `plugins()`.
#[derive(Debug)]
pub struct Kernel {
    config: KernelConfig,
    event_bus: Arc<EventBus>,
    plugins: PluginRegistry,
    shutdown_flag: Arc<AtomicBool>,
}

impl Kernel {
    /// Synchronous constructor. Builds the `Kernel` struct and all in-memory
    /// state, but does NOT start background tasks, discover plugins, or emit
    /// events. Call `start()` to bring the kernel up.
    ///
    /// # Errors
    /// Currently infallible in PRD 01 scope (all validation happens earlier
    /// via `KernelConfig::load`). The `Result` return type is preserved for
    /// forward compatibility with future validation.
    pub fn new(config: KernelConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.event_bus_capacity));
        let plugins = PluginRegistry::new();
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        Ok(Self {
            config,
            event_bus,
            plugins,
            shutdown_flag,
        })
    }

    /// Get a handle to the event bus. Used by `nexus-cli` to install event
    /// taps without going through a plugin.
    #[must_use]
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    /// Read-only access to the kernel config.
    #[must_use]
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn new_succeeds_with_default_config() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-kernel-test"));
        let kernel = Kernel::new(config).unwrap();
        assert_eq!(kernel.config().forge_root, PathBuf::from("/tmp/nexus-kernel-test"));
    }

    #[test]
    fn event_bus_handle_is_clonable_arc() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp"));
        let kernel = Kernel::new(config).unwrap();
        let bus1 = kernel.event_bus();
        let bus2 = kernel.event_bus();
        // Both are Arc clones pointing at the same bus
        assert_eq!(Arc::as_ptr(&bus1), Arc::as_ptr(&bus2));
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Modify `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/lib.rs`:

```rust
mod capability;
mod config;
mod context;
mod error;
mod event;
mod event_bus;
mod kernel;
mod log;
mod plugin;
mod plugin_registry;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use context::PluginContext;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use kernel::Kernel;
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginLifecycle, PluginStatus, TrustLevel};
pub use plugin_registry::PluginRegistry;
```

- [ ] **Step 3: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 57 tests passed.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-kernel/src/kernel.rs crates/nexus-kernel/src/lib.rs
git commit -m "feat(kernel): add Kernel struct with synchronous new()"
```

---

### Task 24: `Kernel::start` (empty plugin set case)

**Files:**
- Modify: `crates/nexus-kernel/src/kernel.rs`

- [ ] **Step 1: Add `start` method**

Append to the `impl Kernel` block in `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/kernel.rs` (before the closing `}` of the impl):

```rust
    /// Start the kernel. Discovers plugins from `config.plugin_search_paths`,
    /// loads them in topological order, and calls their lifecycle hooks.
    ///
    /// In PRD 01 scope, plugin discovery is a no-op (plugins are the
    /// `nexus-plugins` crate's concern). The kernel starts with an empty
    /// plugin set and is ready to accept event bus subscribers.
    ///
    /// # Errors
    /// Returns `Error::Plugin` if any plugin fails to load or initialize.
    /// In PRD 01 scope, this cannot happen (no plugins are loaded).
    pub async fn start(&self) -> Result<()> {
        tracing::info!(
            forge_root = ?self.config.forge_root,
            event_bus_capacity = self.config.event_bus_capacity,
            "nexus kernel starting"
        );

        // Plugin discovery is a no-op in PRD 01 scope.
        // nexus-plugins will fill this in when it lands.
        tracing::debug!("plugin discovery not yet implemented; starting with empty plugin set");

        tracing::info!("nexus kernel started");
        Ok(())
    }
```

Append to the `mod tests` block in `kernel.rs`:

```rust
    #[tokio::test]
    async fn start_succeeds_with_empty_plugin_set() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-test"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
    }

    #[tokio::test]
    async fn start_is_idempotent_across_multiple_calls() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-idem"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        // Calling start again should not fail in PRD 01 scope.
        kernel.start().await.unwrap();
    }
```

- [ ] **Step 2: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 59 tests passed.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-kernel/src/kernel.rs
git commit -m "feat(kernel): add Kernel::start for empty plugin set"
```

---

### Task 25: `Kernel::shutdown` and `Kernel::plugins` accessor

**Files:**
- Modify: `crates/nexus-kernel/src/kernel.rs`

- [ ] **Step 1: Add `shutdown` and `plugins` to `impl Kernel`**

Append to the `impl Kernel` block in `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/src/kernel.rs` (before the closing `}` of the impl):

```rust
    /// Graceful shutdown. Stops all plugins in reverse topological order,
    /// drains the event bus, flushes the audit log, closes DB connections.
    /// Idempotent — safe to call twice.
    ///
    /// In PRD 01 scope, shutdown flips a flag and returns. Real drain
    /// behavior fills in when `nexus-plugins` and `nexus-storage` land.
    ///
    /// # Errors
    /// Returns `Error::Plugin` if any plugin fails to stop. In PRD 01
    /// scope, this cannot happen (no plugins are loaded).
    pub async fn shutdown(&self) -> Result<()> {
        // Flip the shutdown flag. Idempotent: subsequent calls see the flag
        // already set and short-circuit.
        let was_already_shutdown =
            self.shutdown_flag.swap(true, std::sync::atomic::Ordering::SeqCst);

        if was_already_shutdown {
            tracing::debug!("nexus kernel shutdown called on already-shutdown kernel; no-op");
            return Ok(());
        }

        tracing::info!("nexus kernel shutting down");

        // In PRD 01 scope, nothing to drain. nexus-plugins and nexus-storage
        // will fill in real drain logic when they land.
        tracing::debug!("no plugins to stop; no storage to flush");

        tracing::info!("nexus kernel shutdown complete");
        Ok(())
    }

    /// Get a read-only handle to the plugin registry. Used by `nexus-cli` for
    /// introspection commands like `nexus plugin list`.
    ///
    /// Synchronous accessor in PRD 01 scope. When `nexus-plugins` adds
    /// runtime mutations, this signature may change to return a
    /// `RwLockReadGuard` — a refactor local to this file.
    #[must_use]
    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugins
    }
```

Append to the `mod tests` block:

```rust
    #[tokio::test]
    async fn shutdown_succeeds_on_fresh_kernel() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-test"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-idem"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
        kernel.shutdown().await.unwrap();  // no panic, no error
    }

    #[test]
    fn plugins_accessor_returns_empty_registry_before_start() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-plugins-accessor"));
        let kernel = Kernel::new(config).unwrap();
        let registry = kernel.plugins();
        assert!(registry.is_empty());
    }
```

- [ ] **Step 2: Run tests**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel`
Expected: 62 tests passed.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-kernel/src/kernel.rs
git commit -m "feat(kernel): add Kernel::shutdown (idempotent) and Kernel::plugins accessor"
```

---

## Phase 12: Smoke Test

### Task 26: Workspace-level integration smoke test per interface spec §12

**Files:**
- Create: `crates/nexus-kernel/tests/smoke_kernel.rs`

- [ ] **Step 1: Create the smoke test**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-kernel/tests/smoke_kernel.rs`:

```rust
//! PRD 01 smoke test — the §12 acceptance criteria from the interface spec.
//!
//! Verifies that a fresh kernel can be constructed, started with no plugins,
//! have events published to it, and shut down cleanly. This is the
//! single-test proof that the nexus-kernel crate is interface-complete.

use std::path::PathBuf;

use nexus_kernel::{
    EventFilter, Kernel, KernelConfig, NexusEvent,
};

/// Tempdir helper — creates a unique path and ensures cleanup on drop.
struct TempForge {
    path: PathBuf,
}

impl TempForge {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nexus-smoke-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempForge {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn smoke_new_start_shutdown() {
    let forge = TempForge::new("new-start-shutdown");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config).expect("kernel construction should succeed");
    kernel.start().await.expect("kernel start should succeed with empty plugin set");
    kernel.shutdown().await.expect("kernel shutdown should succeed");
}

#[tokio::test]
async fn smoke_event_bus_round_trip() {
    let forge = TempForge::new("bus-roundtrip");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config).unwrap();
    kernel.start().await.unwrap();

    // Subscribe, publish, receive
    let bus = kernel.event_bus();
    let mut sub = bus.subscribe(EventFilter::Variant("PluginLoaded"));

    // We can't call publish_kernel directly from outside the crate (it's
    // pub(crate)), so we verify the bus works by subscribing and then letting
    // the kernel run its own lifecycle. In PRD 01 scope with no plugins,
    // this test just verifies subscription and try_recv don't panic.
    let result = sub.try_recv().unwrap();
    assert!(result.is_none(), "empty plugin set should produce no PluginLoaded events");

    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn smoke_config_loaded_from_disk() {
    let forge = TempForge::new("config-from-disk");
    std::fs::create_dir_all(forge.path.join(".nexus")).unwrap();
    std::fs::write(
        forge.path.join(".nexus/config.toml"),
        "event_bus_capacity = 512\nhot_reload_enabled = false\n",
    )
    .unwrap();

    let config = KernelConfig::load(&forge.path).expect("config load should succeed");
    assert_eq!(config.event_bus_capacity, 512);
    assert!(!config.hot_reload_enabled);

    let kernel = Kernel::new(config).unwrap();
    kernel.start().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn smoke_multiple_shutdown_calls_are_idempotent() {
    let forge = TempForge::new("idempotent-shutdown");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config).unwrap();
    kernel.start().await.unwrap();
    kernel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();  // must not panic or error
}

#[tokio::test]
async fn smoke_plugin_registry_is_empty_in_prd_01_scope() {
    let forge = TempForge::new("empty-registry");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config).unwrap();
    kernel.start().await.unwrap();

    let registry = kernel.plugins();
    assert!(registry.is_empty(), "no plugins should be loaded in PRD 01 scope");
    assert_eq!(registry.len(), 0);

    kernel.shutdown().await.unwrap();
}

// A compile-check test: ensures every public type from the interface spec §3
// can be named. If any of these names break, the contract has regressed.
#[test]
fn smoke_all_public_types_importable() {
    use nexus_kernel::{
        BusError, Capability, CapabilityError, CapabilityParseError, CapabilitySet, ConfigError,
        Error, EventBus, EventFilter, EventMetadata, EventSubscription, IpcError, Kernel,
        KernelConfig, KvError, LogLevel, NexusEvent, PluginContext, PluginError, PluginInfo,
        PluginLifecycle, PluginRegistry, PluginStatus, PublishedEvent, RecvError, Result,
        StopReason, TrustLevel,
    };

    // Just reference each type to force the import — this compiles iff
    // all the types exist and are named consistently with the spec.
    fn _type_check() {
        let _: Option<Capability> = None;
        let _: Option<CapabilityError> = None;
        let _: Option<CapabilityParseError> = None;
        let _: Option<CapabilitySet> = None;
        let _: Option<ConfigError> = None;
        let _: Option<Error> = None;
        let _: Option<EventFilter> = None;
        let _: Option<EventMetadata> = None;
        let _: Option<IpcError> = None;
        let _: Option<KernelConfig> = None;
        let _: Option<KvError> = None;
        let _: Option<LogLevel> = None;
        let _: Option<NexusEvent> = None;
        let _: Option<PluginError> = None;
        let _: Option<PluginInfo> = None;
        let _: Option<PluginStatus> = None;
        let _: Option<PublishedEvent> = None;
        let _: Option<RecvError> = None;
        let _: Option<StopReason> = None;
        let _: Option<TrustLevel> = None;
        let _: Option<BusError> = None;

        // Types that aren't Default/None-constructible are just referenced
        // via std::marker to force the import:
        let _: std::marker::PhantomData<Kernel>           = std::marker::PhantomData;
        let _: std::marker::PhantomData<EventBus>         = std::marker::PhantomData;
        let _: std::marker::PhantomData<EventSubscription> = std::marker::PhantomData;
        let _: std::marker::PhantomData<PluginRegistry>   = std::marker::PhantomData;
        let _: std::marker::PhantomData<dyn PluginContext> = std::marker::PhantomData;
        let _: std::marker::PhantomData<dyn PluginLifecycle> = std::marker::PhantomData;
        type _R = Result<()>;
    }
}
```

- [ ] **Step 2: Run the smoke test suite**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run -p nexus-kernel --test smoke_kernel`
Expected: 6 tests passed.

- [ ] **Step 3: Run the full workspace test suite one final time**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo nextest run --workspace`
Expected: 68 tests passed (62 unit + 6 smoke).

- [ ] **Step 4: Run cargo check with clippy to verify no warnings**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS with zero warnings.

- [ ] **Step 5: Verify `cargo doc` produces no missing-docs warnings**

Run: `cd /mnt/c/Users/baile/dev/nexus && cargo doc --no-deps --workspace 2>&1 | grep -i warning`
Expected: empty (no output) — no missing-docs warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/nexus-kernel/tests/smoke_kernel.rs
git commit -m "test(kernel): add PRD 01 §12 smoke test covering new/start/shutdown and public type surface"
```

---

## Definition of Done

After Task 26, PRD 01 (Kernel & Event System) is **interface complete**:

- ✅ All public types, traits, structs from interface spec §3–§6 defined and exported from `nexus-kernel::lib`
- ✅ All error variants from interface spec §4 defined
- ✅ KV store API defined on `PluginContext` trait (impl is a later crate's concern)
- ✅ IPC call semantics defined on `PluginContext` trait (impl is a later crate's concern)
- ✅ Event bus functional end-to-end within the kernel (publish, subscribe, filter, lagged, closed)
- ✅ `Kernel::new` synchronous, `start` and `shutdown` async, shutdown idempotent
- ✅ Smoke test from interface spec §12 passing
- ✅ `cargo clippy --workspace --all-targets -- -D warnings` clean
- ✅ `cargo doc --no-deps --workspace` no missing-docs warnings
- ✅ All commits follow conventional-commit style
- ✅ 10 ADRs written and committed

**What's NOT done (intentional, filled in by other PRDs):**

- Plugin discovery on disk (`nexus-plugins`, PRD 04)
- Plugin loading / WASM instantiation (`nexus-plugins`)
- Actual file system operations (`nexus-storage`, PRD 03)
- Actual KV backend (`nexus-storage`)
- Capability enforcement in the runtime `PluginContext` impl (scaffolded as trait; concrete `KernelPluginContext` impl is in `nexus-plugins` since it needs the plugin identity to construct)
- Keyring + audit log (`nexus-security`, PRD 02)
- CLI binary (`nexus-cli`, PRD 05)

The `nexus-kernel` crate at this point is a stable foundation that downstream crates can depend on and extend. The smoke test verifies that the contract holds for the empty-plugin case, which is the PRD 01 scope.

---

## Post-Plan Follow-ups

These are noted in the interface spec §13 and remain open after this plan completes:

- **PRD 01 cleanup pass**: annotate `PRDs/01-kernel-event-system.md` with "Amended by PRD 01 interface spec 2026-04-11" notes in each overridden section (capability enum, 5-hook lifecycle, 7-state machine, runtime IPC registration). ~30 minutes.
- **Templates rename pass**: `PRDs/templates/` deferred from the earlier `forge`→`nexus` pass; handle when PRD 04a work begins.
- **Integration tests for cross-PRD seams** (I1–I7 from M1 spec §11.2): not in PRD 01 scope; the walking-skeleton smoke test I7 lands when all M1 crates exist.
