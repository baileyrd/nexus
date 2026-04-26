// src/host/pluginActivation.ts
// Mid-session enable for default-off built-in plugins. Removes the
// "restart required" UX in Settings → Plugins by registering +
// activating the plugin against the live ExtensionHost, persisting
// the id into `plugins.enabled` so the choice survives reboot, and
// refreshing the `pluginList` / `availablePlugins` services so the
// Settings UI re-reads its rows.

import type { Plugin } from '../types/plugin'
import { getRegistry } from './shellRegistry'
import { getHost } from './shellHost'
import {
  ALL_PLUGINS,
  DEFAULT_OFF_PLUGINS,
  PLUGINS_ENABLED_CONFIG_KEY,
} from '../plugins/catalog'
import { useConfigStore } from '../stores/configStore'
import { eventBus } from './EventBus'

/**
 * Fired after `refreshPluginServices` re-publishes `pluginList` /
 * `availablePlugins`. Hooks subscribe to this in preference to
 * `plugin:activated` / `plugin:deactivated` so the read happens after
 * the services have been updated, not before.
 */
export const PLUGIN_LIST_CHANGED_EVENT = 'shell:pluginListChanged'

export type EnableResult =
  | { ok: true }
  | { ok: false; error: string }

/**
 * Register + activate a default-off built-in mid-session.
 *
 * Walks `dependsOn` and pulls in any default-off deps that aren't yet
 * registered with the host (default-on deps are already active and
 * `host.activate` short-circuits on them). Hands the resulting set to
 * `host.loadAll`, which runs the same two-pass register-then-activate
 * pipeline `boot()` uses, so manifest contributions, activation
 * triggers, and dependency ordering all match the cold-start path.
 */
export async function enableBuiltinPlugin(pluginId: string): Promise<EnableResult> {
  const host = getHost()
  const reg = getRegistry()
  if (!host || !reg) {
    return { ok: false, error: 'Shell is not booted yet' }
  }

  const plugin = DEFAULT_OFF_PLUGINS.find((p) => p.manifest.id === pluginId)
  if (!plugin) {
    return { ok: false, error: `Unknown built-in plugin: ${pluginId}` }
  }
  if (host.isActive(pluginId)) {
    // Already running — still make sure the persisted set includes it
    // so a future boot keeps it enabled.
    persistEnabled(pluginId)
    refreshPluginServices()
    return { ok: true }
  }

  // Build a register-set: this plugin plus any default-off deps not yet
  // registered. Default-on deps are skipped (they're already active and
  // `host.activate` will short-circuit them).
  const queue: Plugin[] = []
  const seen = new Set<string>()
  const visit = (p: Plugin): EnableResult | undefined => {
    if (seen.has(p.manifest.id)) return undefined
    seen.add(p.manifest.id)
    for (const depId of p.manifest.dependsOn ?? []) {
      if (host.isActive(depId)) continue
      const dep = ALL_PLUGINS.find((x) => x.manifest.id === depId)
      if (!dep) {
        return {
          ok: false,
          error: `'${p.manifest.id}' depends on '${depId}' which is not in the catalog`,
        }
      }
      const sub = visit(dep)
      if (sub && !sub.ok) return sub
    }
    queue.push(p)
    return undefined
  }
  const visitErr = visit(plugin)
  if (visitErr && !visitErr.ok) return visitErr

  try {
    await host.loadAll(queue)
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    }
  }

  // A lazy plugin (activationEvents like `onCommand:` / `onView:`)
  // legitimately stays in 'registered' after loadAll — it will
  // activate when its trigger fires. Only `error` is a real failure.
  const state = host.getState(pluginId)
  if (state === 'error') {
    const e = host.getError(pluginId)
    return { ok: false, error: e?.message ?? 'Activation failed' }
  }

  persistEnabled(pluginId)
  refreshPluginServices()

  // UX bonus: if the plugin is lazy with an `onCommand:` activation
  // trigger that looks like a "focus / show / open" command, run it
  // now so enabling the plugin actually surfaces its UI. Without
  // this, lazy plugins (Bookmarks, Tags, etc.) silently register and
  // the user has to dig through the command palette to see anything.
  if (state !== 'active') {
    const events = plugin.manifest.activationEvents ?? []
    const focusCmd = events
      .filter((e) => e.startsWith('onCommand:'))
      .map((e) => e.slice('onCommand:'.length))
      .find((c) => /\.(focus|show|open|reveal)$/i.test(c))
    if (focusCmd) {
      try {
        await reg.commands.execute(focusCmd)
      } catch {
        // Best-effort — the plugin is registered either way; user can
        // still invoke the command from the palette.
      }
    }
  }

  return { ok: true }
}

