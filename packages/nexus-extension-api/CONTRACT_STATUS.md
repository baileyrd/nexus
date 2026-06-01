# Plugin context contract status (#187)

This document tracks the divergence between the **target** plugin
context shape exported by this package (`NexusPluginContext` /
`ScriptPlugin`) and the **actual** runtime context shapes the in-tree
runtimes hand to plugins today. Until reconciliation lands, plugin
authors should code against the runtime-specific contract that matches
their plugin's tier, not the target contract.

Source: `docs/0.1.2/audits/expert-review-2026-05-31.md` (R4).

## The three shapes

| Shape | Source | Status |
|---|---|---|
| `NexusPluginContext` / `ScriptPlugin` | `packages/nexus-extension-api/src/index.ts` | **Aspirational** — no in-tree runtime implements this. |
| `PluginAPI` | `shell/src/types/plugin.ts` | **Live** — what the in-process shell host hands first-party plugins. |
| `SandboxedPluginContext` | `packages/nexus-extension-api/src/sandbox/context.ts` | **Live** — what the iframe sandbox runtime hands community plugins. |

`SandboxedPlugin` (`./src/sandbox/plugin.ts`) is the live runtime
counterpart of the aspirational `ScriptPlugin`; the in-process shell
loads first-party plugins via a different protocol entirely (see
`shell/src/host/extensionHost.ts`).

## Field-level divergence — `NexusPluginContext` vs. runtime shapes

| `NexusPluginContext` field | In `PluginAPI`? | In `SandboxedPluginContext`? | Notes |
|---|---|---|---|
| `pluginId: string` | ✗ (passed separately at load time) | ✓ | |
| `settings: { get(): Promise<Record<string, unknown>> }` | Partial — under `api.settings` (tab renderer registry, not values) | Different shape — sandbox `settings` exposes get/set for the plugin's own namespace | |
| `events: { emit }` | ✓ as `api.events` (with `on` + `off` too) | ✓ as `ctx.events` (async over postMessage) | `PluginAPI` is richer; both await-friendly |
| `ipc: { call }` | ✓ as `api.kernel.invoke` (different verb name) | Routed through `ctx.commands.execute` and host-side IPC | The verb name is the headline divergence here |
| `editor` | ✓ but with a *different* surface (`active`, `onChange`); registration verbs live on `viewRegistry` / `commands` | Absent — sandboxed plugins can't reach into the editor realm | |
| `ui` | Split across `api.notifications`, `api.views`, `api.activityBar`, `api.uri`, `api.statusBar` | Equivalent subset under `ctx.notifications`, `ctx.views`, `ctx.activityBar`, `ctx.statusBar` (async) | |
| `workspace: WorkspaceAPI` | ✓ as `api.workspace` (the live Leaf/View facade) | ✗ — live object refs can't cross postMessage | |
| `ai: AiAPI` | ✗ — not yet wired into `PluginAPI` | ✗ | |
| `disposables: DisposableStore` | ✗ — `PluginAPI` consumers track disposables manually | ✗ — sandbox host auto-sweeps `register*` results | |

## `ScriptPlugin` vs. runtime entry points

| Hook | `ScriptPlugin` | Runtime equivalent |
|---|---|---|
| Load | `loadScriptPlugin` (not implemented anywhere) | In-process: `extensionHost.loadFirstPartyPlugin` (different shape). Sandbox: `bootstrapSandboxedPlugin(SandboxedPlugin)`. |
| Init | `onInit(ctx)` | In-process: first-party module's exported `activate(api)`. Sandbox: collapsed into `SandboxedPlugin.activate(ctx)`. |
| Start | `onStart(ctx)` | Collapsed into the same `activate` hook in both runtimes. |
| Per-command | `dispatch(handlerId, args, ctx)` | In-process: command handlers registered via `api.commands.register`. Sandbox: ditto via `ctx.commands.register`. |
| Stop | `onStop(ctx)` | In-process: first-party module's exported `deactivate()`. Sandbox: `SandboxedPlugin.deactivate?()`. |

## Recommended migration

The audit recommends:

1. **Choose one canonical shape.** Most likely `PluginAPI` (the
   in-process runtime is the senior consumer); the aspirational
   `NexusPluginContext` can then be re-derived as a strict subset that
   the sandbox runtime can also satisfy after the async conversion.
2. **Add a type-level conformance test** asserting the chosen runtime
   shape `extends` the exported contract. The current divergence makes
   such a test impossible to write green today — that's the gating
   signal.
3. **Drop "frozen 1.0.0" framing** from the package surface until the
   above lands. ← *covered by the same PR that landed this document.*

Tracking issue: [#187](https://github.com/baileyrd/nexus/issues/187).
