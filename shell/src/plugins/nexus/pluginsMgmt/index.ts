import { invoke } from '@tauri-apps/api/core'
import { PLUGIN_API_VERSION } from '@nexus/extension-api'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import { PluginsMgmtView } from './PluginsMgmtView'
import {
  usePluginsMgmtStore,
  type BuiltInPluginRow,
  type CommunityPluginRow,
  type PluginRow,
} from './pluginsMgmtStore'
import { setApi } from './pluginsMgmtRuntime'
import { parseManifestCapabilities } from './capabilityInfo'

const VIEW_ID = 'nexus.pluginsMgmt.overlay'

const COMMAND_OPEN = 'nexus.plugins.open'
const COMMAND_CLOSE = 'nexus.plugins.close'
const COMMAND_TOGGLE_COMMUNITY = 'nexus.plugins.toggleCommunity'
const CONTEXT_KEY_VISIBLE = 'nexus.plugins.visible'

const SERVICE_PLUGIN_LIST = 'pluginList'
const SERVICE_COMMUNITY_MANIFESTS = 'communityPluginManifests'

/**
 * Shape registered onto the registry by main.tsx at the end of boot().
 * Kept local to avoid a circular import on main.tsx.
 */
interface RegistryPluginEntry {
  id: string
  name: string
  version: string
  core: boolean
  state: string
  error?: string
  /**
   * Optional capability declaration. Not currently populated by
   * `main.tsx` (the shell-side `PluginManifest` has no capabilities
   * field — see `shell/src/types/plugin.ts`). Read defensively as
   * `unknown` so the row code path is ready the moment that field
   * is wired through, without forcing a churning typecast cascade
   * back through the manifest plumbing today.
   */
  capabilities?: unknown
}

/**
 * Read both internal services and merge them into a single tagged row list.
 * If either service isn't registered yet (e.g. community scan failed) we
 * silently fall back to an empty list for that source.
 */
function readRows(api: PluginAPI): PluginRow[] {
  const internal = api.internal
  if (!internal) return []

  let builtins: BuiltInPluginRow[] = []
  try {
    const raw = internal.getInternalService<RegistryPluginEntry[]>(SERVICE_PLUGIN_LIST)
    builtins = raw.map(
      (p): BuiltInPluginRow => ({
        kind: 'builtin',
        id: p.id,
        name: p.name,
        version: p.version,
        core: p.core,
        state: p.state,
        error: p.error,
        capabilities: parseManifestCapabilities(p.capabilities),
      }),
    )
  } catch (err) {
    console.warn('[nexus.pluginsMgmt] pluginList service missing:', err)
  }

  let community: CommunityPluginRow[] = []
  try {
    const raw = internal.getInternalService<CommunityPluginManifest[]>(
      SERVICE_COMMUNITY_MANIFESTS,
    )
    community = raw.map(
      (m): CommunityPluginRow => {
        // WI-33: flag plugins whose declared apiVersion mismatches the
        // shell constant. Undefined passes through silently — that's the
        // legacy-plugin path (warn-continue handled by the loader).
        const declared = m.apiVersion
        const incompatible =
          typeof declared === 'number' && declared !== PLUGIN_API_VERSION
            ? { requested: declared, supported: PLUGIN_API_VERSION }
            : undefined
        return {
          kind: 'community',
          id: m.id,
          name: m.name,
          version: m.version,
          enabled: m.enabled,
          description: m.description,
          author: m.author,
          dir: m.dir,
          manifestPath: m.manifestPath,
          // CommunityPluginManifest doesn't yet expose `capabilities`
          // (the Rust scanner in `src-tauri/src/lib.rs` doesn't
          // deserialise that field), but read it defensively so we
          // surface declared caps the moment that plumbing arrives.
          capabilities: parseManifestCapabilities(
            (m as unknown as { capabilities?: unknown }).capabilities,
          ),
          incompatible,
        }
      },
    )
  } catch (err) {
    console.warn('[nexus.pluginsMgmt] communityPluginManifests service missing:', err)
  }

  return [...builtins, ...community]
}

