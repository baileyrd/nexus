// src/host/ExtensionHost.ts
// Manages the full plugin lifecycle: loading, activating, unloading.
// The only place that calls plugin code.

import type { Plugin } from '../types/plugin'
import { PluginRegistry } from './PluginRegistry'
import { buildPluginAPI } from './PluginAPI'
import { eventBus } from './EventBus'

export type PluginState =
  | 'registered'    // known but not yet activating
  | 'activating'    // activate() is running
  | 'active'        // fully loaded
  | 'deactivating'  // deactivate() is running
  | 'inactive'      // cleanly unloaded
  | 'error'         // activation failed

export class ExtensionHost {
  private registry: PluginRegistry
  private plugins  = new Map<string, Plugin>()
  private states   = new Map<string, PluginState>()
  private errors   = new Map<string, Error>()

  constructor(registry: PluginRegistry) {
    this.registry = registry
  }

  // ─── Public API ──────────────────────────────────────────────────────────

  /** Load all plugins in dependency order */
  async loadAll(plugins: Plugin[]) {
    // Register all manifests first so dependency resolution can see them
    for (const plugin of plugins) {
      this.plugins.set(plugin.manifest.id, plugin)
      this.states.set(plugin.manifest.id, 'registered')
    }

    const ordered = this.resolveDependencyOrder(plugins)

    for (const plugin of ordered) {
      await this.activate(plugin)
    }
  }

  /** Activate a single plugin (respects dependency chain) */
  async activate(plugin: Plugin) {
    const { id, core } = plugin.manifest
    const state = this.states.get(id)

    if (state === 'active')      return  // already loaded
    if (state === 'activating')  return  // circular — handled by topological sort
    if (state === 'error')       return  // failed — won't retry

    // Ensure all dependencies are active first
    for (const depId of plugin.manifest.dependsOn ?? []) {
      const dep = this.plugins.get(depId)
      if (!dep) {
        this.fail(id, new Error(
          `Plugin '${id}' requires '${depId}' which is not registered`
        ))
        return
      }
      if (this.states.get(depId) !== 'active') {
        await this.activate(dep)
      }
      // If dependency failed, don't continue
      if (this.states.get(depId) !== 'active') {
        this.fail(id, new Error(
          `Plugin '${id}' dependency '${depId}' failed to activate`
        ))
        return
      }
    }

    this.states.set(id, 'activating')

    // Register static manifest contributions before activate() runs.
    // This populates the command palette with labels even before plugin
    // code has executed — enabling lazy activation.
    this.registerManifestContributions(plugin)

    const api = buildPluginAPI(this.registry, { isCore: core, pluginId: id })

    try {
      await plugin.activate(api)
      this.states.set(id, 'active')
      eventBus.emit('plugin:activated', { pluginId: id })
      console.info(`[ExtensionHost] ✓ ${id}`)
    } catch (err) {
      // Clean up any partial registrations before marking as failed
      this.registry.unregisterAll(id)
      this.fail(id, err as Error)
    }
  }

  /** Unload a plugin — cleans up all contributions automatically */
  async unload(id: string) {
    if (this.states.get(id) !== 'active') return

    const plugin = this.plugins.get(id)
    if (!plugin) return

    this.states.set(id, 'deactivating')

    try {
      await plugin.deactivate?.()
    } catch (err) {
      console.error(`[ExtensionHost] deactivate() threw for '${id}':`, err)
    }

    // Sweep all registry contributions this plugin made
    this.registry.unregisterAll(id)

    this.states.set(id, 'inactive')
    eventBus.emit('plugin:deactivated', { pluginId: id })
    console.info(`[ExtensionHost] ✗ ${id} (unloaded)`)
  }

  // ─── Introspection ────────────────────────────────────────────────────────

  getState(id: string): PluginState | undefined {
    return this.states.get(id)
  }

  getError(id: string): Error | undefined {
    return this.errors.get(id)
  }

  isActive(id: string): boolean {
    return this.states.get(id) === 'active'
  }

  listActive(): string[] {
    return [...this.states.entries()]
      .filter(([, s]) => s === 'active')
      .map(([id]) => id)
  }

  listAll(): Array<{ id: string; state: PluginState }> {
    return [...this.states.entries()]
      .map(([id, state]) => ({ id, state }))
  }

  // ─── Private ─────────────────────────────────────────────────────────────

  private fail(id: string, error: Error) {
    this.states.set(id, 'error')
    this.errors.set(id, error)
    eventBus.emit('plugin:error', { pluginId: id, error })
    console.error(`[ExtensionHost] ✗ ${id}: ${error.message}`)
  }

  private registerManifestContributions(plugin: Plugin) {
    const { id, contributes } = plugin.manifest
    if (!contributes) return

    contributes.commands?.forEach(c => {
      this.registry.commands.registerFromManifest(id, c)
      this.registry.track(id, `command:${c.id}`)
    })

    contributes.keybindings?.forEach(k => {
      this.registry.keybindings.registerFromManifest(id, k)
      this.registry.track(id, `keybinding:${id}:${k.command}`)
    })

    // Views and config schema are registered in activate() when the
    // component/schema is available — not from the manifest alone
  }

  /**
   * Topological sort by dependsOn declarations.
   * Core plugins sort before community plugins within each dependency tier.
   */
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

    // Core plugins first (sorted by dependency graph), then community
    plugins.filter(p => p.manifest.core).forEach(visit)
    plugins.filter(p => !p.manifest.core).forEach(visit)

    return result
  }
}
