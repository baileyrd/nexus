import { invoke } from '@tauri-apps/api/core'
import { clientLogger } from '../../../clientLogger'
import type { Capability } from '@nexus/extension-api'
import { PLUGIN_API_VERSION } from '@nexus/extension-api'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import { PluginsMgmtView } from './PluginsMgmtView'
// Importing the lifecycle-status store at module load primes its
// EventBus subscriptions so the modal's per-row state badges reflect
// every `plugin:activated` / `plugin:deactivated` / `plugin:error`
// from the very first plugin onwards. The priming used to live in
// `nexus.extensionsTab/index.ts`; this plugin is default-on and
// loaded at boot, so it's a safe home for the import once the
// Extensions tab is retired.
import '../../../stores/pluginsStatusStore'
import {
  usePluginsMgmtStore,
  type AvailablePluginRow,
  type BuiltInPluginRow,
  type CommunityPluginRow,
  type PluginRow,
} from './pluginsMgmtStore'
import { DEFAULT_OFF_PLUGINS } from '../../catalog'
import {
  PLUGIN_LIST_CHANGED_EVENT,
  enableBuiltinPlugin,
  disableBuiltinPlugin,
} from '../../../host/pluginActivation'
import { setApi } from './pluginsMgmtRuntime'
import { CAPABILITY_INFO, parseManifestCapabilities } from './capabilityInfo'
import {
  requestModalConsent,
  kernelStringsToCaps,
  type PriorGrant,
} from '../../core/capabilityPrompt'
import { getRegistry } from '../../../host/shellRegistry'

const VIEW_ID = 'nexus.pluginsMgmt.overlay'

const COMMAND_OPEN = 'nexus.plugins.open'
const COMMAND_CLOSE = 'nexus.plugins.close'
const COMMAND_TOGGLE_COMMUNITY = 'nexus.plugins.toggleCommunity'
const COMMAND_REVIEW_CAPS = 'nexus.plugins.reviewCapabilities'
// Enable or disable an optional (default-off) built-in plugin against
// the live ExtensionHost — no reload required. Persistence is handled
// by `enableBuiltinPlugin` / `disableBuiltinPlugin` in pluginActivation.
const COMMAND_ENABLE_BUILTIN = 'nexus.plugins.enableBuiltin'
const COMMAND_DISABLE_BUILTIN = 'nexus.plugins.disableBuiltin'
/** Open the Settings panel to the given plugin's settings surface. */
const COMMAND_CONFIGURE = 'nexus.plugins.configure'
const CONTEXT_KEY_VISIBLE = 'nexus.plugins.visible'

/**
 * Resolve the rail-entry id to deep-link to when the user clicks
 * Configure on a plugin row. Returns the contributed Settings tab's
 * id when one is registered (richer UX than the auto-form), otherwise
 * the plugin id itself — every plugin that registers a configuration
 * schema gets a rail entry keyed on its id (see
 * SettingsPanelView.tsx's per-plugin rail loop). Returns `null` when
 * the plugin has neither surface, so the modal can hide the button.
 */
function resolveSettingsTarget(pluginId: string): string | null {
  const reg = getRegistry()
  if (!reg) return null
  const tab = reg.settingsTabs.all().find((t) => t.pluginId === pluginId)
  if (tab) return tab.id
  const section = reg.config.all().find((s) => s.pluginId === pluginId)
  if (section) return section.pluginId
  return null
}

const SERVICE_PLUGIN_LIST = 'pluginList'
const SERVICE_COMMUNITY_MANIFESTS = 'communityPluginManifests'
const SERVICE_COMMUNITY_DENIED = 'communityPluginDenied'
const SERVICE_AVAILABLE_PLUGINS = 'availablePlugins'

/** Shape registered by main.tsx for each dormant default-off plugin. */
interface AvailablePluginEntry {
  id: string
  name: string
  version: string
  core: boolean
  description?: string
}

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
  description?: string
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
 * Cache of per-plugin HIGH-risk grant counts. Seeded on plugin open
 * (async Tauri call) and consulted synchronously by `readRows`. A stale
 * cache renders a stale subtitle until the next `refreshGrants` call —
 * acceptable trade-off vs. making row reads async.
 */
