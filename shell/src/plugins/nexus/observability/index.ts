// BL-054 Phase 4 — Observability plugin.
//
// Three internal tabs over a single sidebar leaf:
//   - Usage     : per-surface + per-day rollup of `com.nexus.ai::activity_list`
//   - Automation: foundation workflows (cron / file_event triggers) from
//                 `com.nexus.workflow::list`, with manual "Run now"
//   - Vault feed: file activity from `com.nexus.activity.appended`
//                 filtered to raw/, wiki/, output/
//
// Default-off in the catalog — surface targets the OS-template forge
// flow and would clutter the activity bar for plain forges.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { clientLogger } from '../../../clientLogger'
import type { ActivityEntry } from '../activityTimeline/activityTimelineStore'
import { OsObservabilityView } from './OsObservabilityView'
import { osObservabilityPaneViewCreator } from './OsObservabilityPaneView'
import { useObservabilityStore, type AutomationEntry, type VaultFeedEntry } from './observabilityStore'
import { aggregateUsage } from './usageAggregate'

const VIEW_ID = 'nexus.osObservability.view'
const COMMAND_REFRESH = 'nexus.osObservability.refresh'
const COMMAND_SHOW = 'nexus.osObservability.show'

const AI_PLUGIN_ID = 'com.nexus.ai'
const WORKFLOW_PLUGIN_ID = 'com.nexus.workflow'

const TOPIC_ACTIVITY_APPENDED = 'com.nexus.activity.appended'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

/** The forge roots vault-feed cares about. Anything under one of these
 *  prefixes (or starting with one of these slugs) is routed to the
 *  feed; other file-activity entries are dropped on the floor. */
const VAULT_PATH_PREFIXES = ['raw/', 'wiki/', 'output/', 'projects/', 'ops/']

interface ActivityListResult { entries: ActivityEntry[] }

export const osObservabilityPlugin: Plugin = {
  manifest: {
    id: 'nexus.osObservability',
    name: 'Observability',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace'],
  },

  async activate(api: PluginAPI) {
    const refreshUsage = async () => {
      let available = false
      try { available = await api.kernel.available() } catch { available = false }
      if (!available) return
      useObservabilityStore.getState().setUsageLoading(true)
      try {
        const resp = await api.kernel.invoke<ActivityListResult>(
          AI_PLUGIN_ID,
          'activity_list',
          {},
        )
        const entries = Array.isArray(resp?.entries) ? resp.entries : []
        const rollup = aggregateUsage(entries)
        useObservabilityStore.getState().setUsageData(entries, rollup)
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        useObservabilityStore.getState().setUsageError(msg)
        useObservabilityStore.getState().setUsageLoading(false)
      }
    }

    const refreshAutomation = async () => {
      let available = false
      try { available = await api.kernel.available() } catch { available = false }
      if (!available) return
      useObservabilityStore.getState().setAutomationLoading(true)
      try {
        const raw = await api.kernel.invoke<unknown>(WORKFLOW_PLUGIN_ID, 'list', {})
        const list = Array.isArray(raw) ? raw : []
        const decoded: AutomationEntry[] = []
        for (const item of list) {
          if (!item || typeof item !== 'object') continue
          const wf = item as Record<string, unknown>
          const name = typeof wf.name === 'string' ? wf.name : null
          if (!name) continue
          const trigger = wf.trigger as Record<string, unknown> | undefined
          const triggerType =
            (trigger && typeof trigger.type === 'string' ? trigger.type : null)
            ?? (typeof wf.trigger === 'string' ? (wf.trigger as string) : 'manual')
          decoded.push({
            name,
            description: typeof wf.description === 'string' ? wf.description : '',
            triggerType,
            stepCount: Array.isArray(wf.steps) ? wf.steps.length : 0,
          })
        }
        decoded.sort((a, b) => a.name.localeCompare(b.name))
        useObservabilityStore.getState().setAutomations(decoded)
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        useObservabilityStore.getState().setAutomationError(msg)
        useObservabilityStore.getState().setAutomationLoading(false)
      }
    }

    const runWorkflow = async (name: string) => {
      try {
        await api.kernel.invoke(WORKFLOW_PLUGIN_ID, 'run', { name })
        api.notifications.show({
          type: 'info',
          message: `Workflow "${name}" started.`,
        })
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          type: 'error',
          message: `Workflow "${name}" failed to start: ${msg}`,
        })
      }
    }

    const renderView = () =>
      createElement(OsObservabilityView, {
        onRefreshUsage: () => void refreshUsage(),
        onRefreshAutomation: () => void refreshAutomation(),
        onRunWorkflow: (name: string) => { void runWorkflow(name) },
      })

    viewRegistry.register('osObservability', osObservabilityPaneViewCreator(renderView))

    api.activityBar.addItem({
      id: 'nexus.osObservability.activityItem',
      icon: '',
      iconName: 'activity',
      title: 'Observability',
      viewId: VIEW_ID,
      priority: 47,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refreshUsage()
      void refreshAutomation()
    })
    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType('osObservability', 'main')
      workspace.revealLeaf(leaf)
    })

    // ── Vault-feed bus subscription ───────────────────────────────────
    const kernelUnsubs: Array<() => void> = []
    const subscribeBus = async () => {
      if (kernelUnsubs.length > 0) return
      try {
        const unsub = await api.kernel.on<ActivityEntry>(
          TOPIC_ACTIVITY_APPENDED,
          (_topic, payload) => {
            if (!payload || typeof payload !== 'object') return
            if (payload.surface !== 'file') return
            const files = Array.isArray(payload.files) ? payload.files : []
            if (!files.some((f) => isVaultPath(f))) return
            const entry: VaultFeedEntry = {
              id: payload.id,
              timestamp: payload.timestamp,
              prompt: payload.prompt,
              files,
            }
            useObservabilityStore.getState().prependVault(entry)
          },
        )
        kernelUnsubs.push(unsub)
      } catch (err) {
        clientLogger.warn('[nexus.osObservability] bus subscribe failed:', err)
      }
    }
    const unsubscribeBus = () => {
      while (kernelUnsubs.length > 0) {
        const unsub = kernelUnsubs.pop()
        if (!unsub) continue
        try { unsub() } catch (err) {
          clientLogger.warn('[nexus.osObservability] unsubscribe failed:', err)
        }
      }
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refreshUsage()
      void refreshAutomation()
      void subscribeBus()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      unsubscribeBus()
      useObservabilityStore.getState().reset()
    })
    if (await api.kernel.available()) {
      void refreshUsage()
      void refreshAutomation()
      void subscribeBus()
    }
  },
}

/** True when `relpath` lives under one of the vault roots
 *  ([`VAULT_PATH_PREFIXES`]). Pure — exposed so the unit tests can
 *  pin the path-classification matrix without spinning a kernel. */
export function isVaultPath(relpath: string): boolean {
  if (!relpath) return false
  const normalised = relpath.replace(/\\/g, '/').replace(/^\.?\//, '')
  return VAULT_PATH_PREFIXES.some((p) => normalised.startsWith(p))
}
