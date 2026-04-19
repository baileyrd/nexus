# Plugin System

## What is a Plugin?

A plugin is a TypeScript module that exports a `Plugin` object with three parts:

1. **A manifest** — static metadata declared before any code runs
2. **An `activate()` function** — called by the extension host when the plugin loads
3. **An optional `deactivate()` function** — called when the plugin unloads

```typescript
export const myPlugin: Plugin = {
  manifest: {
    id: 'my-org.my-plugin',
    name: 'My Plugin',
    version: '1.0.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['core.configuration-service'],
    contributes: {
      commands: [
        { id: 'myPlugin.doThing', title: 'Do the thing' }
      ],
      keybindings: [
        { command: 'myPlugin.doThing', key: 'ctrl+shift+t' }
      ]
    }
  },

  activate(api: PluginAPI) {
    api.commands.register('myPlugin.doThing', () => {
      console.log('doing the thing')
    })
  },

  deactivate() {
    // cleanup — but most cleanup is automatic via the registry's ownership tracking
  }
}
```

---

## The Manifest

The manifest is static data — it's read by the extension host before `activate()` is called. This enables the host to:

- Populate the command palette with command labels before the plugin code runs
- Resolve dependencies and determine load order
- Register keybindings immediately, before handlers exist (handlers are wired in `activate()`)

### Manifest fields

```typescript
interface PluginManifest {
  // Unique identifier. Convention: 'org.plugin-name' or 'core.plugin-name'
  id: string

  // Human-readable display name
  name: string

  // Semver version string
  version: string

  // true = loaded first, gets api.internal.*
  // false = loaded after core plugins, public API only
  core: boolean

  // When should this plugin activate?
  // 'onStartup'         — always, on app start
  // 'onCommand:id'      — lazily, when a specific command is first invoked
  // 'onView:id'         — lazily, when a specific view is first shown
  // 'onLanguage:lang'   — lazily, when a file of that language is opened
  activationEvents: string[]

  // Plugin IDs this plugin requires to be activated first
  dependsOn?: string[]

  // Static contributions registered before activate() runs
  contributes?: {
    commands?: CommandContribution[]
    views?: ViewContribution[]
    menus?: MenuContribution[]
    keybindings?: KeybindingContribution[]
    statusBarItems?: StatusBarContribution[]
    configuration?: ConfigSection
    contextKeys?: ContextKeyContribution[]
  }
}
```

### Contribution types

**CommandContribution**
```typescript
interface CommandContribution {
  id: string         // 'myPlugin.doThing'
  title: string      // 'Do the thing' — shown in command palette
  category?: string  // 'My Plugin' — groups commands in palette
  icon?: string      // icon identifier
}
```

**ViewContribution**
```typescript
interface ViewContribution {
  id: string            // 'myPlugin.myView'
  slot: SlotId          // which slot to render into
  title: string         // display name
  priority?: number     // position within the slot (lower = earlier)
}
```

**KeybindingContribution**
```typescript
interface KeybindingContribution {
  command: string     // command ID to execute
  key: string         // 'ctrl+shift+p'
  mac?: string        // 'cmd+shift+p' — overrides key on macOS
  when?: string       // context expression: 'editorFocus && !readOnly'
}
```

**MenuContribution**
```typescript
interface MenuContribution {
  menu: string        // 'file', 'edit', 'view', 'editor/context', etc.
  command: string     // command to invoke
  group?: string      // menu group for separator placement
  order?: number      // position within group
  when?: string       // context expression
}
```

---

## The activate() Function

`activate()` is called once when the plugin loads. It receives a `PluginAPI` object and should:

1. Wire command handlers for commands declared in the manifest
2. Register view components into slots
3. Subscribe to events
4. Initialize plugin state
5. Set up any timers or watchers