const grantCache = new Map<string, { granted: number; version: string }>()

async function refreshGrants(manifests: CommunityPluginManifest[]) {
  const dirs: Record<string, string> = {}
  for (const m of manifests) dirs[m.id] = m.dir
  try {
    const raw = await invoke<Record<string, PriorGrant>>(
      'get_plugin_granted_capabilities',
      { pluginDirs: dirs },
    )
    grantCache.clear()
    for (const [pluginId, entry] of Object.entries(raw)) {
      const caps = kernelStringsToCaps(entry.capabilities ?? [])
      grantCache.set(pluginId, {
        granted: caps.length,
        version: entry.version,
      })
    }
  } catch (err) {
    clientLogger.warn('[nexus.pluginsMgmt] refreshGrants failed:', err)
  }
}

/**
 * Read both internal services and merge them into a single tagged row list.
 * If either service isn't registered yet (e.g. community scan failed) we
 * silently fall back to an empty list for that source.
 */
function readRows(api: PluginAPI): PluginRow[] {
  const internal = api.internal
  if (!internal) return []

  // Optional built-ins = anything in DEFAULT_OFF_PLUGINS. These are the
  // only built-ins safe to disable mid-session (disableBuiltinPlugin
  // refuses to touch anything else).
  const optionalIds = new Set(DEFAULT_OFF_PLUGINS.map((e) => e.id))

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
        description: p.description,
        capabilities: parseManifestCapabilities(p.capabilities),
        canConfigure: resolveSettingsTarget(p.id) !== null,
        optional: optionalIds.has(p.id),
      }),
    )
  } catch (err) {
    clientLogger.warn('[nexus.pluginsMgmt] pluginList service missing:', err)
  }

  let community: CommunityPluginRow[] = []
  let deniedSet: Set<string> = new Set()
  try {
    deniedSet = internal.getInternalService<Set<string>>(SERVICE_COMMUNITY_DENIED)
  } catch {
    // No prompt has run yet (or no denials) — treat as empty.
  }
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
        // WI-31: the Rust scanner now forwards `capabilities` from
        // plugin.json. `parseManifestCapabilities` filters to known
        // `Capability` variants and distinguishes "absent" (null) from
        // "declared empty" ([]) — see capabilityInfo.ts.
        const parsedCaps = parseManifestCapabilities(m.capabilities)
        const declaredHighRiskCount =
          parsedCaps === null
            ? null
            : parsedCaps.filter(
                (c) => CAPABILITY_INFO[c]?.risk === 'high',
              ).length
        const cached = grantCache.get(m.id)
        const grantSummary = {
          declared: declaredHighRiskCount,
          granted: cached?.granted ?? 0,
          denied: deniedSet.has(m.id),
        }
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
          capabilities: parsedCaps,
          incompatible,
          grantSummary,
          pluginDir: m.dir,
          canConfigure: resolveSettingsTarget(m.id) !== null,
        }
      },
    )
  } catch (err) {
    clientLogger.warn('[nexus.pluginsMgmt] communityPluginManifests service missing:', err)
  }

  // WI-43: dormant default-off catalog entries — shipped but not loaded
  // this session. Rendered in a dedicated "Available (disabled)" section
  // with a one-click Enable button.
  let available: AvailablePluginRow[] = []
  try {
    const raw = internal.getInternalService<AvailablePluginEntry[]>(
      SERVICE_AVAILABLE_PLUGINS,
    )
    available = raw.map(
      (p): AvailablePluginRow => ({
        kind: 'available',
        id: p.id,
        name: p.name,
        version: p.version,
        core: p.core,
        description: p.description,
      }),
    )
  } catch {
    // Service not registered (older boot path) — render without an
    // Available section rather than erroring the whole modal.
  }

  return [...builtins, ...community, ...available]
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
    popoutCompatible: false,
    contributes: {
      commands: [
        { id: COMMAND_OPEN, title: 'Manage Plugins', category: 'View' },
        { id: COMMAND_CLOSE, title: 'Close Plugins', category: 'View' },
        {
          id: COMMAND_TOGGLE_COMMUNITY,
          title: 'Toggle Community Plugin',
          category: 'View',
        },
        {
          id: COMMAND_REVIEW_CAPS,
          title: 'Review Plugin Capabilities',
          category: 'View',
        },
        {
          id: COMMAND_ENABLE_BUILTIN,
          title: 'Enable Built-in Plugin',
          category: 'View',
        },
        {
          id: COMMAND_DISABLE_BUILTIN,
          title: 'Disable Built-in Plugin',
          category: 'View',
        },
        {
          id: COMMAND_CONFIGURE,
          title: 'Configure Plugin',
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

    // Seed rows whenever the plugin list changes. main.tsx fires
    // PLUGIN_LIST_CHANGED_EVENT once after boot finishes registering
    // `pluginList` / `communityPluginManifests` (which happens *after*
    // every plugin's activate() has run); refreshPluginServices emits
    // it again when mid-session enable/disable mutates the lists.
    // Reading at activate-time would race main.tsx and warn on every
    // boot — defer to the event instead.
    api.events.on(PLUGIN_LIST_CHANGED_EVENT, () => {
      usePluginsMgmtStore.getState().setRows(readRows(api))
    })

    api.commands.register(COMMAND_OPEN, async () => {
      // WI-31: refresh grant cache before we render rows so the
      // "Granted N/M" subtitle reflects the current state of each
      // plugin's `granted_caps.json` (a prior consent-prompt run or
      // manual edit could have changed it since boot).
      try {
        const manifests = api.internal!.getInternalService<
          CommunityPluginManifest[]
        >(SERVICE_COMMUNITY_MANIFESTS)
        await refreshGrants(manifests)
      } catch {
        // Service missing → no grants to refresh.
      }
      // Re-read from the registry so rows are fresh. `pluginList` is
      // a snapshot taken by main.tsx at boot — state transitions after
      // that don't update the array, but in practice nothing mutates
      // plugin state at runtime yet. Re-reading is still cheap and
      // future-proofs the view.
      usePluginsMgmtStore.getState().setRows(readRows(api))
      usePluginsMgmtStore.getState().open()
    })

    api.commands.register(COMMAND_REVIEW_CAPS, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        clientLogger.warn('[nexus.pluginsMgmt] reviewCapabilities requires a pluginId')
        return
      }
      const rows = usePluginsMgmtStore.getState().rows
      const row = rows.find(
        (r): r is CommunityPluginRow => r.kind === 'community' && r.id === pluginId,
      )
      if (!row || !row.capabilities || row.capabilities.length === 0) return

      // Read the live prior grant from disk (the cache may be stale).
      let prior: Capability[] = []
      try {
        const raw = await invoke<Record<string, PriorGrant>>(
          'get_plugin_granted_capabilities',
          { pluginDirs: { [pluginId]: row.dir } },
        )
        prior = kernelStringsToCaps(raw[pluginId]?.capabilities ?? [])
      } catch (err) {
        clientLogger.warn('[nexus.pluginsMgmt] get_granted failed:', err)
      }

      const result = await requestModalConsent({
        pluginId: row.id,
        pluginName: row.name,
        version: row.version,
        pluginDir: row.dir,
        caps: row.capabilities,
        previouslyGranted: prior,
        // Manual review-after-the-fact — use capability-change copy.
        reason: 'capability-change',
      })

      // BL-096 follow-up — applyCapabilityChange persists the new
      // set AND issues `revoke_plugin_capability` for any cap that
      // was previously granted but is no longer in `result`. Live
      // revoke means the running plugin loses access immediately;
      // pre-fix the disk write only took effect at next boot.
      const { applyCapabilityChange } = await import(
        '../../core/capabilityPrompt'
      )
      try {
        const { revokeErrors } = await applyCapabilityChange(
          { invoke: invoke as never },
          {
            pluginId,
            pluginDir: row.dir,
            version: row.version,
            prior,
            next: result,
          },
        )
        for (const { capability, error } of revokeErrors) {
          clientLogger.warn(
            `[nexus.pluginsMgmt] live-revoke failed for ${pluginId} ${capability}:`,
            error,
          )
        }
      } catch (err) {
        clientLogger.warn(
          `[nexus.pluginsMgmt] set_granted failed for ${pluginId}:`,
          err,
        )
      }

      // Refresh so the subtitle reflects the new grant count.
      try {
        const manifests = api.internal!.getInternalService<
          CommunityPluginManifest[]
        >(SERVICE_COMMUNITY_MANIFESTS)
        await refreshGrants(manifests)
        usePluginsMgmtStore.getState().setRows(readRows(api))
      } catch {
        // best-effort
      }
    })

    api.commands.register(COMMAND_CLOSE, () => {
      usePluginsMgmtStore.getState().close()
    })

    // Deep-link from a plugin row to its Settings surface. Closes the
    // modal, opens the settings panel, and routes the rail to either
    // the plugin's contributed Settings tab (when one is registered)
    // or its configuration-schema section (keyed on the plugin id).
    // Re-resolves at click time so a tab registered after the modal
    // opened is honoured without a row re-seed.
    api.commands.register(COMMAND_CONFIGURE, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        clientLogger.warn('[nexus.pluginsMgmt] configure requires a pluginId')
        return
      }
      const targetId = resolveSettingsTarget(pluginId)
      if (!targetId) {
        api.notifications.show({
          type: 'info',
          message: `${pluginId} has no settings surface.`,
        })
        return
      }
      usePluginsMgmtStore.getState().close()
      await api.commands.execute('workbench.action.openSettings')
      // `settingsActiveTab` is the canonical deep-link context key the
      // settings panel reads on every open — same path used by
      // `workbench.action.openKeybindings` in core/settings/index.ts.
      api.context.set('settingsActiveTab', targetId)
    })

    // Enable a default-off built-in plugin against the live host —
    // registers, activates, and persists in one shot. No reload
    // required. `enableBuiltinPlugin` fires PLUGIN_LIST_CHANGED_EVENT
    // after refreshing services, which re-runs `readRows` and drops
    // the row out of the Available section automatically.
    api.commands.register(COMMAND_ENABLE_BUILTIN, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        clientLogger.warn('[nexus.pluginsMgmt] enableBuiltin requires a pluginId')
        return
      }
      const result = await enableBuiltinPlugin(pluginId)
      if (!result.ok) {
        api.notifications.show({
          type: 'error',
          message: `Failed to enable ${pluginId}: ${result.error}`,
        })
        return
      }
      api.notifications.show({
        type: 'success',
        message: `${pluginId} enabled.`,
      })
    })

    // Disable a default-off built-in plugin against the live host.
    // `disableBuiltinPlugin` refuses to touch required built-ins and
    // returns an error string; surface it as a toast rather than
    // silently no-op'ing so the user understands why the toggle
    // didn't take.
    api.commands.register(COMMAND_DISABLE_BUILTIN, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        clientLogger.warn('[nexus.pluginsMgmt] disableBuiltin requires a pluginId')
        return
      }
      const result = await disableBuiltinPlugin(pluginId)
      if (!result.ok) {
        api.notifications.show({
          type: 'error',
          message: `Failed to disable ${pluginId}: ${result.error}`,
        })
        return
      }
      api.notifications.show({
        type: 'info',
        message: `${pluginId} disabled.`,
      })
    })

    api.commands.register(COMMAND_TOGGLE_COMMUNITY, async (...args: unknown[]) => {
      const pluginId = args[0]
      if (typeof pluginId !== 'string') {
        clientLogger.warn('[nexus.pluginsMgmt] toggleCommunity requires a pluginId string')
        return
      }

      const state = usePluginsMgmtStore.getState()
      const row = state.rows.find(
        (r): r is CommunityPluginRow => r.kind === 'community' && r.id === pluginId,
      )
      if (!row) {
        clientLogger.warn(`[nexus.pluginsMgmt] Unknown community plugin: ${pluginId}`)
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
        clientLogger.warn(
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
