// src/host/PluginRegistry.ts
// Root registry composed of all sub-registries.
// Maintains an ownership index for automatic cleanup on plugin unload.

import { CommandRegistry } from '../registry/CommandRegistry'
import { ConfigurationRegistry } from '../registry/ConfigurationRegistry'
import { KeybindingRegistry } from '../registry/KeybindingRegistry'
import { SettingsTabRegistry } from '../registry/SettingsTabRegistry'
import { SnippetRegistry } from '../registry/SnippetRegistry'
import { StatusBarRegistry } from '../registry/StatusBarRegistry'
import { slotRegistry } from '../registry/SlotRegistry'
import { uriHandlerRegistry } from '../registry/UriHandlerRegistry'
import { eventBus } from './EventBus'
import { clientLogger } from './clientLogger'
import { workspace } from '../workspace'

export class PluginRegistry {
  readonly commands     = new CommandRegistry()
  readonly config       = new ConfigurationRegistry()
  readonly keybindings  = new KeybindingRegistry()
  readonly settingsTabs = new SettingsTabRegistry()
  readonly snippets     = new SnippetRegistry()
  readonly statusBar    = new StatusBarRegistry()

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

  // Per-plugin viewType ownership. Plugins register workspace leaf
  // creators via `api.viewRegistry.register(type, creator)`; the API
  // wrapper records the owning plugin id here so:
  //   1. On plugin unload, `unregisterAll` calls each disposer to
  //      remove the creator from `viewRegistry`.
  //   2. Any live workspace leaves of that viewType are detached so
  //      disabling a plugin in Settings actually makes its panels go
  //      away — no restart required.
  // Shape: `pluginId → Map<viewType, dispose>`.
  private viewTypeOwnership = new Map<string, Map<string, () => void>>()

  // Per-plugin keybinding override tags (FU-9). Records `commandId →
  // { pluginId, chord }` for every override pushed via the
  // `api.keybindings.setOverride` facade. On plugin deactivate we clear
  // only those whose tag pluginId matches AND whose currently-active
  // override still equals the chord we set — so a Settings-UI override
  // that landed on top of a plugin override is preserved.
  private pluginKeybindingOverrides = new Map<
    string,
    { pluginId: string; chord: string }
  >()

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
   * Record that `pluginId` owns the workspace viewType `viewType`,
   * with `dispose` being the unregister function returned by
   * `viewRegistry.register`. Idempotent: re-registering the same type
   * from the same plugin (e.g. an HMR-driven re-activate) replaces
   * the prior disposer.
   */
  trackViewType(pluginId: string, viewType: string, dispose: () => void) {
    let owned = this.viewTypeOwnership.get(pluginId)
    if (!owned) {
      owned = new Map()
      this.viewTypeOwnership.set(pluginId, owned)
    }
    const prior = owned.get(viewType)
    if (prior && prior !== dispose) {
      try {
        prior()
      } catch (err) {
        clientLogger.warn(
          `[PluginRegistry] prior viewType disposer for '${viewType}' threw:`,
          err,
        )
      }
    }
    owned.set(viewType, dispose)
  }

  /**
   * List of viewTypes registered by a given plugin. Exposed so the
   * tab context menu can resolve a leaf back to its owning plugin
   * ("Disable plugin <name>").
   */
  ownerOfViewType(viewType: string): string | null {
    for (const [pluginId, owned] of this.viewTypeOwnership) {
      if (owned.has(viewType)) return pluginId
    }
    return null
  }

  /**
   * Push a keybinding override on behalf of `pluginId` and tag it so
   * `unregisterAll` can sweep it on plugin unload. The chord is
   * recorded as the registry stored it (post-normalisation), so
   * `unregisterAll` can detect when a Settings-UI override has since
   * replaced ours and skip clearing in that case.
   */
  async setKeybindingOverride(
    pluginId: string,
    commandId: string,
    chord: string,
  ): Promise<void> {
    await this.keybindings.setOverride(commandId, chord)
    const stored = this.keybindings.getOverride(commandId)
    if (stored !== undefined) {
      this.pluginKeybindingOverrides.set(commandId, { pluginId, chord: stored })
    }
  }

