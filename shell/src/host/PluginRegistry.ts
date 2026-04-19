// src/host/PluginRegistry.ts
// Root registry composed of all sub-registries.
// Maintains an ownership index for automatic cleanup on plugin unload.

import { CommandRegistry } from '../registry/CommandRegistry'
import { ConfigurationRegistry } from '../registry/ConfigurationRegistry'
import { KeybindingRegistry } from '../registry/KeybindingRegistry'
import { StatusBarRegistry } from '../registry/StatusBarRegistry'
import { slotRegistry } from '../registry/SlotRegistry'

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

  // ─── Ownership tracking ──────────────────────────────────────────────────

  track(pluginId: string, contributionKey: string) {
    if (!this.ownership.has(pluginId)) {
      this.ownership.set(pluginId, new Set())
    }
    this.ownership.get(pluginId)!.add(contributionKey)
  }

  /**
   * Remove all contributions made by a plugin.
   * Called automatically by the ExtensionHost on plugin unload.
   */
  unregisterAll(pluginId: string) {
    const keys = this.ownership.get(pluginId)
    if (!keys) return

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
