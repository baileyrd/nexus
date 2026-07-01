# Plugin context contract status (#187)

**Status: reconciled (2026-07-01).** `NexusPluginContext` is no longer
aspirational — it is the **common plugin contract**: the subset of verbs
both in-tree runtimes hand plugins today, expressed with
`MaybePromise<T>` returns wherever the tiers disagree on
sync-versus-async. Both live context shapes structurally satisfy it,
and that fact is locked by compile-only conformance tests (the gating
signal the original audit asked for):

| Tier | Runtime shape | Conformance test |
|---|---|---|
| In-process shell host | `PluginAPI` (`shell/src/types/plugin.ts`) | `shell/src/types/contractConformance.test-d.ts` + runtime key-walk in `shell/src/host/PluginAPI.test.ts` |
| Sandboxed community plugins | `SandboxedPluginContext` (`./src/sandbox/context.ts`) | `src/contractConformance.test-d.ts` |

Both tests participate in CI (`pnpm --filter @nexus/extension-api
check`, `pnpm --filter nexus-shell typecheck` / `test`). A contract
edit either runtime cannot satisfy fails `tsc`.

Source: `docs/0.1.2/audits/expert-review-2026-05-31.md` (R4) and
`docs/0.1.2/audits/repo-review-2026-06-10.md` (V9). Tracking issue:
[#187](https://github.com/baileyrd/nexus/issues/187).

## The common contract

`NexusPluginContext` promises exactly the verbs available in **every**
tier:

`pluginId` · `commands.{register,execute}` · `kernel.{invoke,on}` ·
`platform.{fs,dialog,window,shell}` · `events.{on,emit}` ·
`storage.{get,set,delete}` · `notifications.show` ·
`context.{set,get,evaluate}` · `input.{prompt,confirm}` ·
`uri.register` · `activityBar.{addItem,removeItem}` ·
`statusBar.createItem`

Portable rules:

1. **Always `await`** `MaybePromise`-returning calls (`storage.*`,
   `notifications.show`, `context.set/evaluate`,
   `statusBar.createItem`) and treat `context.get` results as
   promise-wrapped. Awaiting a plain value is a no-op, so portable code
   pays nothing in the sync tier.
2. **Don't rely on `register`/`addItem` return values.** The sandbox
   tier returns a `Disposable`; the in-process tier returns `void` and
   sweeps registrations via `PluginRegistry.unregisterAll`.
3. **Treat status-bar handles as `NexusStatusBarItemHandle`** —
   `dispose` is the only cross-tier verb.

## What stays tier-specific

| Surface | Tier | Why it can't be common (today) |
|---|---|---|
| `workspace` / `viewRegistry` (live Leaf/View facade) | in-process only | live object refs can't cross `postMessage` |
| `views.register` (React component slots) | in-process only | React nodes can't be structured-cloned |
| `views.registerPanel` (declarative `PanelNode`) | sandbox only | the in-process tier registers React components instead |
| `configuration` / `settings` / `keybindings` / `fs` / `editor` | in-process only | not yet bridged over RPC (`configuration` is the sandbox TODO in `./src/sandbox/context.ts`) |
| `internal` | in-process core plugins only | trust boundary |
| `input.pick`, `kernel.available`, `storage.clear`, `commands.all` | in-process only | additive conveniences; candidates for promotion into the contract once the sandbox bridges them |

Closing rows in this table (bridging `configuration`, promoting
`input.pick`, …) is the remaining #187 follow-up work — each bridged
surface moves from this table into the common contract with the
conformance tests keeping both tiers honest.

## Entry points

The entry-point contract resolved to the two-hook shape both runtimes
already use:

| Hook | In-process (`Plugin`, shell-side) | Sandbox (`SandboxedPlugin`) |
|---|---|---|
| Activate | exported `activate(api: PluginAPI)` | `activate(ctx: SandboxedPluginContext)` |
| Deactivate | exported `deactivate()` | `deactivate?()` |

`ScriptPlugin` (the four-hook `dispatch`/`onInit`/`onStart`/`onStop`
shape) was never implemented by any runtime and is **deprecated** as of
0.1.0 — see [`DEPRECATED.md`](./DEPRECATED.md). Removal target: 0.2.0.

## Versioning

Re-cut from `1.0.0` to `0.1.0` (V9): the original tag predated the
runtime audit and implied a structural freeze that did not exist.
`1.0.0` will be re-cut once the common contract and its conformance
gates have soaked for a release. In-repo consumers are unaffected
(`workspace:*`).
