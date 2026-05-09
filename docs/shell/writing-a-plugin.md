# Writing a Plugin — Reference

This document is the in-depth reference for writing Nexus shell plugins.
It assumes you have already scaffolded a plugin with
`nexus plugin scaffold --template script` — if not, start with the
[quickstart](../../docs/writing-your-first-plugin.md) and come back
here once your plugin boots.

The reference is organised around the five subsystems every
non-trivial plugin touches:

1. [Manifest and activation events](#manifest-and-activation-events)
2. [The `@nexus/extension-api` import surface](#the-nexusextension-api-import-surface)
3. [Sandbox model](#sandbox-model)
4. [Capability declarations](#capability-declarations)
5. [Slot system and contributions](#slot-system-and-contributions)

At the end, a [worked example](#worked-example-word-count) reimplements
the classic word-count panel as a reference plugin that exercises
every subsystem.

---

## Manifest and activation events

Every plugin ships a `plugin.json` (sandboxed JS/TS) or exports a
`manifest` object (native TS host plugin). The shape is defined in
`shell/src/types/plugin.ts`:

```ts
interface PluginManifest {
  id: string                 // reverse-DNS: "com.example.word-count"
  name: string
  version: string
  core: boolean              // false for user/community plugins
  activationEvents: string[] // when the host should run activate()
  apiVersion?: number        // @nexus/extension-api version
  dependsOn?: string[]       // plugin ids that must activate first
  contributes?: PluginContributions
}
```

### Activation events (Phase 2 WI-19)

`activationEvents` decides *when* `activate()` runs. The host defers
activation until one of the events fires — this is what keeps boot
fast when 30+ plugins are registered. Supported kinds:

| Event              | Fires when                                            |
|--------------------|-------------------------------------------------------|
| `onStartup`        | immediately after `loadAll` completes                 |
| `onCommand:<id>`   | a registered command is about to be executed          |
| `onView:<slotId>`  | the shell is about to mount a view in the named slot  |
| `onLanguage:<id>`  | a file of the given language is opened in the editor  |
| `onFileOpen:<glob>`| a file matching the glob is opened                    |

See `shell/src/host/ExtensionHost.test.ts` for the canonical
behaviour — a plugin with `['onView:foo']` does *not* activate during
`loadAll`; it activates when the first mount of slot `foo` fires the
trigger.

Pick the narrowest event that works. `onStartup` is appropriate for
core services and chrome (activity bar, status bar), but for leaf
features prefer `onView:` or `onCommand:`.

### `dependsOn`

If your plugin calls `api.configuration.register(...)` inside
`activate()`, list `core.configuration-service` in `dependsOn` so the
host activates it first. Cycles are rejected at load time.

---

## The `@nexus/extension-api` import surface

Sandboxed community plugins import exclusively from
`@nexus/extension-api`, the versioned ABI package at
`packages/nexus-extension-api/`. The package is types-only at compile
time — the runtime context is injected by the host across the
sandbox iframe boundary.

Key exports (see `packages/nexus-extension-api/src/index.ts`):

```ts
import {
  bootstrapSandboxedPlugin,    // runtime entry — call this last in index.ts
  type SandboxedPlugin,        // { activate(ctx), deactivate?() }
  type SandboxedPluginContext, // the ctx handed to activate()
  type PanelNode,              // declarative view tree
  type Capability,             // capability taxonomy (ts-rs generated)
} from '@nexus/extension-api'
```

Runtime surface on `SandboxedPluginContext` (see
`packages/nexus-extension-api/src/sandbox/context.ts`):

- `commands` — register / execute commands
- `notifications` — transient toasts
- `views` — `registerPanel`, `registerTree`, `registerWebview`
- `storage` — per-plugin KV
- `statusBar` — create and update status bar items
- `kernel.invoke` — typed IPC to native service crates
- `events` — subscribe to typed shell events

Never import from `shell/src/*` in a sandboxed plugin — the sandbox
iframe has no module resolution for host internals. If a symbol isn't
re-exported from `@nexus/extension-api`, either it's host-only or it
needs to be promoted (file an issue).

### Host-side (non-sandboxed) plugins

Plugins under `shell/src/plugins/{core,nexus}/` are trusted,
React-rendering plugins that run inside the host process. They
import from `shell/src/types/plugin` directly and receive a
`PluginAPI` (not `SandboxedPluginContext`). The two surfaces are
similar but not identical — sandboxed plugins get `PanelNode` trees;
host plugins can register React components directly.

Which one you want:

| You need                                        | Use          |
|-------------------------------------------------|--------------|
| Third-party / untrusted code                    | sandboxed    |
| Renders its own React components                | host         |
| Calls service-crate IPC with per-user consent   | either       |
| Only available in `~/.nexus-shell/plugins/`     | sandboxed    |
| Shipped with the shell binary                   | host (nexus.*) |

---

## Sandbox model

Phase 3 WI-30 introduced the sandbox: sandboxed plugins run in a
null-origin iframe, communicate with the host over `postMessage`,
and render panels declaratively. The orchestrator lives at
`shell/src/host/sandbox/` — see
`packages/nexus-extension-api/src/sandbox/runtime.ts` for the author
side.

`plugin.json` opts in:

```json
{
  "id": "com.example.word-count",
  "name": "Word Count",
  "version": "1.0.0",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": ["UiNotify", "KvRead", "KvWrite"],
  "activationEvents": ["onStartup"]
}
```

Consequences of `"sandboxed": true`:

- The plugin's JS is loaded into an iframe with `sandbox=""` and no
  DOM access outside the panel tree.
- `document`, `window`, `localStorage`, `fetch`, etc. are all
  either absent or proxied/denied.
- Panels are described by `PanelNode` trees (`vstack`, `hstack`,
  `heading`, `button`, `text`, `input`, …) — no JSX crosses the
  boundary.
- `ctx.kernel.invoke(method, params)` is the only way to reach
  service-crate IPC; capabilities gate which methods succeed.

If your plugin is first-party and ships inside the shell binary,
drop the `sandboxed` flag — you get the full host `PluginAPI`
(including React registration) in exchange for being trusted code.

---

## Capability declarations

Phase 3 WI-31 added capability consent. Every gated API call checks
the plugin's declared `capabilities` against an enforced taxonomy.
Host rejects any call whose capability wasn't declared, *and* the
user is prompted at first-activation for any non-trivial capability.

The authoritative enum lives in `crates/nexus-plugin-api/src/capability.rs`
and is re-exported to TypeScript via ts-rs (see
`packages/nexus-extension-api/src/generated/`). Current variants:

| Capability          | Grants                                      |
|---------------------|---------------------------------------------|
| `FsRead`            | read forge files via storage IPC            |
| `FsWrite`           | write forge files                           |
| `FsReadExternal`    | read files outside the forge (prompted)     |
| `FsWriteExternal`   | write files outside the forge (prompted)    |
| `NetHttp`           | arbitrary outbound HTTP                     |
| `NetHttpLocalhost`  | HTTP to localhost only                      |
| `ProcessSpawn`      | spawn child processes                       |
| `KvRead` / `KvWrite`| per-plugin KV storage                       |
| `IpcCall`           | invoke other plugins' IPC handlers          |
| `DbQuery` / `DbWrite` | read/write SQLite index directly          |
| `EventsPublish`     | publish on the shared event bus             |
| `UiNotify`          | show toast notifications                    |

Declare the narrowest set that works. An empty `capabilities: []`
is perfectly valid for a purely-visual panel plugin; it just can't
talk to the kernel.

---

## Slot system and contributions

The shell renders plugin-contributed UI into named slots. Slots are
registered in `shell/src/registry/SlotRegistry.ts`. Currently
defined:

`activityBar`, `sidebar`, `sidebarContent`, `rightPanel`,
`rightPanelContent`, `editorArea`, `panelArea`, `paneMode`,
`statusBarLeft`, `statusBarRight`, `overlay`.

A view contribution binds a plugin view to a slot:

```ts
contributes: {
  views: [
    { id: 'wordCountPanel', slot: 'sidebar', title: 'Word Count', priority: 80 }
  ]
}
```

At `activate()` time the plugin registers the component:

```ts
api.views.register('wordCountPanel', {
  slot: 'sidebar',
  component: WordCountView,
  priority: 80,
})
```

Other contribution kinds (see `PluginContributions` in
`shell/src/types/plugin.ts`): `commands`, `menus`, `keybindings`,
`statusBarItems`, `configuration`, `contextKeys`.

The `deactivate()` hook is usually empty — the registry sweep removes
all contributions automatically. Only override it if you hold
resources outside the registry (timers, subscriptions to services
that don't go through `api.events`).

---

## Worked example: word count

This is the original tutorial example, updated to reflect current
APIs. It registers a sidebar view, a status bar item, a command, and
a config section.

### Manifest (host plugin form)

```ts
// shell/src/plugins/community/wordCount/index.ts
import type { Plugin } from '../../../types/plugin'
import { WordCountView } from './WordCountView'

export const wordCountPlugin: Plugin = {
  manifest: {
    id: 'community.word-count',
    name: 'Word Count',
    version: '1.0.0',
    core: false,
    activationEvents: ['onView:sidebar'],
    dependsOn: ['core.configuration-service'],
    contributes: {
      commands: [
        { id: 'wordCount.toggle', title: 'Toggle Word Count Panel', category: 'Word Count' },
      ],
      keybindings: [
        { command: 'wordCount.toggle', key: 'ctrl+shift+w', when: 'sidebarVisible' },
      ],
      views: [
        { id: 'wordCountPanel', slot: 'sidebar', title: 'Word Count', priority: 80 },
      ],
    },
  },

  activate(api) {
    api.views.register('wordCountPanel', {
      slot: 'sidebar',
      component: WordCountView,
      priority: 80,
    })

    const statusItem = api.statusBar.createItem({
      id: 'wordCount.statusItem',
      slot: 'statusBarRight',
      priority: 20,
      text: '0 words',
      tooltip: 'Word count',
      command: 'wordCount.toggle',
    })

    api.events.on('editor:contentChanged', ({ content }) => {
      const count = countWords(content)
      statusItem.text = `${count} words`
      api.context.set('wordCount.currentCount', count)
    })

    api.commands.register('wordCount.toggle', () => {
      const visible = api.context.get('wordCount.panelVisible') ?? true
      api.context.set('wordCount.panelVisible', !visible)
    })

    api.configuration.register({
      pluginId: 'community.word-count',
      title: 'Word Count',
      order: 100,
      schema: [
        { key: 'wordCount.includeCode', title: 'Include code blocks', type: 'boolean', default: false },
        { key: 'wordCount.includeHeadings', title: 'Include headings', type: 'boolean', default: true },
      ],
    })

    api.context.set('wordCount.panelVisible', true)
  },
}
```

### View component

```tsx
// shell/src/plugins/community/wordCount/WordCountView.tsx
import { useContextKey } from '../../../host/ContextKeyService'
import { useConfigValue } from '../../../stores/configStore'

export function WordCountView() {
  const visible = useContextKey('wordCount.panelVisible')
  const count = (useContextKey('wordCount.currentCount') as number) ?? 0
  const includeCode = useConfigValue('wordCount.includeCode', false)
  if (!visible) return null
  return (
    <div className="word-count-panel">
      <div className="wc-header">Word Count</div>
      <div className="wc-value">{count.toLocaleString()} words</div>
      <div className="wc-note">{includeCode ? 'Including code blocks' : 'Excluding code blocks'}</div>
    </div>
  )
}
```

### Sandboxed form (PanelNode)

The same plugin as a sandboxed community plugin — note the
`PanelNode` tree instead of a React component, and capability
declaration instead of free access to kernel state:

```ts
import {
  bootstrapSandboxedPlugin,
  type SandboxedPlugin,
} from '@nexus/extension-api'

const plugin: SandboxedPlugin = {
  async activate(ctx) {
    let count = 0

    ctx.commands.register('wordCount.toggle', async () => {
      await ctx.notifications.show({ message: `${count} words`, type: 'info' })
    })

    ctx.events.on('editor:contentChanged', ({ content }) => {
      count = content.trim().split(/\s+/).filter(Boolean).length
      ctx.statusBar.update('wordCount.statusItem', { text: `${count} words` })
    })

    ctx.statusBar.create({
      id: 'wordCount.statusItem',
      slot: 'statusBarRight',
      text: '0 words',
      command: 'wordCount.toggle',
    })

    ctx.views.registerPanel('wordCountPanel', () => ({
      type: 'vstack',
      gap: 8,
      children: [
        { type: 'heading', value: 'Word Count', level: 2 },
        { type: 'text', value: `${count} words` },
      ],
    }))
  },
}

bootstrapSandboxedPlugin(plugin)
export default plugin
```

Matching `plugin.json`:

```json
{
  "id": "community.word-count",
  "name": "Word Count",
  "version": "1.0.0",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": ["UiNotify", "EventsPublish"],
  "activationEvents": ["onView:sidebar"]
}
```

---

## Plugin checklist

Before loading a plugin in a real forge:

- [ ] `id` follows reverse-DNS convention
- [ ] `activationEvents` is as narrow as possible (avoid `onStartup` for leaf features)
- [ ] `dependsOn` lists every service you call in `activate()`
- [ ] Every command in the manifest has a handler registered
- [ ] Every view id in `contributes.views` matches an `api.views.register` call
- [ ] Context keys and config keys are namespaced (`myPlugin.keyName`)
- [ ] No direct imports from other plugins — use events
- [ ] No hardcoded colors — use CSS custom properties
- [ ] Sandboxed plugins declare the minimum viable `capabilities` set
- [ ] `deactivate()` only implements non-registry cleanup

---

## Related docs

- [Plugin quickstart](../../docs/writing-your-first-plugin.md) — the scaffold-to-install path
- [Architecture overview](architecture.md) — shell substrate and plugin layers
- [Plugin system](plugin-system.md) — loading, lifecycle, registry
- [Extension host](extension-host.md) — activation events in depth
- [Slot system](slot-system.md) — every slot and its contract
- [Plugin API](plugin-api.md) — full `PluginAPI` surface (host plugins)
- `packages/nexus-extension-api/src/sandbox/context.ts` — `SandboxedPluginContext` surface
- `crates/nexus-plugin-api/src/capability.rs` — authoritative capability enum