  /**
   * Drop the plugin-tagged override for `commandId`. Only clears the
   * registry entry when the tag's pluginId matches the caller — calls
   * from a different plugin (or the Settings UI's untagged path) are
   * a no-op for the tag side; the registry-level clear still runs so
   * the plugin can revert its own contribution.
   */
  async clearKeybindingOverride(
    pluginId: string,
    commandId: string,
  ): Promise<void> {
    const tag = this.pluginKeybindingOverrides.get(commandId)
    if (tag && tag.pluginId === pluginId) {
      this.pluginKeybindingOverrides.delete(commandId)
    }
    await this.keybindings.clearOverride(commandId)
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
          case 'command':     this.commands.unregister(id);     break
          case 'slot':        slotRegistry.unregister(id);      break
          case 'statusBar':   this.statusBar.unregister(id);    break
          case 'config':      this.config.unregister(id);       break
          case 'keybinding':  this.keybindings.unregister(id);  break
          case 'settingsTab': this.settingsTabs.unregister(id); break
          case 'snippet':     this.snippets.unregister(id);     break
          // Activity-bar items live in a Zustand store fed by the
          // event bus; emitting `itemRemoved` is the supported way to
          // drop them. Without this case, disabling a plugin leaves
          // its rail icons visible until reload.
          case 'activityBar': eventBus.emit('activityBar:itemRemoved', { id }); break
          default:
            clientLogger.warn(`[PluginRegistry] Unknown contribution type: '${type}'`)
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
          clientLogger.warn(
            `[PluginRegistry] subscription disposer threw for '${pluginId}':`,
            err,
          )
        }
      }
      this.subscriptions.delete(pluginId)
    }

    // Sweep keybinding overrides this plugin pushed (FU-9). Settings-UI
    // overrides for the same command — recognised by the active override
    // no longer matching the chord we recorded — are left in place.
    for (const [commandId, tag] of [...this.pluginKeybindingOverrides]) {
      if (tag.pluginId !== pluginId) continue
      this.pluginKeybindingOverrides.delete(commandId)
      if (this.keybindings.getOverride(commandId) === tag.chord) {
        // Fire-and-forget: persistence inside `clearOverride` is async
        // but unload is sync; errors are logged by the registry.
        void this.keybindings.clearOverride(commandId)
      }
    }

    // Drain workspace viewTypes this plugin registered. Two-step:
    //   1. Detach every live leaf whose viewType matches — disabling a
    //      plugin in Settings must remove its panels live (no restart).
    //   2. Call each disposer so the creator vanishes from the
    //      `viewRegistry`, preventing a stale persisted leaf from
    //      rehydrating into a half-broken view on the next workspace
    //      open.
    // Order matters: detach first so the leaf's `onClose` runs while the
    // creator (and any closures referencing plugin state) is still alive.
    const ownedViewTypes = this.viewTypeOwnership.get(pluginId)
    if (ownedViewTypes && ownedViewTypes.size > 0) {
      for (const viewType of ownedViewTypes.keys()) {
        // Detach is async; fire-and-forget so unload stays synchronous.
        // Errors are logged inside `detachLeavesByViewType` — they don't
        // abort the sweep of the remaining types.
        void workspace.detachLeavesByViewType(viewType)
      }
      for (const [viewType, dispose] of ownedViewTypes) {
        try {
          dispose()
        } catch (err) {
          clientLogger.warn(
            `[PluginRegistry] viewType disposer for '${viewType}' threw:`,
            err,
          )
        }
      }
      this.viewTypeOwnership.delete(pluginId)
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
      clientLogger.warn(`[PluginRegistry] Service '${name}' is already registered — overwriting`)
    }
    this.services.set(name, service)
  }

  /** Overwrite a service without warning — for intentional refresh after plugin enable/disable. */
  updateService(name: string, service: unknown) {
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
