# ADR 0015: Iframe Sandbox as the Community-Plugin Runtime

**Date:** 2026-04-24
**Status:** Accepted

## Context

Nexus has two distinct populations of plugin code:

1. **Core plugins** — first-party Rust code shipped with the binary. Fully
   trusted. See ADR 0016 for the native-plugin path.
2. **Community plugins** — third-party JavaScript/TypeScript installed from
   `~/.nexus-shell/plugins/`. Not trusted by default.

The community-plugin set cannot share JS globals, the shell's DOM, or the
renderer's direct `invoke()` handle with the host. Before this decision the
shell loaded JS plugins via a plain `<script>` module in the host renderer,
which gave every community plugin ambient access to the Tauri `invoke`
bridge, the full DOM, and shell React state. That escape path is enumerated
in `docs/archive/planning/UI-AUDIT.md` and is the precondition for every capability
bypass in the plugin model.

We need a browser-native isolation boundary that:
- Runs arbitrary author-written JS without letting it touch shell globals.
- Works with the TypeScript build pipeline authors already use — no WASM
  toolchain requirement for UI-level plugins.
- Is mediated by the existing capability strings in
  `nexus-plugin-api::Capability` (14 variants, see
  `crates/nexus-plugin-api/src/capability.rs`) so the host side of every
  surface can reject calls a plugin didn't declare.

## Decision

**Community plugins execute inside a null-origin iframe
(`sandbox="allow-scripts"` — no `allow-same-origin`), with a
`postMessage`-based RPC bridge to the host. The `@nexus/extension-api`
surface is proxied across that bridge, and capability declarations in
`plugin.json` gate which bridge methods are exposed.**

Implementation:

- `shell/src/host/sandbox/SandboxOrchestrator.ts` spawns the iframe, runs
  the handshake, keeps a ping/pong watchdog, and owns the `PanelNode`
  refresh channel that powers `SandboxPanelView.tsx`.
- `shell/src/host/sandbox/IframePort.ts` is the host-side transport.
  `router.ts` dispatches incoming RPC envelopes.
- `shell/src/host/sandbox/methodCatalog.ts` enumerates the methods the
  bridge will serve; `capabilityGuard.ts` rejects calls whose required
  capability isn't in the plugin's granted set.
- `shell/src/host/communityPluginLoader.ts` is the discovery layer: it
  walks `~/.nexus-shell/plugins/`, validates manifests, and hands matched
  plugins to the orchestrator rather than `import()`ing them into the host
  realm.
- `shell/src/host/ExtensionHost.ts` is the stable API surface plugins see
  on the guest side of the bridge.
- `shell/src/plugins/community/hello-world/` is the fixture plugin proving
  the pathway end-to-end (see `plugin.json` — `"sandboxed": true`,
  capabilities declared explicitly).

The install-time consent flow (ADR'd separately in the capability-grants
file format) writes `granted_caps.json` next to the plugin; the sandbox
bridge only exposes a method if the plugin's declared capability is in that
grant set.

## Alternatives considered

### A. Web Workers
Rejected: no DOM access. Most plugin views need to render UI (panels, menu
items, ribbons). A worker-only runtime forces every plugin UI through an
awkward "worker computes, host renders" split.

### B. Shadow DOM only
Rejected: Shadow DOM encapsulates markup and styles but shares the JS
global object with the host. A malicious plugin can still read shell
closures, monkey-patch `fetch`, or reach `window.__TAURI__`.

### C. Separate Tauri window per plugin
Rejected: OS-level window management cost per plugin, extra IPC serializer
hops, and the shell loses in-place panel embedding. Not viable for the
ribbon/sidebar contribution surfaces.

### D. Compile TS plugins to WASM
Rejected for the UI tier: TS→WASM toolchains (AssemblyScript, Javy)
inflate the inner dev loop for marginal isolation benefit over a
null-origin iframe, and WASM in the renderer still can't touch the DOM
without a bridge — so we'd build the same RPC layer anyway, just with an
extra compile step on top.

## Consequences

### Positive
- Clear isolation boundary. A sandboxed plugin cannot read shell cookies,
  `localStorage`, `indexedDB`, or reach `window.top`.
- Familiar browser primitive. No new Rust/WASM tooling for plugin authors.
- Capability gating has a single enforcement point
  (`capabilityGuard.ts`) rather than being scattered across every API
  surface.
- One API surface (`@nexus/extension-api`) serves both sides of the
  bridge, so plugins get TypeScript types against the same contract the
  host implements.

### Negative
- Per-plugin iframe startup cost. Visible on cold-load of plugins with
  heavy bundles.
- RPC roundtrip latency on every capability-gated call vs. an in-process
  function call.
- Some Web APIs (cross-origin storage, `SharedArrayBuffer` gated on COOP/
  COEP) are unavailable inside a null-origin frame. Accepted tradeoff.

### Neutral
- Compute-time / memory quotas are **not** covered by the iframe boundary
  — a runaway plugin can still burn CPU in its own frame. That concern is
  deferred to the Phase 6 runtime-quota work and is out of scope for this
  ADR.

## References

- Phase 3 work items **WI-30a…f** (sandbox RPC protocol, orchestrator,
  iframe lifecycle, hello-world migration, end-to-end tests) — see
  commits `699b497`, `e331a54`, `4e8e3d8`, `c6b8513`, `f3b8b2f`,
  `16fe35e`, `417dff1`.
- Phase 3 **WI-31** — install-time capability consent prompt
  (commit `6f90aec`).
- `shell/src/host/sandbox/` — orchestrator, port, router, guard, panel
  view.
- `shell/src/host/communityPluginLoader.ts`, `ExtensionHost.ts` — host
  integration points.
- `shell/src/plugins/community/hello-world/plugin.json` — fixture.
- `crates/nexus-plugin-api/src/capability.rs` — the 14-variant capability
  enum the sandbox speaks.
- `docs/PRDs/BACKLOG_COMPLETED.md` entries **UI F-5.1.1** (drop
  `allow-same-origin` from plugin panel iframes) and **UI F-8.1.1**
  (iframe sandbox as precondition for community script plugins).
