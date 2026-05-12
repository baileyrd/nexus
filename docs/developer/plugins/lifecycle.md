# Lifecycle and activation

A plugin moves through a small fixed set of states. Activation events
control **when** the plugin's code first runs; lifecycle hooks
control **what** runs at each transition.

## States

```
       ┌────────┐  manifest read, ready to activate
       │ Loaded │
       └───┬────┘
           │ activation event fires
           ▼
     ┌─────────────┐  on_init() returned Ok
     │ Initialized │
     └──────┬──────┘
            │ on_start() returned Ok
            ▼
       ┌─────────┐
       │ Running │  steady state — handlers fire
       └────┬────┘
            │ on_stop() called
            ▼
       ┌─────────┐
       │ Stopped │
       └─────────┘

       ┌─────────┐  on any hook panic / error
       │ Crashed │
       └─────────┘
```

(Canonical enum: `PluginStatus` in `nexus-plugin-api/src/plugin.rs`.)

You don't usually observe these directly — they're surfaced in the
**Plugins** panel as a status pill and in `nexus plugin list`.

## Hooks (TypeScript / extension API)

Your plugin module exports a `Plugin` object with up to four hooks:

```ts
import type { Plugin, PluginContext } from '@nexus/extension-api';

export const plugin: Plugin = {
  id: 'com.example.hello',

  // Required. Called once when activation triggers.
  activate(ctx: PluginContext) {
    ctx.commands.register({ id: 'hello.sayHi', run: () => /* … */ });
  },

  // Optional. Called on shutdown or uninstall.
  // Use to release subscriptions, flush KV, close handles.
  deactivate() {
    /* clean up */
  },
};
```

The hooks return `void` or `Promise<void>`. A throw or rejection
moves the plugin to **Crashed** and prevents further hooks; the user
sees an error toast and can re-enable from the Plugins panel.

`activate` should return quickly (under a few hundred ms). Long-
running setup belongs in a background task spawned from `activate`.

## Activation events

Plugins are lazy by default. Listing activation events keeps idle
Nexus fast — most plugins activate only when needed.

| Event | Fires when |
|---|---|
| `onStartup` | Nexus boots (eager — use sparingly) |
| `onCommand:<id>` | A specific command is invoked from anywhere |
| `onView:<id>` | A specific view is opened |
| `onLanguage:<lang>` | A file of the language is opened (e.g. `markdown`) |
| `onEvent:<topic>` | A specific topic publishes on the event bus |
| `onFile:<glob>` | A file matching the glob is opened or focused |

In `plugin.json`:

```json
"activation": [
  "onCommand:hello.sayHi",
  "onView:hello.panel",
  "onEvent:files:changed"
]
```

If `activation` is omitted the default is `["onStartup"]`.

### Picking activation events

- Use `onStartup` only when your plugin needs to register a long-lived
  subscription that can't be deferred (e.g. a status-bar item that
  must always be present).
- Prefer `onCommand:` and `onView:` for everything else. The user
  pays no cost until they reach for your feature.
- `onEvent:<topic>` is for plugins that react to events — but be
  careful: if many plugins subscribe to a high-frequency topic
  (`editor:change`), the activation tax adds up.

## What `activate` should do

```ts
activate(ctx: PluginContext) {
  // 1. Register every contribution: commands, views, IPC handlers.
  ctx.commands.register({ id: 'hello.sayHi', run: handle });

  // 2. Subscribe to events you care about.
  ctx.events.subscribe('files:changed', (e) => /* … */);

  // 3. Restore state from KV if needed.
  const last = await ctx.kv.get('hello:lastName');

  // 4. Kick off any background work as a separate task.
  void backgroundSync(ctx);
}
```

Don't do heavy I/O directly in `activate`. Defer it.

## What `deactivate` should do

```ts
deactivate() {
  // 1. Cancel any in-flight async work.
  abortController.abort();

  // 2. Release subscriptions you registered.
  // (The kernel auto-releases registrations on deactivate, but if
  //  you hold native resources — file handles, intervals — clear them.)
  clearInterval(myInterval);
}
```

`deactivate` is best-effort. It runs on disable, on uninstall, and on
graceful Nexus shutdown. It does **not** run on a hard kill (SIGKILL,
power loss). Don't depend on it for data integrity — flush important
state on every change instead.

## Reload semantics

When the user reinstalls or upgrades a plugin:

1. The kernel calls `deactivate` on the old version.
2. The old plugin's KV namespace is preserved.
3. The new version is loaded; its manifest validated.
4. `activate` runs against a fresh `PluginContext`.

Hot reload during development uses the same path — no special
"dev mode". This means a bug in `deactivate` can leak state across
reloads; keep cleanup simple.

## Crashes

A panic in `activate`, an unhandled async rejection, or a sandbox
violation moves the plugin to **Crashed**. The kernel:

- Records the error in `<forge>/.forge/logs/plugin-events.jsonl`.
- Surfaces a toast to the user with a **Re-enable** action.
- Removes any registrations the plugin made before the crash.

Other plugins continue running. The crashed plugin is excluded from
event delivery and IPC dispatch until the user re-enables it.

## See also

- [Plugin overview](overview.md)
- [Manifest](manifest.md)
- `PluginStatus` enum: `crates/nexus-plugin-api/src/plugin.rs`
- Loader internals (advanced):
  [`../../shell/extension-host.md`](../../shell/extension-host.md)
