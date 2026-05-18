# ADR 0016: Microkernel Native-Plugin / WASM-Plugin Split

**Date:** 2026-04-24
**Status:** Accepted

## Context

Two classes of plugin coexist in Nexus, with opposite trust and
capability profiles:

- **First-party core plugins** need full native Rust power. They call
  `rusqlite` directly (storage), hold a `notify` file-watcher handle
  (storage), keep a PTY via `portable-pty` (terminal), spawn a tokio
  runtime (agent), and publish directly onto the kernel event bus. None
  of this composes cleanly through a WASI-P2 boundary today.
- **Third-party community plugins** must be isolated. They have no right
  to `rusqlite` handles, a native FS watcher, or raw event-bus access.
  They run in the iframe sandbox (ADR 0015) or, prospectively, a WASM
  runtime.

A single plugin runtime cannot serve both honestly. Either core plugins
pay an isolation tax they don't need, or community plugins get a trust
level they shouldn't have.

The manifest format already anticipates this split: `PluginManifest` in
`crates/nexus-plugins/src/manifest.rs` declares `pub wasm:
Option<WasmConfig>` — the WASM configuration is optional because native
core plugins don't have one.

## Decision

**Two plugin runtimes, one manifest format, one `PluginLoader`.**

### Native path (core plugins)
- Defined by the `CorePlugin` trait in
  `crates/nexus-plugins/src/loader.rs` (line 88).
- Registered at boot via `PluginLoader::register_core(...)`
  (`loader.rs` line 649).
- The bootstrap crate wires in the workspace's first-party plugins from
  `crates/nexus-bootstrap/src/lib.rs::register_core_plugins`.
- Each workspace crate that provides a core plugin ships a
  `core_plugin.rs` module. There are currently 13 of them:
  `nexus-agent`, `nexus-ai`, `nexus-database`, `nexus-editor`,
  `nexus-git`, `nexus-linkpreview`, `nexus-mcp`, `nexus-security`,
  `nexus-skills`, `nexus-storage`, `nexus-terminal`, `nexus-theme`,
  `nexus-workflow`.
- Dispatch is a direct Rust function call (sync or `CorePluginFuture` for
  async handlers) — zero serialization, zero sandbox overhead, full access
  to the kernel-side APIs.

### WASM path (community plugins, eventual)
- Defined by `PluginLoader::load(plugin_dir)` in `loader.rs` (line 542).
- Rejects any manifest whose trust level is `"core"` — core plugins
  **must** use `register_core`. The reverse is also enforced:
  `register_core` rejects manifests whose trust level isn't `"core"`
  (`loader.rs` line 666).
- Drives the wasmtime-based `WasmSandbox` in
  `crates/nexus-plugins/src/sandbox.rs`, capability-gated through the
  same `Capability` enum used on the native side.

### Manifest shape
`PluginManifest.wasm: Option<WasmConfig>` (`manifest.rs` line 70, config
struct line 117) makes the WASM fields optional. Native core manifests
simply omit the `[wasm]` table; community WASM manifests populate it.

The JS/TS community path described in ADR 0015 reuses the same manifest
parser and capability vocabulary — the only difference is that the iframe
bridge is the execution surface instead of the wasmtime engine.

## Alternatives considered

### A. Single WASM-only runtime
Rejected. Core plugins need:
- `rusqlite` handles (nexus-storage, nexus-database) — not cleanly
  exposable through WASI-P2 without a bespoke host-function layer per
  call site.
- Native file watching via `notify` — no portable WASI equivalent today.
- A shared tokio runtime — WASM guests can't share the host runtime
  directly; every async boundary becomes a bridge.
- Workspace-wide compile cost inflation if every `nexus-*` crate became a
  `cdylib`/component target.

The productivity and performance loss across 13 core crates to win an
isolation property we don't need (we trust our own code) isn't worth it.

### B. Single native-only runtime
Rejected. No sandbox story for third-party code. A dlopen-style native
plugin API is the opposite of what a knowledge-app plugin marketplace
needs (see ADR 0011 on the plugin-first shell direction).

### C. Process-per-plugin (even for core)
Rejected. OS-level IPC tax on every kernel event. The event bus publishes
thousands of events per session; serializing each one across a pipe for
every core subscriber is unacceptable overhead for zero security gain
(core plugins are already trusted).

### D. Two separate loaders (one per runtime), two manifest formats
Rejected. The capability vocabulary, plugin-id scheme, manifest
discovery, settings file, grants file, and hot-reload logic are identical
between runtimes. Splitting the types duplicates every bit of that
without adding clarity.

## Consequences

### Positive
- Core plugins pay **no** sandbox tax. Direct function-call dispatch,
  shared tokio runtime, direct event-bus publish.
- Community plugins **cannot** compromise the host. They only see a
  capability-gated RPC bridge (iframe, per ADR 0015) or the
  `WasmSandbox`.
- **One manifest format** describes both. Hot-reload, settings cache,
  grants file, and capability vocabulary are shared.
- `register_core` vs. `load` is a **compile-time** decision on the host
  side (`register_core` takes a `Box<dyn CorePlugin>`; `load` takes a
  `&Path`) — impossible to accidentally register a community plugin as
  core or vice versa.

### Negative
- Two code paths to maintain inside `PluginLoader`. Changes to the
  `CorePlugin` trait are breaking for every core crate in the workspace
  — 13 call sites to update in lockstep.
- The WASM path is partially-implemented scaffolding. `WasmSandbox` works
  and has integration tests, but there are no production community WASM
  plugins yet — today's community plugins all take the iframe path from
  ADR 0015.

### Neutral
- The iframe runtime from ADR 0015 is the **current** community surface
  for JS/TS plugins. The WASM `load()` path coexists for future
  polyglot / compute-heavy community plugins but is not the default
  authoring target.

## References

- `crates/nexus-plugins/src/loader.rs` — `CorePlugin` trait (line 88),
  `PluginLoader::load` (line 542), `PluginLoader::register_core`
  (line 649).
- `crates/nexus-plugins/src/manifest.rs` — `PluginManifest` (line 47),
  `pub wasm: Option<WasmConfig>` (line 70), `WasmConfig` (line 117).
- `crates/nexus-bootstrap/src/lib.rs::register_core_plugins` — the
  workspace's core-plugin registration site.
- `crates/nexus-plugin-api/src/capability.rs` — shared capability
  vocabulary for both runtimes.
- `crates/nexus-*/src/core_plugin.rs` — 13 native implementations:
  `nexus-agent`, `nexus-ai`, `nexus-database`, `nexus-editor`,
  `nexus-git`, `nexus-linkpreview`, `nexus-mcp`, `nexus-security`,
  `nexus-skills`, `nexus-storage`, `nexus-terminal`, `nexus-theme`,
  `nexus-workflow`.
- ADR 0004 — crate ownership boundaries.
- ADR 0011 — plugin-first shell direction.
- ADR 0015 — iframe sandbox as the current community-plugin runtime.
