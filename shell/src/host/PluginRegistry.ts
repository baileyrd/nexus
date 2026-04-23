// src/host/PluginRegistry.ts
// Root registry composed of all sub-registries.
// Maintains an ownership index for automatic cleanup on plugin unload.

import { CommandRegistry } from '../registry/CommandRegistry'
import { ConfigurationRegistry } from '../registry/ConfigurationRegistry'
import { KeybindingRegistry } from '../registry/KeybindingRegistry'
import { StatusBarRegistry } from '../registry/StatusBarRegistry'
import { slotRegistry } from '../registry/SlotRegistry'
import { uriHandlerRegistry } from '../registry/UriHandlerRegistry'

export class PluginRegistry {
  readonly commands    = new CommandRegistry()
  readonly config      = new ConfigurationRegistry()
  readonly keybindings = new KeybindingRegistry()
  readonly statusBar   = new StatusBarRegistry()

  // Internal services registered by core service plugins
  private services = new Map<string, unknown>()

  // Reverse index: pluginId → Set of contribution keys
  // Format: 'type:id' e.g. 'command:myPlugin.doThing', 'slot:myPlugin.view'
  private ownership = new Map<string, Set<string>>()

  // Per-plugin kernel-bus subscription disposers. Tracked separately from
  // `ownership` because subscriptions don't have a stable string id we can
  // round-trip through the parse/switch path in `unregisterAll`. The shape
  // is `pluginId → Set<unsubscribe>`; entries are added by
  // `trackSubscription` (called from `api.kernel.on` in PluginAPI) and
  // drained on plugin unload so dead listeners can't keep receiving events
  // and the Rust-side forwarder tasks get torn down.
  private subscriptions = new Map<string, Set<() => void>>()

  // ─── Ownership tracking ──────────────────────────────────────────────────

  track(pluginId: string, contributionKey: string) {
    if (!this.ownership.has(pluginId)) {
      this.ownership.set(pluginId, new Set())
    }
    this.ownership.get(pluginId)!.add(contributionKey)
  }

  /**
   * Track a kernel-bus unsubscribe so it gets called automatically when the
   * plugin is unloaded. The unsubscribe must be idempotent — it may be
   * invoked by the plugin itself and again from `unregisterAll`.
   */
  trackSubscription(pluginId: string, unsubscribe: () => void) {
    if (!this.subscriptions.has(pluginId)) {
      this.subscriptions.set(pluginId, new Set())
    }
    this.subscriptions.get(pluginId)!.add(unsubscribe)
  }

  /**
   * Remove all contributions made by a plugin.
   * Called automatically by the ExtensionHost on plugin unload.
   */
  unregisterAll(pluginId: string) {
    const keys = this.ownership.get(pluginId)
    if (keys) {
      for (const key of keys) {
        const colonIdx = key.indexOf(':')
        const type = key.slice(0, colonIdx)
        const id   = key.slice(colonIdx + 1)

        switch (type) {
          case 'command':    this.commands.unregister(id);    break
          case 'slot':       slotRegistry.unregister(id);     break
          case 'statusBar':  this.statusBar.unregister(id);   break
          case 'config':     this.config.unregister(id);      break
          case 'keybinding': this.keybindings.unregister(id); break
          default:
            console.warn(`[PluginRegistry] Unknown contribution type: '${type}'`)
        }
      }
      this.ownership.delete(pluginId)
    }

    // Drain kernel-bus subscriptions. Wrapped individually so a single
    // throwing disposer doesn't strand the rest (and doesn't abort the
    // plugin-unload path in ExtensionHost).
    const subs = this.subscriptions.get(pluginId)
    if (subs) {
      for (const unsub of subs) {
        try {
          unsub()
        } catch (err) {
          console.warn(
            `[PluginRegistry] subscription disposer threw for '${pluginId}':`,
            err,
          )
        }
      }
      this.subscriptions.delete(pluginId)
    }

    // Belt-and-braces: sweep any URI handlers still owned by the plugin.
    // The per-handler unsub returned from `api.uri.register` is already
    // tracked via `trackSubscription` and drained above; this catches
    // the edge case where a handler entry survived (e.g. a plugin
    // registered directly against the singleton in a future code path).
    uriHandlerRegistry.unregisterByPlugin(pluginId)
  }

  // ─── Internal service bus (core plugins only) ────────────────────────────

  registerService(name: string, service: unknown) {
    if (this.services.has(name)) {
      console.warn(`[PluginRegistry] Service '${name}' is already registered — overwriting`)
    }
    this.services.set(name, service)
  }

  getService<T>(name: string): T {
    const svc = this.services.get(name)
    if (!svc) {
      throw new Error(`[PluginRegistry] Service '${name}' is not registered`)
    }
    return svc as T
  }

  hasService(name: string): boolean {
    return this.services.has(name)
  }
}
