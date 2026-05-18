# Extension Host

The extension host is responsible for the full lifecycle of every plugin: loading, activating, and unloading. It is the only place that calls plugin code.

## Responsibilities

- Accept a list of plugins and load them in dependency order
- Call `activate(api)` on each plugin with the appropriate API object
- Register manifest contributions before `activate()` runs
- Track loaded plugins and their states
- Call `deactivate()` on unload and sweep all registry contributions
- Emit lifecycle events on the event bus

---

## Implementation

```typescript
// src/host/ExtensionHost.ts

import { PluginRegistry } from './PluginRegistry'
import { buildPluginAPI } from './PluginAPI'
import { eventBus } from './EventBus'
import type { Plugin } from '../types/plugin'

export type PluginState =
  | 'registered'
  | 'activating'
  | 'active'
  | 'deactivating'
  | 'inactive'
  | 'error'

export class ExtensionHost {
  private registry: PluginRegistry
  private plugins = new Map<string, Plugin>()
  private states = new Map<string, PluginState>()
  private errors = new Map<string, Error>()

  constructor(registry: PluginRegistry) {
    this.registry = registry
  }

  // Load all plugins in dependency order
  async loadAll(plugins: Plugin[]) {
    // Register all manifests first so dependency resolution can read them
    for (const plugin of plugins) {
      this.plugins.set(plugin.manifest.id, plugin)
      this.states.set(plugin.manifest.id, 'registered')
    }

    const ordered = this.resolveDependencyOrder(plugins)

    for (const plugin of ordered) {
      await this.activate(plugin)
    }
  }

  // Activate a single plugin
  async activate(plugin: Plugin) {
    const { id, core } = plugin.manifest

    if (this.states.get(id) === 'active') return
    if (this.states.get(id) === 'error') return

    // Ensure all dependencies are active first
    for (const depId of plugin.manifest.dependsOn ?? []) {
      const dep = this.plugins.get(depId)
      if (!dep) {
        const err = new Error(`Plugin '${id}' requires '${depId}' which is not loaded`)
        this.states.set(id, 'error')
        this.errors.set(id, err)
        console.error(err.message)
        return
      }
      if (this.states.get(depId) !== 'active') {
        await this.activate(dep)
      }
    }

    this.states.set(id, 'activating')

    // Register static manifest contributions before activate() runs
    // This populates the command palette with labels immediately
    this.registerManifestContributions(plugin)

    // Build the API object — core plugins get internal API access
    const api = buildPluginAPI(this.registry, { isCore: core, pluginId: id })

    try {
      await plugin.activate(api)
      this.states.set(id, 'active')
      eventBus.emit('plugin:activated', { pluginId: id })
    } catch (err) {
      this.states.set(id, 'error')
      this.errors.set(id, err as Error)
      // Clean up any partial registrations
      this.registry.unregisterAll(id)
      console.error(`Plugin '${id}' failed to activate:`, err)
    }
  }

  // Unload a plugin — cleans up all contributions automatically
  async unload(id: string) {
    const plugin = this.plugins.get(id)
    if (!plugin || this.states.get(id) !== 'active') return

    this.states.set(id, 'deactivating')

    try {
      await plugin.deactivate?.()
    } catch (err) {
      console.error(`Plugin '${id}' deactivate() threw:`, err)
    }

    // Sweep all registry contributions this plugin made
    this.registry.unregisterAll(id)

    this.states.set(id, 'inactive')
    eventBus.emit('plugin:deactivated', { pluginId: id })
  }

  getState(id: string): PluginState | undefined {
    return this.states.get(id)
  }

  getError(id: string): Error | undefined {
    return this.errors.get(id)
  }

  listLoaded(): string[] {
    return [...this.states.entries()]
      .filter(([, state]) => state === 'active')
      .map(([id]) => id)
  }

  // Register contributions declared in the manifest
  // These are registered before activate() so the command palette
  // can show them even before plugin code has run
  private registerManifestContributions(plugin: Plugin) {
    const { id, contributes } = plugin.manifest
    if (!contributes) return

    contributes.commands?.forEach(c =>
      this.registry.commands.registerFromManifest(id, c))
    contributes.views?.forEach(v =>
      this.registry.views.registerFromManifest(id, v))
    contributes.menus?.forEach(m =>
      this.registry.menus.registerFromManifest(id, m))
    contributes.keybindings?.forEach(k =>
      this.registry.keybindings.registerFromManifest(id, k))
  }

  // Topological sort by dependsOn declarations
  // Core plugins sort before community plugins within each tier
  private resolveDependencyOrder(plugins: Plugin[]): Plugin[] {
    const visited = new Set<string>()
    const result: Plugin[] = []
    const pluginMap = new Map(plugins.map(p => [p.manifest.id, p]))

    const visit = (plugin: Plugin) => {
      if (visited.has(plugin.manifest.id)) return
      visited.add(plugin.manifest.id)

      for (const depId of plugin.manifest.dependsOn ?? []) {
        const dep = pluginMap.get(depId)
        if (dep) visit(dep)
      }

      result.push(plugin)
    }

    // Visit core plugins first
    plugins.filter(p => p.manifest.core).forEach(visit)
    // Then community plugins
    plugins.filter(p => !p.manifest.core).forEach(visit)

    return result
  }
}
```

---

## Plugin Lifecycle States

```
registered  → plugin is known but not yet activating
activating  → activate() is running
active      → activate() completed successfully
deactivating→ deactivate() is running
inactive    → deactivate() completed
error       → activate() threw, or a dependency failed
```

A plugin in `error` state will not be retried. It must be explicitly reloaded.

---

## Lazy Activation

Plugins with activation events other than `onStartup` are not activated immediately. The extension host registers their manifest contributions and marks them `registered`, but defers `activate()` until the trigger fires.

```typescript
// When a command is about to execute:
if (this.states.get(owningPluginId) === 'registered') {
  await this.activate(this.plugins.get(owningPluginId)!)
}
// Then execute the command

// When a view is about to be shown:
if (this.states.get(owningPluginId) === 'registered') {
  await this.activate(this.plugins.get(owningPluginId)!)
}
// Then render the view
```

This is how VS Code loads extensions lazily — the command palette shows thousands of commands from hundreds of extensions on first open, but most extension code hasn't run yet.

---

## Lifecycle Events

The extension host emits these events on the event bus. Other plugins can subscribe:

| Event | Payload | When |
|---|---|---|
| `plugin:activated` | `{ pluginId: string }` | After activate() completes successfully |
| `plugin:deactivated` | `{ pluginId: string }` | After deactivate() and cleanup completes |
| `plugin:error` | `{ pluginId: string, error: Error }` | When activation fails |
