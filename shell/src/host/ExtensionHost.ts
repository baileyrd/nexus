// src/host/ExtensionHost.ts
// Manages the full plugin lifecycle: loading, activating, unloading.
// The only place that calls plugin code.

import type { Plugin } from '../types/plugin'
import { PluginRegistry } from './PluginRegistry'
import { buildPluginAPI } from './PluginAPI'
import { eventBus } from './EventBus'
import { activationTriggers } from './ActivationTriggers'

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
  // Tracks which plugins have already had their manifest contributions
  // (commands + keybindings) installed, so the eager-activation path
  // doesn't re-register on top of the lazy pre-registration done in
  // Pass 1 of `loadAll`. KeybindingRegistry.registerFromManifest is not
  // idempotent (it pushes unconditionally) so a second call would
  // duplicate the binding.
  private contribsRegistered = new Set<string>()

  constructor(registry: PluginRegistry) {
    this.registry = registry
    // Wire the activation-trigger singleton so trigger sources
    // (CommandRegistry.execute, Leaf.setViewState, UriHandlerRegistry.dispatch)
    // can wake deferred plugins on demand. See ActivationTriggers.ts.
    activationTriggers.setActivator(async (pluginId) => {
      const plugin = this.plugins.get(pluginId)
      if (!plugin) {
        console.warn(`[ExtensionHost] activator: unknown plugin '${pluginId}'`)
        return
      }
      await this.activate(plugin)
    })
  }

  // ─── Public API ──────────────────────────────────────────────────────────

  /**
   * Two-pass loader (WI-19).
   *
   * Pass 1 — register manifests + parse `activationEvents`.
   *   - `onStartup` (or empty / `*`) → queued for eager activation.
   *   - `onView:X`, `onCommand:X`, `onUri:X`, `onLanguage:X` → recorded
   *     in `activationTriggers`; the plugin stays in `registered` until
   *     a trigger source fires for one of its keys.
   *
   * Pass 2 — activate the eager set in dependency order.
   *
   * Dep-resolution caveat: a lazy plugin that is `dependsOn`'d by an
   * eager one is implicitly *promoted* to eager because `activate()`
   * recursively pulls in dependencies. That's the documented escape
   * hatch (the dep-graph wins over laziness — see PHASE-2 plan §5.4).
   */
  async loadAll(plugins: Plugin[]) {
    // ── Pass 1: register everything, classify eager vs lazy ──
    const eager: Plugin[] = []
    for (const plugin of plugins) {
      const id = plugin.manifest.id
      this.plugins.set(id, plugin)
      this.states.set(id, 'registered')

      const events = plugin.manifest.activationEvents ?? []
      const isEager =
        events.length === 0 ||
        events.includes('onStartup') ||
        events.includes('*')

      if (isEager) {
        eager.push(plugin)
      } else {
        // For lazy plugins, pre-register manifest contributions (commands
        // + keybindings) so the command palette / keybinding system can
        // surface them without forcing activation. activate() is
        // idempotent on these calls — `registerFromManifest` no-ops when
        // the entry already exists, and the keybinding registry tolerates
        // duplicate registrations from the same (plugin, command) pair.
        // Without this step, a lazy plugin's `onCommand:` trigger would
        // never fire because the command label wouldn't be in the palette
        // for the user to invoke. See registerManifestContributions().
        this.registerManifestContributions(plugin)
      }

      // A plugin can mix eager + trigger events (rare but legal — e.g.
      // a service that wants to also wake on a specific deep link).
      // Record every non-eager event so the trigger maps stay accurate
      // even when the plugin is going to load eagerly anyway. The
      // eviction step on activation cleans up the redundant entries.
      for (const ev of events) {
        if (ev === 'onStartup' || ev === '*') continue
        activationTriggers.register(ev, id)
      }
    }

    // ── Pass 2: dep-ordered activation of eager plugins only ──
    const ordered = this.resolveDependencyOrder(eager)
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
      // Drop any deferred-trigger entries this plugin still owned — it's
      // active now, subsequent fires of the same trigger should be no-ops
      // rather than re-attempts.
      activationTriggers.evict(id)
      eventBus.emit('plugin:activated', { pluginId: id })
      console.info(`[ExtensionHost] ✓ ${id}`)
    } catch (err) {
      // Clean up any partial registrations before marking as failed
      this.registry.unregisterAll(id)
      this.contribsRegistered.delete(id)
      // Drop triggers too — re-firing won't help a plugin in `error` state
      // (the activate() guard returns early on subsequent attempts).
      activationTriggers.evict(id)
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
    // Forget the dedupe marker so a future re-activation re-registers
    // manifest contributions.
    this.contribsRegistered.delete(id)

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
    // Idempotent guard: in lazy mode we register manifest contributions
    // up front in `loadAll` Pass 1 so the command palette / keybinding
    // matcher can see entries before activation. The eager-activation
    // path then runs activate() which would re-enter here; without this
    // skip, KeybindingRegistry.registerFromManifest would duplicate the
    // binding (it pushes unconditionally).
    if (this.contribsRegistered.has(id)) return
    this.contribsRegistered.add(id)

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
