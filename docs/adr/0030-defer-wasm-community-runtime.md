# ADR 0030: Defer WASM Community-Plugin Runtime; Iframe is the Community Surface

**Date:** 2026-05-15
**Status:** Accepted
**Supersedes (partially):** [ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) — the WASM half only. The native-plugin half of ADR 0016 (the `CorePlugin` trait + `register_core` path) is unchanged.

## Context

[ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) (2026-04-24) declared two community-plugin runtimes: an iframe sandbox (per [ADR 0015](0015-iframe-sandbox-plugin-runtime.md)) and a wasmtime-backed `WasmSandbox`. The iframe path shipped and is the active community-plugin authoring surface today. The WASM path shipped as scaffolding — `WasmSandbox` works, has integration tests, and is referenced by `PluginLoader::load`, but no production community plugin uses it.

The [BL-137](../PRDs/backlog/BL-137.md) architectural review (2026-05-14) called out the divergence as a maintenance cost:

> "**Decide WASM commitment.** ADR 0016 is 'native vs WASM' but no production WASM plugin exists today; the iframe runtime (ADR 0015) is the actual community-plugin surface. Either commit (ship one real WASM community plugin) or document iframe-only and move WASM to 'deferred experiment.' Large if committing, trivial if deferring. Low priority — costs are small today, will rise."

A year after the iframe ADR landed, the iframe runtime is what every shipped community plugin uses, the `@nexus/extension-api` package is built around the iframe bridge, and every cross-cutting concern (capability prompts, hot-reload, signature verification, the BL-138 cap matrix) has been authored against the iframe surface first. The WASM-side test fixtures exist but haven't been exercised against the BL-113 / ADR 0027 protocol-host contribution model, the BL-138 default-deny capability registration, or the BL-134 ai-runtime event surface.

Keeping both paths "supported but not authored against" is the worst of both worlds: shell-host code carries WASM-aware branches, the capability matrix has to think about WASM trust levels, and any community-plugin docs we publish have to disclaim that one runtime is real and the other is theoretical.

## Decision

**The iframe runtime (ADR 0015) is the canonical community-plugin runtime. The WASM path moves to "deferred experiment" status.**

Practical consequences:

1. **No new WASM-facing capability work.** Cross-cutting concerns (capability prompts, signature verification, the BL-138 cap matrix, the BL-134 ai-runtime event surface, future BL-137 follow-ups) are designed against the iframe path. WASM-side parity, if needed later, is a Phase-2 step inside whatever feature is landing — not a checklist item every feature has to satisfy.

2. **No new WASM tests.** The existing `WasmSandbox` unit + integration tests stay in place — `wasmtime` is still a transitive dep, the code still compiles, and the test surface is the regression guard. New community-plugin behaviour does not need WASM-side coverage.

3. **No production WASM plugins commissioned.** Until someone presents a concrete use case that the iframe runtime cannot serve (heavy CPU work that benefits from native compilation, polyglot guest code, a strict-memory-isolation requirement the iframe sandbox can't provide), WASM stays in scaffolding state.

4. **`nexus plugin scaffold --type wasm` keeps working** because it threads through the existing `WasmSandbox`. We're not deleting code; we're declining to extend it. The CLI subcommand also keeps the existing experimental-status warning it already prints.

5. **`PluginLoader::load`'s WASM dispatch stays in tree.** Removing it would be churn for no behavioural benefit. The `pub wasm: Option<WasmConfig>` field on `PluginManifest` likewise stays as a typed null.

6. **Docs realignment.** [`docs/developer/README.md`](../developer/README.md) and [`docs/shell/writing-a-plugin.md`](../shell/writing-a-plugin.md) point at the iframe path as the singular community-plugin authoring story; WASM gets a "deferred — see ADR 0030" sidebar instead of a parallel walkthrough.

## Alternatives considered

### A. Commit to WASM — ship one real WASM community plugin

The forcing function would have been a concrete capability the iframe runtime cannot deliver (multi-MB native dependencies, hot loops the JS engine can't run inside the iframe). Today there is no such ask: the community-plugin candidates we've seen (note-templating, alternative pickers, light analyzers) all fit comfortably in the iframe model. Investing the multi-week effort to harden `WasmSandbox` against BL-138's cap matrix + the protocol-host model + signature verification with no production plugin demanding it is speculative work.

### B. Remove the WASM path entirely

Trivially possible — `PluginLoader::load` only routes to `WasmSandbox` when a manifest carries `[wasm]`, and we can simply make `WasmConfig` parsing always reject. But removing live code that compiles and has tests trades a small recurring maintenance cost for a one-time churn cost. Removing it would also close the door on a future "we need a real CPU-bound community plugin" decision; keeping it as scaffolding leaves the door cracked.

### C. Status quo — keep ADR 0016 as written, treat both runtimes as first-class

Rejected per the BL-137 ask. The cost is real (capability matrix has to reason about two runtimes, docs have to disclaim, every architectural decision asks "what about WASM?"); the benefit is hypothetical.

## Consequences

### Positive

- One canonical authoring story for community plugins. The "where do I start?" answer is unambiguous: iframe + `@nexus/extension-api`.
- Docs collapse to one walkthrough; the "WASM is experimental scaffolding" note is small.
- Cross-cutting feature work (BL-138 cap matrix, BL-134 ai-runtime events, BL-113 protocol host) doesn't need WASM-side parity tasks.

### Negative

- `WasmSandbox` accumulates technical debt slowly — it works against today's manifest shape, but each new capability or signed-manifest extension that lands on the iframe side is one more thing the WASM side doesn't yet honour. If we eventually want WASM, the catch-up cost grows.
- Anyone who builds a `WasmSandbox`-targeted plugin in the meantime hits whatever drift has accumulated. The CLI scaffold's experimental-status warning is the only signal that this is unsupported.

### Neutral

- The `pub wasm: Option<WasmConfig>` field on `PluginManifest` is a typed null. ADR 0016's "one manifest format" promise still holds — just one of the two runtimes is dormant.

## When to revisit

Revisit if any of the following is true:

- A concrete community-plugin candidate appears that the iframe runtime genuinely cannot host (heavy native code, polyglot guest, etc.) and the author is willing to land the integration.
- Iframe sandbox security comes under scrutiny and we need wasmtime's stricter isolation model.
- Multi-month effort on a plugin marketplace demands cross-language portability the iframe model doesn't deliver.

Until then, treat the iframe runtime as the community-plugin surface.

## References

- [ADR 0015](0015-iframe-sandbox-plugin-runtime.md) — iframe sandbox; the canonical community surface.
- [ADR 0016](0016-microkernel-native-vs-wasm-plugin-split.md) — native vs WASM split; the native half stands, the WASM half is deferred by this ADR.
- [BL-137](../PRDs/backlog/BL-137.md) — architectural review surfacing the call.
- `crates/nexus-plugins/src/sandbox.rs` — `WasmSandbox` (scaffolding-only going forward).
- `crates/nexus-plugins/src/loader.rs::PluginLoader::load` — dispatch site to `WasmSandbox`.
- `docs/shell/writing-a-plugin.md` — the canonical authoring walkthrough (iframe).