function persistEnabled(pluginId: string) {
  const cfg = useConfigStore.getState()
  const current =
    (cfg.values[PLUGINS_ENABLED_CONFIG_KEY] as string[] | undefined) ?? []
  if (current.includes(pluginId)) return
  cfg.set(PLUGINS_ENABLED_CONFIG_KEY, [...current, pluginId])
}

function persistDisabled(pluginId: string) {
  const cfg = useConfigStore.getState()
  const current =
    (cfg.values[PLUGINS_ENABLED_CONFIG_KEY] as string[] | undefined) ?? []
  if (!current.includes(pluginId)) return
  cfg.set(
    PLUGINS_ENABLED_CONFIG_KEY,
    current.filter((id) => id !== pluginId),
  )
}

/**
 * Mid-session disable for a default-off built-in. Calls `host.unload`,
 * which fires the plugin's `deactivate()` and sweeps every contribution
 * (commands, keybindings, activity-bar items, views) it registered.
 * The plugin's id is also removed from the persisted `plugins.enabled`
 * list so it stays off across reboots.
 *
 * Default-on plugins (`DEFAULT_ON_PLUGINS`) are not eligible — they're
 * load-bearing services. Disabling them mid-session would leave the
 * shell in a half-broken state.
 */
export async function disableBuiltinPlugin(pluginId: string): Promise<EnableResult> {
  const host = getHost()
  if (!host) return { ok: false, error: 'Shell is not booted yet' }

  const isOptional = DEFAULT_OFF_PLUGINS.some(
    (p) => p.manifest.id === pluginId,
  )
  if (!isOptional) {
    return {
      ok: false,
      error: `'${pluginId}' is a required built-in and can't be disabled`,
    }
  }

  try {
    await host.unload(pluginId)
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    }
  }

  persistDisabled(pluginId)
  refreshPluginServices()
  return { ok: true }
}

/**
 * Re-publish the two services Settings → Plugins reads so newly-enabled
 * built-ins move from "Available (disabled)" to "Core plugins" without
 * a reload. `registerService` overwrites in-place; subscribers observe
 * the new values on their next `getService` call (the Settings hooks
 * re-read on `shellReady` flips, which we trigger by leaving the rail
 * and coming back — covered by the manual close/reopen flow today).
 */
function refreshPluginServices() {
  const host = getHost()
  const reg = getRegistry()
  if (!host || !reg) return

  const all = host.listAll()
  const manifestById = new Map(
    ALL_PLUGINS.map((p) => [p.manifest.id, p.manifest]),
  )
  // `inactive` means the plugin was unloaded (either deactivated or its
  // lazy pre-registration was swept by `unload`). It's still in the
  // host's state map so listAll can replay history, but for the
  // Settings UI it's "off" — must move to `availablePlugins`, not stay
  // under "Core plugins" where the toggle would be stuck on.
  const loaded = all.filter(({ state }) => state !== 'inactive')
  const pluginList = loaded.map(({ id, state }) => {
    const m = manifestById.get(id)
    return {
      id,
      name: m?.name ?? id,
      version: m?.version ?? '?',
      core: m?.core ?? false,
      state,
      error: host.getError(id)?.message,
    }
  })
  reg.registerService('pluginList', pluginList)

  const enabled = new Set(loaded.map(({ id }) => id))
  const available = DEFAULT_OFF_PLUGINS
    .filter((p) => !enabled.has(p.manifest.id))
    .map((p) => ({
      id: p.manifest.id,
      name: p.manifest.name,
      version: p.manifest.version,
      core: p.manifest.core,
    }))
  reg.registerService('availablePlugins', available)
  eventBus.emit(PLUGIN_LIST_CHANGED_EVENT, null)
}
