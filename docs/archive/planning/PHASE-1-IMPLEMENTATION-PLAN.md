# Phase 1 Implementation Plan — Bridge & Leaf Foundation

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). Paths below reference the legacy `app/` and `crates/nexus-app/` tree that has since been deleted. For current locations see `docs/legacy-shell-retirement.md`.

**Status:** Plan only (no code changes yet)
**Date:** 2026-04-23
**Author:** Claude (audit + planning run)
**Phase:** 1 of 6 in the shell-migration roadmap (per [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5 and [ADR 0011](../adr/0011-adopt-plugin-first-shell.md))
**Sibling plan:** [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md) — Phase 2 work items (some P0s from that list live here in Phase 1: WI-06, WI-20, WI-22, WI-23).

---

## 1. Why Phase 1 exists

Phase 1 takes the shell from "working prototype with stubs" to "structurally ready for parity migration." Everything downstream depends on this landing:

- Phase 2 (feature parity) assumes the extension-api contract is stable and plugins can be wrapped cleanly around it.
- Phase 3 (security hardening) assumes there's one enforcement surface (ExtensionHost) rather than two.
- The freeze policy accepted in Phase 0 (ADR 0011, CONTRIBUTING.md) works only if there's a structural guardrail preventing regressions — human review alone won't scale.

Good news surfaced in the readiness audit: **the Leaf/ViewRegistry foundation is ~90% complete**. Phases 0–4 of the leaf-migration-plan are done, tests pass, and `WorkspaceRenderer.tsx` is in place. The remaining work is finishing, not building from scratch. Bridge Phases 0–3 have shipped; only Phase 4 polish remains.

## 2. Scope summary

Four work streams, each with a stable WI-ID for tracking. Three already appear in `PARITY-CHECKLIST.md` (WI-06, WI-20, WI-22, WI-23); two are new to this plan (WI-24, WI-25).

| ID | Stream | Size | Priority | Depends on |
|---|---|---|---|---|
| **WI-22** | Rust-side guardrail: cap `#[tauri::command]` count in `crates/nexus-app` | XS (~1d) | P0 | — |
| **WI-23** | Shell-side guardrail: reject raw Tauri imports in `shell/src/plugins/` | S (~3d) | P1 | WI-20 |
| **WI-20** | Regenerate `@nexus/extension-api` via ts-rs + wire as single import path | M (~1wk) | P0 | — |
| **WI-06** | Bridge Phase 4 polish: per-plugin subscription tracking + structured errors | S (~1wk) | P0 | — |
| **WI-24 (new)** | Finish Leaf view-wrapping for the 6 plugins still on legacy `slotRegistry` | M (~1wk) | P1 | WI-20 (for typed contract) |
| **WI-25 (new)** | Eliminate direct `@tauri-apps/*` imports in shell plugins | S (~3d) | P1 | WI-23 (enforces it) |

**Total:** ~4–5 engineer-weeks. Two engineers can parallelize to ~2½ calendar weeks (see §7 dependency graph).

**Phase 1 acceptance** — all six items landed and:

1. `cargo test -p nexus-bootstrap` passes including the WI-22 cap.
2. `pnpm --filter nexus-shell test` passes including the WI-23 import-hygiene test.
3. `@nexus/extension-api` is the only import path for all first-party shell plugins. `grep -r "@tauri-apps/" shell/src/plugins/` returns zero matches (or a documented allowlist).
4. A plugin deactivation test proves kernel subscriptions created via `api.kernel.on(...)` are unsubscribed, with no event forwarding afterward.
5. A smoke test boots the shell empty, loads a single hello-world plugin, mounts it as a View via Leaf, invokes `com.nexus.storage::list_dir`, receives a kernel bus event, and shuts down cleanly.

---

## 3. WI-22 — Rust-side freeze guardrail

### 3.1 Intent

Prevent any new `#[tauri::command]` handler from being added to `crates/nexus-app/src/` without a conscious override. This is the structural enforcement of the freeze we declared in Phase 0. Human review alone doesn't scale (especially if agents are doing the work).

### 3.2 Current state

- `crates/nexus-bootstrap/tests/dep_invariants.rs` already exists (88 lines) and enforces dependency invariants. Good precedent and good home for this test.
- Per earlier count, legacy `crates/nexus-app/src/` declares exactly **95** `#[tauri::command]` handlers, registered in `lib.rs`'s `generate_handler!` block. That's the baseline.

### 3.3 Design

Add a sibling test to `dep_invariants.rs` (or a new file `crates/nexus-bootstrap/tests/legacy_freeze.rs`) that:

1. Walks `crates/nexus-app/src/**/*.rs`.
2. Counts occurrences of `#[tauri::command]` as a literal substring (fast, no AST needed).
3. Asserts the count is `<= 95`.
4. Error message references `CONTRIBUTING.md` and `docs/adr/0011-adopt-plugin-first-shell.md`.

**Fast path** — use `std::fs::read_to_string` with `str::matches("#[tauri::command]").count()`. No syntax parsing. A handler split across multiple lines with comments or macros in between doesn't matter; the attribute itself is a single token pattern.

**Override escape hatch** — if someone genuinely needs to remove a handler (post-parity work, bug fix), they edit the baseline constant in the test and note the reason in the commit message. The baseline only goes down, never up.

### 3.4 Subagent pattern

**None — do it in the main thread.** This is a single 40-line test file plus a constant; an agent round-trip costs more than the implementation.

### 3.5 Commit plan

One commit:

```
chore(bootstrap): add legacy-shell freeze guardrail test

Caps the count of #[tauri::command] handlers in crates/nexus-app/src at
95 (current baseline). New tauri commands are the wrong answer per
CONTRIBUTING.md and ADR 0011 — new capability work belongs as a
service-crate IPC handler + a plugin in shell/src/plugins/nexus/.

If you're legitimately removing a handler, drop the baseline and note
the reason in your commit message.
```

**Files touched:**
- `crates/nexus-bootstrap/tests/legacy_freeze.rs` — new, ~60 lines with doc comment.

### 3.6 Acceptance

- `cargo test -p nexus-bootstrap --test legacy_freeze` passes on `main`.
- Adding a new `#[tauri::command]` to `crates/nexus-app/src/forge.rs` fails the test with a clear message.

---

## 4. WI-23 — Shell-side import-hygiene guardrail

### 4.1 Intent

Prevent shell plugins from reaching around `@nexus/extension-api` to raw Tauri, filesystem, or shell-internal paths. Same motivation as WI-22: structural enforcement so agents can't regress us.

### 4.2 Current state

- No test exists today. `shell/package.json` has `typecheck` and `lint` scripts but no test runner configured.
- 9+ files in `shell/src/plugins/{core,nexus}/` currently import `@tauri-apps/*` directly (WI-25 fixes those; this test is what prevents new ones).

### 4.3 Design

Write `shell/tests/plugin-import-hygiene.test.ts` using Node's built-in test runner (`node --test`) so we don't add a vitest/jest dependency just for this.

The test:

1. Globs `shell/src/plugins/{core,nexus}/**/*.{ts,tsx}`.
2. For each file, reads source and checks for forbidden imports using regex:
   - `/from ['"]@tauri-apps\//` — raw Tauri
   - `/from ['"]\.\.\/\.\.\/host\//` — shell internals (`host/`)
   - `/from ['"]\.\.\/\.\.\/registry\//` — shell internals (`registry/`)
3. Allowlist: a single constant at the top of the test listing files permitted to import from `host/` (the `workspace` plugin legitimately needs it for kernel lifecycle).
4. Failure message points at `CONTRIBUTING.md` + `packages/nexus-extension-api/README.md`.

### 4.4 Wiring

Add to `shell/package.json`:

```json
"scripts": {
  "test": "node --test tests/"
}
```

Add to CI (if present) — script verification is single-file so the CI YAML change is trivial.

### 4.5 Subagent pattern

**One Explore subagent** to enumerate the existing bad imports so the allowlist is informed. Prompt: *"grep shell/src/plugins/{core,nexus}/**/*.{ts,tsx} for `from '@tauri-apps/` and for `from '../../host/` and for `from '../../registry/`; return list of (file, line, import) triples."* ~5 min agent run, gives us ground truth for the allowlist.

Main-thread work: write the test, verify it passes against the current (pre-WI-25) tree modulo allowlist, verify it fails when a new raw Tauri import is added.

### 4.6 Commit plan

One commit:

```
test(shell): add plugin-import hygiene guardrail

Fails if any file in shell/src/plugins/{core,nexus}/ imports from
@tauri-apps/* or shell internals (host/, registry/) outside the
@nexus/extension-api contract. Known violators are in the allowlist;
WI-25 will drain that list.

Prevents agent or human regressions of the bridge invariant from
shell-kernel-bridge-plan.md.
```

**Files touched:**
- `shell/tests/plugin-import-hygiene.test.ts` — new, ~100 lines.
- `shell/package.json` — add `test` script.
- (optional) `.github/workflows/*.yml` or equivalent CI config.

### 4.7 Acceptance

- `pnpm --filter nexus-shell test` (or `cd shell && npm test`) passes.
- Adding a new `import { invoke } from '@tauri-apps/api/core'` to a plugin not on the allowlist fails the test.

---

## 5. WI-20 — Wire `@nexus/extension-api` via ts-rs

### 5.1 Intent

Make `@nexus/extension-api` the single import path for shell plugins. Today the package is hand-authored (261 lines) and zero plugins import from it. The risk is drift: every kernel refactor breaks plugins silently until runtime. Generate the Rust-aligned types via ts-rs so they can't drift.

### 5.2 Current state

- `crates/nexus-plugin-api/src/` contains the Rust contract: `Capability`, `NexusEvent`, `EventFilter`, `PluginInfo`, `PluginStatus`, `TrustLevel`, `IpcError`, `BusError`, `CapabilityError`, `LogLevel`.
- `packages/nexus-extension-api/src/index.ts` — 261 lines, hand-authored, includes contribution DTOs (`EditorBlockType`, `EditorDecorationProvider`, `EditorKeybinding`, `Snippet`, `MenuItem`, `TreeDataProvider`, `ScriptPlugin` etc.) plus the `NexusPluginContext` shape.
- `shell/src/types/plugin.ts` — 40+ lines, shell-internal duplication of manifest + contribution types.
- `shell/src/host/PluginAPI.ts` — implementation of the API object, imports plugin types from `shell/src/types/plugin.ts` rather than the package.
- ts-rs is already a workspace dependency (per `Cargo.toml` line 142: `ts-rs = { version = "11", features = ["serde-compat"] }`). Used in `nexus-theme`. So the tooling is in place.

### 5.3 Design

Two-layer strategy:

**Layer A — Rust-generated primitives (the fixed-shape types).** Add `#[derive(TS)]` + `#[ts(export)]` to the following Rust types so they emit to `packages/nexus-extension-api/src/generated/`:

- `Capability`, `CapabilitySet`, `CapabilityError` — `capability.rs`
- `NexusEvent`, `EventFilter`, `EventMetadata`, `PublishedEvent`, `StopReason` — `event.rs`
- `IpcError`, `BusError` — `error.rs`
- `TrustLevel`, `PluginInfo`, `PluginStatus` — `plugin.rs`
- `LogLevel` — `log.rs`
- `PLUGIN_API_VERSION` — const export

**Layer B — TypeScript-authored contribution surface.** The shell-side contribution types (`EditorBlockType`, `MenuItem`, `TreeDataProvider`, etc.) live in React/CM6-land and have no Rust equivalent. Keep them hand-authored in `src/index.ts` but re-export from the same package so plugins use a single import statement.

**Build wiring:**

1. `packages/nexus-extension-api/package.json` gains a `"generate"` script: `cd ../../crates/nexus-plugin-api && cargo test --features ts-export -- --test-threads=1` (ts-rs emits `.ts` files during a test run with the `export` attribute).
2. The generated files land in `packages/nexus-extension-api/src/generated/` per ts-rs's `export_to` attribute.
3. `src/index.ts` becomes a **barrel**: re-exports hand-authored contribution types + `export * from './generated'` for the Rust-derived ones.
4. Generated files are committed (not .gitignored) so consumers don't need Rust toolchain to use the package.

**Drift protection** — a CI job runs `pnpm generate` and fails if the git diff is non-empty. Keeps Rust and TS in lockstep.

**Shell adoption — minimal first step:** update `shell/src/types/plugin.ts` to re-export from `@nexus/extension-api` rather than declare its own `PluginManifest`, `PluginContributions`, etc. Any missing types get moved into the extension-api package. This keeps existing plugin imports working (they import from `../types/plugin`) while making the package the source of truth.

**Full adoption** follows as each plugin is touched in WI-24/WI-25.

### 5.4 Subagent pattern

This is the best subagent opportunity of the four streams.

**Agent 1 — Rust ts-rs derivation (parallel task).**
Prompt spec: *"Add `#[derive(ts_rs::TS)]` and `#[ts(export, export_to = '../../packages/nexus-extension-api/src/generated/')]` to these specific Rust types in `crates/nexus-plugin-api/src/`: [full list with file:line refs]. Add a feature `ts-export` to `Cargo.toml` that gates the derives (so default builds don't pull ts-rs unnecessarily). Run `cargo test -p nexus-plugin-api --features ts-export` and verify the .ts files land. Report exact file paths written and any types that refused to derive (e.g. types with non-primitive associated types)."*

Deliverable: a diff-able patch against `crates/nexus-plugin-api/`, the generated `.ts` files, and a report of any gotchas (ts-rs doesn't handle async functions, has quirks with `Arc<T>`, etc.).

**Agent 2 — TypeScript consolidation (sequential after Agent 1).**
Prompt spec: *"Consolidate `shell/src/types/plugin.ts` into `packages/nexus-extension-api/src/index.ts`. Move `PluginManifest`, `PluginContributions`, `CommandContribution`, `ViewContribution`, etc. into the package; re-export them from `shell/src/types/plugin.ts` as a thin compat shim so existing imports keep working. Verify `pnpm --filter nexus-shell typecheck` passes. Do NOT rewrite plugin imports — that's WI-25."*

Main-thread work: wire the CI drift check, merge the two patches, update `packages/nexus-extension-api/README.md` with a stability statement and usage snippet.

### 5.5 Commit plan

Three commits:

1. `feat(plugin-api): derive ts-rs bindings for Rust contract types`
   - Adds ts-rs derives and feature flag in `crates/nexus-plugin-api/`.
   - Commits generated `.ts` files under `packages/nexus-extension-api/src/generated/`.
2. `feat(extension-api): consolidate TS plugin contract into single package`
   - Moves manifest/contribution types from `shell/src/types/plugin.ts` to `packages/nexus-extension-api/src/index.ts`.
   - Adds barrel export pattern.
   - Leaves `shell/src/types/plugin.ts` as a compat re-export.
3. `ci: enforce @nexus/extension-api drift check`
   - Adds CI step: `pnpm generate` must produce no git diff.
   - Updates `packages/nexus-extension-api/README.md` with stability + regen instructions.

**Files touched:**
- `crates/nexus-plugin-api/Cargo.toml` — `ts-rs` dep behind `ts-export` feature.
- `crates/nexus-plugin-api/src/{capability,event,error,plugin,log}.rs` — derives.
- `packages/nexus-extension-api/src/generated/*.ts` — new, auto-generated.
- `packages/nexus-extension-api/src/index.ts` — barrel + moved types.
- `packages/nexus-extension-api/package.json` — `generate` script.
- `packages/nexus-extension-api/README.md` — stability statement.
- `shell/src/types/plugin.ts` — slim down to re-exports.
- `.github/workflows/*.yml` or equivalent — drift check.

### 5.6 Acceptance

- `cargo test -p nexus-plugin-api --features ts-export` generates `.ts` files matching HEAD.
- `pnpm --filter nexus-extension-api build` produces `dist/` with no errors.
- `shell/src/types/plugin.ts` imports from `@nexus/extension-api` and re-exports for compat.
- CI drift check fails if Rust type changes without regenerated TS.
- `pnpm --filter nexus-shell typecheck` still green.

---

## 6. WI-06 — Bridge Phase 4 polish (subscription cleanup + structured errors)

### 6.1 Intent

Close two gaps that block Phase 2 streaming plugins (WI-01 AI chat, WI-07 agent, WI-12 terminal streaming):

1. **Per-plugin kernel subscription tracking.** Today `api.kernel.on(topic, handler)` returns an unsubscribe function but the `PluginRegistry` doesn't track it. When a plugin deactivates, its subscriptions live on — the Rust-side forwarder tasks keep running, events keep arriving at a dead listener. Benign today (events are silently dropped by the GC'd listener), bad at scale (leak of Rust tasks, memory, and event-bus bandwidth).

2. **Structured error surface from kernel to shell.** `bridge.rs::kernel_invoke` returns `Result<Value, String>`; the String loses the `IpcError` variant. Plugins can't distinguish `Timeout` from `PluginCrashed` from `CapabilityDenied`. Makes error UX poor and debugging hard.

### 6.2 Current state

- `shell/src-tauri/src/bridge.rs` — 7 bridge commands, in-process kernel runtime held in Tauri state. ~180 LOC.
- `shell/src/host/PluginAPI.ts` — builds the per-plugin API. The `kernel.on()` method creates a `listen<KernelEventEnvelope>('kernel:event', ...)` handler and calls `kernel_subscribe`. Returns an unsubscribe function.
- `shell/src/host/PluginRegistry.ts` — tracks commands, slots, status bar items, config, keybindings per plugin. Does NOT track kernel subscriptions. `unregisterAll(id)` sweeps the former but not the latter.
- `shell/src/host/ExtensionHost.ts` — calls `plugin.deactivate?.()` then `registry.unregisterAll(id)`.

### 6.3 Design

**Part A — subscription tracking.**

Add to `PluginRegistry`:

```typescript
private subscriptions = new Map<string, Set<Disposable>>()  // pluginId → set of unsubscribe fns

trackSubscription(pluginId: string, unsubscribe: Disposable): void {
  let set = this.subscriptions.get(pluginId)
  if (!set) { set = new Set(); this.subscriptions.set(pluginId, set) }
  set.add(unsubscribe)
}

unregisterAll(pluginId: string): void {
  // ... existing sweeps ...
  const subs = this.subscriptions.get(pluginId)
  if (subs) {
    for (const unsub of subs) {
      try { unsub() } catch (e) { logger.warn(`subscription unsub failed for ${pluginId}`, e) }
    }
    this.subscriptions.delete(pluginId)
  }
}
```

In `buildPluginAPI` (`PluginAPI.ts`), wrap `kernel.on`:

```typescript
const origOn = kernelApi.on
kernelApi.on = (topic, handler) => {
  const unsub = origOn(topic, handler)
  registry.trackSubscription(pluginId, unsub)
  return unsub
}
```

**Part B — structured errors.**

Define a JSON envelope:

```typescript
interface IpcErrorEnvelope {
  kind: 'timeout' | 'plugin_crashed' | 'capability_denied' | 'dispatch_failed' | 'serialization' | 'unknown'
  plugin_id: string
  command: string
  message: string
  retryable: boolean
}
```

`bridge.rs::kernel_invoke` changes its return from `Result<Value, String>` to `Result<Value, IpcErrorEnvelope>` (serialized as a JSON object). Map `IpcError` variants:

| `IpcError` variant | envelope.kind | retryable |
|---|---|---|
| `Timeout` | `timeout` | `true` |
| `PluginNotFound` / `CommandNotFound` | `dispatch_failed` | `false` |
| `CapabilityDenied` | `capability_denied` | `false` |
| `PluginCrashed` / `Panic` | `plugin_crashed` | `false` |
| `Serialization` | `serialization` | `false` |
| fallback | `unknown` | `false` |

On the TS side, `api.kernel.invoke` throws an `IpcError` object with the envelope fields, so plugins can:

```typescript
try {
  await api.kernel.invoke('com.nexus.ai', 'ask', { q })
} catch (e) {
  if (e.kind === 'timeout' && e.retryable) { /* retry */ }
  else if (e.kind === 'capability_denied') { /* show settings link */ }
}
```

### 6.4 Subagent pattern

**Agent 1 — Rust bridge update.**
Prompt spec: *"In `shell/src-tauri/src/bridge.rs::kernel_invoke`, change the error return from `String` to a serializable `IpcErrorEnvelope` struct matching: `{ kind, plugin_id, command, message, retryable }`. Map the kernel's `IpcError` variants per this table: [table]. Ensure `serde::Serialize` + `#[derive(TS)]` so Layer A (WI-20) picks it up. Add a unit test per variant. Don't change `kernel_subscribe` / `kernel_unsubscribe` — separate WI."*

Deliverable: diff + unit tests.

**Agent 2 — Shell TS update (sequential after Agent 1).**
Prompt spec: *"In `shell/src/host/PluginAPI.ts`: (1) wrap `kernel.on` so it registers the returned unsubscribe function via `registry.trackSubscription(pluginId, unsub)`. (2) Catch error rejections from `invoke()` and re-throw a typed `KernelIpcError extends Error` with `kind`, `pluginId`, `command`, `retryable` fields from the envelope. In `PluginRegistry.ts`: add `trackSubscription()` and extend `unregisterAll()` per the design. Add a unit test in `shell/tests/` that activates a plugin, calls `kernel.on`, deactivates the plugin, then emits a kernel event and asserts the handler did NOT fire."*

Deliverable: diff + test.

### 6.5 Commit plan

Two commits:

1. `feat(bridge): structured IpcError envelope on kernel_invoke`
   - Rust-side envelope + variant mapping + unit tests.
   - Shell-side typed `KernelIpcError` + test coverage.
2. `feat(host): track and clean up kernel subscriptions per plugin`
   - `PluginRegistry.trackSubscription()` + sweep in `unregisterAll()`.
   - `PluginAPI.kernel.on` wrap.
   - Plugin-deactivate regression test.

### 6.6 Acceptance

- Deactivating a plugin clears its tracked subscriptions and the Rust forwarder tasks exit (verifiable by log / debug command).
- A test plugin that calls `api.kernel.invoke('bogus', 'bogus', {})` receives an error with `kind === 'dispatch_failed'` and `retryable === false`.
- Timeout simulation returns `kind === 'timeout'` with `retryable === true`.

---

## 7. WI-24 — Finish Leaf view-wrapping (Phase 5 of leaf-migration-plan)

### 7.1 Intent

Six of the twelve view types in `leaf-migration-plan.md` §Phase 5 still wire via the legacy `slotRegistry.register('sidebar' | 'rightPanel' | 'panelArea', ...)` pattern. Until every first-party view is a `ViewBase` subclass with registry entries, the Phase 7 cleanup (deleting those slot ids from `SlotRegistry.ts`) can't happen and plugins still have two mounting paths.

### 7.2 Current state (per Explore audit)

**Wrapped:** workspace, files, editor (partially), terminal, canvas, bases.

**Not yet wrapped (still slotRegistry):** outline, backlinks, search, graph, ai, agent (at least — confirm before starting).

### 7.3 Design

For each unwrapped view, the pattern is identical:

1. Create `shell/src/plugins/nexus/<plugin>/<Plugin>View.ts` that extends `ViewBase` from `@nexus/extension-api`.
2. `onOpen(containerEl)` calls `createRoot(containerEl).render(<ExistingReactComponent />)`. Save the root handle on the instance.
3. `onClose()` calls `root.unmount()`.
4. `getState()` / `setState()` serialize/restore any per-view state (scroll position, active tab filter, etc.).
5. Plugin's `activate()` registers the view type with `viewRegistry.register('nexus.<plugin>', creator)` and contributes a view via `api.views.register({ id, kind: 'nexus.<plugin>', slot: 'rightPanel' | ... })`.
6. Remove the `slotRegistry.register(...)` call.

Six views × ~80 LOC each = ~480 LOC + 6 test updates.

### 7.4 Subagent pattern

**Primary delegation target.** This is mechanical, repetitive, and each view is independent. Ideal for a batch of parallel Explore subagents.

**Agent dispatch per view (6 parallel agents):**
Prompt spec (templated): *"Port `shell/src/plugins/nexus/<view>/index.ts` from the legacy `slotRegistry.register('rightPanel', {...})` pattern to the Leaf/ViewRegistry pattern. Create `<View>View.ts` extending `ViewBase` from `@nexus/extension-api`. Keep the existing React component [file path] unchanged; `onOpen(el)` wraps it with `createRoot(el).render(<Component />)`. `onClose()` unmounts. Register the view type in `activate()` and remove the slotRegistry.register call. Ensure existing tests still pass. Report: file diffs, new LOC, any state that needs `getState`/`setState` plumbing, any blockers."*

Main-thread work: review each agent's output for consistency, write the consolidated commit message, run `typecheck` and the shell test suite.

### 7.5 Commit plan

One commit per view (six commits, each self-contained):

1. `refactor(outline): migrate to ViewBase/ViewRegistry`
2. `refactor(backlinks): migrate to ViewBase/ViewRegistry`
3. `refactor(search): migrate to ViewBase/ViewRegistry`
4. `refactor(graph): migrate to ViewBase/ViewRegistry`
5. `refactor(ai): migrate to ViewBase/ViewRegistry`
6. `refactor(agent): migrate to ViewBase/ViewRegistry`

Keeping them independent makes bisecting + reverting trivial.

### 7.6 Acceptance

- Every first-party plugin in `shell/src/plugins/nexus/` registers via `viewRegistry.register` + `api.views.register` rather than `slotRegistry.register` for view-shaped contributions.
- `grep -rn "slotRegistry.register" shell/src/plugins/` returns only non-view contributions (overlays, status items).
- Manual smoke test: open each view, close it, reopen — no console errors, state round-trips.

---

## 8. WI-25 — Eliminate direct `@tauri-apps/*` imports in plugins

### 8.1 Intent

Route every filesystem / dialog / window operation through the kernel bridge so the WI-23 guardrail is green and plugin sandboxing (Phase 3) has one chokepoint. Currently 9+ files in `shell/src/plugins/` import `@tauri-apps/plugin-fs`, `@tauri-apps/plugin-dialog`, or `@tauri-apps/api/window` directly.

### 8.2 Current state

Per audit: violations in `core/editorArea`, `core/fileExplorer`, `nexus/editor`, `nexus/launcher`, possibly others. Each is a plugin reading/writing files or opening native dialogs directly, bypassing `api.kernel.invoke('com.nexus.storage', ...)`.

### 8.3 Design

Two substitution patterns:

**Filesystem ops → kernel IPC:**

| Legacy (direct) | Replacement (bridge) |
|---|---|
| `readTextFile(path)` | `api.kernel.invoke('com.nexus.storage', 'read_file', { path })` |
| `writeTextFile(path, content)` | `api.kernel.invoke('com.nexus.storage', 'write_file', { path, content })` |
| `readDir(path)` | `api.kernel.invoke('com.nexus.storage', 'list_dir', { path })` |
| `exists(path)` | `api.kernel.invoke('com.nexus.storage', 'exists', { path })` (add IPC if missing) |

**Dialog / window ops → thin shell-side adapter:**

For native dialogs (open / save / confirm), these aren't kernel concerns. Add a small shell-side API `api.shell.dialog.open()` that routes via a new Tauri command in `shell/src-tauri/src/lib.rs` (one of the few places new Tauri commands are legitimate — not in the legacy crate). `@nexus/extension-api` declares the shape.

Window ops (resize, focus, minimize) same pattern: `api.shell.window.*`.

**Do NOT** re-export `@tauri-apps/*` from the package — that defeats the point. The API wrappers are the contract.

### 8.4 Subagent pattern

Similar to WI-24 — per-file mechanical refactor. Could fan out one agent per violator file (~9 agents), but I'd sequence this:

**Step 1 (main thread):** audit all violators, write the shell-side adapter for dialog/window ops, add missing kernel IPC commands (if any — `exists` may need adding to `com.nexus.storage`).

**Step 2 (parallel agents):** one agent per plugin file, prompt: *"Replace all `@tauri-apps/plugin-fs` imports in `<file>` with calls to `api.kernel.invoke(...)`. Replace any `@tauri-apps/plugin-dialog` calls with `api.shell.dialog.*`. Don't change external behaviour. Report the diff."*

**Step 3 (main thread):** merge, verify `pnpm test` passes (WI-23 guardrail comes clean), remove allowlisted files from the WI-23 exemption list as each one drains.

### 8.5 Commit plan

2 + N commits:

1. `feat(shell-api): add api.shell.dialog and api.shell.window adapters`
2. `feat(storage): add com.nexus.storage::exists IPC command` (if needed)
3. One refactor commit per plugin file (~9 commits), e.g. `refactor(editor): route file I/O through kernel bridge`

### 8.6 Acceptance

- `grep -rn "@tauri-apps/" shell/src/plugins/` returns zero matches.
- WI-23 allowlist is empty.
- Manual regression: file operations, dialogs, window controls all still work end-to-end.

---

## 9. Dependency graph & parallelization

```
             ┌──────────────────────────────────────────┐
             │ WI-22 (Rust freeze guardrail)           │  XS, no deps
             └──────────────────────────────────────────┘

             ┌──────────────────────────────────────────┐
             │ WI-06 (bridge subscription + errors)    │  S, no deps
             └─────────────────┬────────────────────────┘
                               │
                 ┌─────────────┘
                 ▼
             ┌──────────────────────────────────────────┐
  Unblocks → │ Phase 2 WI-01 (AI chat streaming)       │
             │ Phase 2 WI-07 (agent panel streaming)   │
             │ Phase 2 WI-12 (terminal streaming)      │
             └──────────────────────────────────────────┘

             ┌──────────────────────────────────────────┐
             │ WI-20 (ts-rs + extension-api unify)     │  M, no deps
             └─────────────────┬────────────────────────┘
                               │
             ┌─────────────────┴─────────────────┐
             ▼                                   ▼
   ┌────────────────────┐          ┌────────────────────────┐
   │ WI-23 (shell      │          │ WI-24 (Leaf view      │
   │ import guardrail) │          │ wrapping — 6 views)   │
   │ S, depends on 20  │          │ M, depends on 20       │
   └─────────┬──────────┘          └───────────┬────────────┘
             │                                 │
             └────────────────┬────────────────┘
                              ▼
                   ┌────────────────────────────┐
                   │ WI-25 (drop raw Tauri     │
                   │ imports; drain allowlist) │
                   │ S                          │
                   └────────────────────────────┘
```

### 9.1 Single-engineer serialization (5 weeks)

Week 1: WI-22 (1 day) → WI-06 (3–4 days)
Week 2: WI-20 Rust half (ts-rs derivation + extension-api barrel)
Week 3: WI-20 TS half (consolidation, CI drift) + WI-23 (shell guardrail)
Week 4: WI-24 (six view wraps in parallel via agents)
Week 5: WI-25 (nine refactors) + cleanup + acceptance tests

### 9.2 Two-engineer parallelization (~2½ calendar weeks)

Engineer A (Rust-leaning): WI-22 → WI-06 Rust half → WI-20 Rust half.
Engineer B (TS-leaning): WI-20 TS half (after A ships generator) → WI-23 → WI-24 (delegate to agents) → WI-25.

The critical path is WI-20 → WI-24/25. WI-06 can ship independently on A's track.

### 9.3 Agent-heavy run (one engineer + Claude agents, ~2 weeks)

Most of WI-20, WI-24, and WI-25 is repetitive mechanical work that fans out across agents cleanly. Agent throughput caps at roughly 6 parallel Explore agents per wave; with prompt caching, a single wave runs in ~5 min. Human-in-the-loop bottleneck is reviewing outputs and dealing with the 10–20% that come back needing correction.

Realistic daily pace: 2–3 WIs per day if prompts are tight and the main thread stays focused on review + integration. Two weeks of focused work → Phase 1 done.

---

## 10. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| ts-rs refuses to derive on a type (e.g. `Arc<T>`, recursive enums) | Medium | Agent 1 reports back which types failed; fall back to hand-authoring those TS shapes rather than stalling. |
| Subscription tracking exposes a pre-existing leak in ExtensionHost (double-track of commands or slots) | Medium | The regression test catches this. Fix in the same PR as WI-06 if it surfaces. |
| WI-24 view wrapping reveals per-view state that doesn't survive serialization | Low | The `getState`/`setState` pattern is additive; plugins that need it add it incrementally. Default: no state. |
| WI-25 uncovers a Tauri API plugins use that has no kernel equivalent (rare — most are FS or dialog) | Medium | Expected for window ops; the `api.shell.*` adapter is already in the design. If something truly needs a bespoke Tauri command, allow it in `shell/src-tauri/src/lib.rs` (not `nexus-app`) and document in an ADR. |
| CRLF drift from the earlier audit recurs during implementation | Low | Agent prompts should specify "preserve existing file encoding/line endings"; periodic `git diff --check` during work. |
| Agents return subtly wrong code that passes `tsc` but breaks at runtime | Medium | Every WI has a regression test spec. Don't merge an agent's output without the test running green locally. |

---

## 11. Open questions for user before execution

None blocking. A few low-stakes calls that can be deferred to implementation time:

1. **Where should `IpcErrorEnvelope` live?** Propose `crates/nexus-plugin-api/src/error.rs` (so ts-rs picks it up for WI-20). Alternative: invent it in `bridge.rs` and hand-author the TS twin. Recommendation: plugin-api.

2. **Does the Phase 1 acceptance test need to be E2E (full Tauri window), or is a unit-test harness that mocks Tauri commands enough?** Recommendation: unit-test harness for Phase 1, reserve full E2E for Phase 4 polish.

3. **What's the naming convention for `api.shell.*`?** Is `api.shell.dialog.openFile(opts)` acceptable, or does the stability-minded `@nexus/extension-api` want a different namespace? Recommendation: `api.platform.*` (future-proof if the shell isn't always Tauri).

Flag these during the kickoff of WI-20 so they're settled before code lands.

---

## 12. What this plan does NOT cover

- **Phase 2 parity migration.** Separate document: `PARITY-CHECKLIST.md`. Starts after Phase 1 acceptance.
- **Phase 3 security hardening.** Iframe sandbox, install-time capability prompt, TOCTOU fix, api_version enforcement. Starts after Phase 2.
- **Phase 4+ — frontend unification, retiring `crates/nexus-app`, v1 ship.** Far out; don't design yet.
- **Rewriting the legacy shell (`app/`).** Frozen per ADR 0011; touch only for security patches.

---

## 13. Next action

Review this plan. If approved, the execution order is:

1. Start with WI-22 (half-day, risk-free) to validate the workflow.
2. WI-06 in parallel (or immediately after) — no dependencies, unblocks Phase 2 P0s.
3. Kick off WI-20 (big item; the ts-rs generator is the payoff).
4. WI-23, then WI-24 (agent-heavy), then WI-25 drain.
5. Acceptance smoke test + ship Phase 1 complete.

Each WI has its own commit plan in §3–§8; we'll land them incrementally per the Phase 0 workflow.