export const pluginsMgmtPlugin: Plugin = {
  manifest: {
    // core:true so we can reach api.internal.getInternalService to read the
    // pluginList / communityPluginManifests services registered by main.tsx.
    // Nothing about being `nexus.*`-namespaced bars a plugin from being core;
    // the flag is about internal-API access, not provenance. This is
    // substrate-level infra, same category as configurationService.
    id: 'nexus.pluginsMgmt',
    name: 'Plugins',
    version: '0.1.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: COMMAND_OPEN, title: 'Manage Plugins', category: 'View' },
        { id: COMMAND_CLOSE, title: 'Close Plugins', category: 'View' },
        {
          id: COMMAND_TOGGLE_COMMUNITY,
          title: 'Toggle Community Plugin',
          category: 'View',
        },
      ],
      keybindings: [
        // VSCode's Extensions view shortcut — free in our shell.
        { command: COMMAND_OPEN, key: 'ctrl+shift+x', mac: 'cmd+shift+x' },
        // Gated so palette's own escape binding isn't stolen.
        { command: COMMAND_CLOSE, key: 'escape', when: CONTEXT_KEY_VISIBLE },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True while the plugins modal is open.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    setApi(api)

    // Seed rows once on activate. Refreshed on every open() below so
    // plugin-state transitions since boot show up without a manual action.
    usePluginsMgmtStore.getState().setRows(readRows(api))

    api.commands.register(COMMAND_OPEN, () => {
      // Re-read from the registry so rows are fresh. `pluginList` is
      // a snapshot taken by main.tsx at boot — state transitions after
      // that don't update the array, but in practice nothing mutates
      // plugin state at runtime yet. Re-reading is still cheap and
      // future-proofs the view.
      usePluginsMgmtStore.getState().setRows(readRows(api))
      usePluginsMgmtStore.getState().open()
    })

    api.commands.register(COMMAND_CLOSE, () => {
      usePluginsMgmtStore.getState().close()
    })

    api.commands.register(COMMAND_TOGGLE_COMMUNITY, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        console.warn('[nexus.pluginsMgmt] toggleCommunity requires a pluginId string')
        return
      }

      const state = usePluginsMgmtStore.getState()
      const row = state.rows.find(
        (r): r is CommunityPluginRow => r.kind === 'community' && r.id === pluginId,
      )
      if (!row) {
        console.warn(`[nexus.pluginsMgmt] Unknown community plugin: ${pluginId}`)
        return
      }

      const next = !row.enabled

      // Optimistic flip — roll back if the Tauri call rejects.
      state.updateCommunityEnabled(pluginId, next)

      try {
        // Tauri serializes camelCase JS arg names to snake_case Rust
        // parameter names by default, so `pluginId` → `plugin_id`.
        // The Rust signature is `set_plugin_enabled(plugin_id: String, enabled: bool)`.
        await invoke('set_plugin_enabled', { pluginId, enabled: next })
      } catch (err) {
        console.warn(
          `[nexus.pluginsMgmt] set_plugin_enabled failed for ${pluginId}:`,
          err,
        )
        // Roll back to the previous enabled state.
        usePluginsMgmtStore.getState().updateCommunityEnabled(pluginId, row.enabled)
      }
    })

    // Keep the context key in sync with the store's visibility, same
    // pattern as nexus.commandPalette so `when`-clauses evaluate
    // correctly (our own escape binding depends on it).
    api.context.set(CONTEXT_KEY_VISIBLE, usePluginsMgmtStore.getState().visible)
    usePluginsMgmtStore.subscribe((s, prev) => {
      if (s.visible !== prev.visible) {
        api.context.set(CONTEXT_KEY_VISIBLE, s.visible)
      }
    })

    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: PluginsMgmtView,
      priority: 20,
    })
  },
}