```typescript
activate(api: PluginAPI) {
  // Wire a command handler
  api.commands.register('myPlugin.doThing', async (arg?: string) => {
    // handler can be async
    const result = await doSomething(arg)
    api.notifications.show({ message: `Done: ${result}` })
  })

  // Register a view component
  api.views.register('myPlugin.myView', {
    slot: 'sidebar',
    component: MyViewComponent,
    priority: 30,
  })

  // Subscribe to an event
  api.events.on('file:opened', ({ path }) => {
    api.context.set('myPlugin.lastOpenedFile', path)
  })

  // Register a config section
  api.configuration.register({
    pluginId: 'my-org.my-plugin',
    title: 'My Plugin',
    order: 50,
    schema: [
      {
        key: 'myPlugin.enabled',
        title: 'Enable My Plugin',
        type: 'boolean',
        default: true,
        description: 'Enables or disables My Plugin features',
      }
    ]
  })
}
```

---

## The deactivate() Function

`deactivate()` is optional. Most cleanup happens automatically — the extension host tracks every registry contribution by plugin ID and sweeps them all on unload. You only need `deactivate()` for things outside the registry:

- Timers (`clearInterval`, `clearTimeout`)
- DOM event listeners added directly
- External connections (WebSocket, file watchers)
- Zustand subscriptions created with `.subscribe()`

```typescript
deactivate() {
  clearInterval(this.pollTimer)
  this.wsConnection?.close()
}
```

Note: You cannot store state on the plugin object directly in TypeScript without making the plugin a class. The pattern above is illustrative — in practice, store cleanup references in module-level variables or a local cleanup array.

---

## Core vs Community Plugins

### Core plugins (`core: true`)

- Loaded first, before community plugins
- Receive `api.internal.*` — direct access to the registry and service registration
- Can define new slot types
- Can register internal services consumed by other plugins
- Shipped with the app binary
- Trusted unconditionally

### Community plugins (`core: false`)

- Loaded after all core plugins have activated
- Receive public API only — `api.internal` is `undefined`
- Cannot define new slot types
- Cannot register internal services
- Installed by the user
- Capability-checked at API boundary

The API boundary enforcement is in `buildPluginAPI()`:

```typescript
const api = buildPluginAPI(registry, { isCore: plugin.manifest.core })
// isCore: false → api.internal is omitted from the returned object
```

---

## Activation Events

Activation events control when a plugin's code loads. This enables **lazy loading** — the plugin's manifest contributions (commands, views, keybindings) are registered immediately, but the actual plugin code only runs when needed.

| Event | Triggers when |
|---|---|
| `onStartup` | App starts — plugin loads immediately |
| `onCommand:myPlugin.doThing` | That command is first invoked |
| `onView:myPlugin.myView` | That view is first made visible |
| `onLanguage:typescript` | A TypeScript file is opened |

The command palette can show a command from a lazy plugin's manifest before the plugin has loaded. When the user selects it, the extension host loads the plugin, calls `activate()`, and then executes the command.

---

## Dependency Resolution

The extension host resolves the load order before loading anything:

```
1. Topological sort of all plugins by their dependsOn declarations
2. Core plugins always sort before community plugins
3. Within each tier, sort by dependency graph then by manifest order
4. If a cycle is detected, both plugins fail to load with an error
5. If a dependency is missing, the dependent plugin fails to load with an error
```

A plugin that declares `dependsOn: ['core.configuration-service']` is guaranteed that `api.configuration` is available when its `activate()` runs.

---

## Plugin Ownership and Cleanup

Every registration call tracks the owning plugin:

```
plugin.activate() calls api.commands.register('myPlugin.doThing', handler)
    ↓
CommandRegistry.register() stores the command
PluginRegistry.track('my-org.my-plugin', 'command:myPlugin.doThing')
    ↓
On plugin unload:
PluginRegistry.unregisterAll('my-org.my-plugin')
    → sweeps CommandRegistry, ViewRegistry, SlotRegistry, StatusBarRegistry, etc.
    → removes every contribution the plugin made
```

The result: when a plugin is disabled, every trace of it disappears from the shell immediately — commands gone from palette, views gone from slots, keybindings gone, status bar items gone. No restart required.
