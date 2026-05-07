// BL-054 Phase 2 — Architecture panel plugin.
//
// Reads `architecture.md` at the forge root through `com.nexus.storage`,
// parses it into a domain → task hierarchy, runs drift detection
// against the live skill / workflow registries, and renders the result
// as a sidebar leaf. Tolerant of missing / empty files — the panel
// shows a helpful empty state pointing at Phase 1 + Phase 5 instead of
// erroring out.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { OsArchitectureView } from './OsArchitectureView'
import { osArchitecturePaneViewCreator } from './OsArchitecturePaneView'
import { useOsArchitectureStore } from './osArchitectureStore'
import { parseArchitecture } from './architectureParser'
import { detectDrift } from './driftDetect'
import { clientLogger } from '../../../clientLogger'

const VIEW_ID = 'nexus.osArchitecture.view'
const COMMAND_REFRESH = 'nexus.osArchitecture.refresh'
const COMMAND_SHOW = 'nexus.osArchitecture.show'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const SKILLS_PLUGIN_ID = 'com.nexus.skills'
const WORKFLOW_PLUGIN_ID = 'com.nexus.workflow'

const ARCHITECTURE_RELPATH = 'architecture.md'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

interface ReadFileResp { bytes?: number[] }

function decodeUtf8(bytes: number[]): string {
  return new TextDecoder('utf-8').decode(new Uint8Array(bytes))
}

function stringIds(raw: unknown, key: string): string[] {
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const item of raw) {
    if (item && typeof item === 'object') {
      const value = (item as Record<string, unknown>)[key]
      if (typeof value === 'string' && value.length > 0) out.push(value)
    }
  }
  return out
}

export const osArchitecturePlugin: Plugin = {
  manifest: {
    id: 'nexus.osArchitecture',
    name: 'Architecture',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    // Soft deps: skills / workflow surfaces feed drift detection but
    // aren't required — when their list IPC fails the panel still
    // renders the architecture without drift warnings.
    dependsOn: ['nexus.workspace'],
  },

  async activate(api: PluginAPI) {
    const store = useOsArchitectureStore.getState()

    const refresh = async () => {
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        useOsArchitectureStore.getState().setStatus('idle')
        return
      }
      useOsArchitectureStore.getState().setStatus('loading')

      // Read the architecture file. Missing-file error is the
      // expected case for a forge that hasn't run Phase 5; surface as
      // 'missing' rather than 'error'.
      let source: string | null = null
      try {
        const resp = await api.kernel.invoke<ReadFileResp>(
          STORAGE_PLUGIN_ID,
          'read_file',
          { path: ARCHITECTURE_RELPATH },
        )
        source = decodeUtf8(resp.bytes ?? [])
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        if (/not\s*found|no such file|enoent/i.test(msg)) {
          useOsArchitectureStore.getState().setMissing()
          return
        }
        useOsArchitectureStore.getState().setStatus('error', msg)
        return
      }

      const architecture = parseArchitecture(source)

      // Pull the drift inputs in parallel; either failure degrades to
      // empty-set so the architecture itself still renders.
      const [skillIds, workflowNames] = await Promise.all([
        listIds(api, SKILLS_PLUGIN_ID, 'list', 'id'),
        listIds(api, WORKFLOW_PLUGIN_ID, 'list', 'name'),
      ])

      const drift = detectDrift({
        architecture,
        skillIds: new Set(skillIds),
        workflowNames: new Set(workflowNames),
      })
      useOsArchitectureStore.getState().setData(architecture, drift)
    }

    const renderView = () =>
      createElement(OsArchitectureView, { onRefresh: () => void refresh() })

    viewRegistry.register('osArchitecture', osArchitecturePaneViewCreator(renderView))

    api.activityBar.addItem({
      id: 'nexus.osArchitecture.activityItem',
      icon: '',
      iconName: 'compass',
      title: 'Architecture',
      viewId: VIEW_ID,
      priority: 45,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refresh()
    })
    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType('osArchitecture', 'main')
      workspace.revealLeaf(leaf)
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useOsArchitectureStore.getState().reset()
    })
    if (await api.kernel.available()) {
      void refresh()
    }
    // Suppress unused-warning when the only consumer is the on-load
    // call above; `store` was destructured for symmetry with sibling
    // plugins but isn't otherwise needed.
    void store
  },
}

async function listIds(
  api: PluginAPI,
  pluginId: string,
  command: string,
  field: string,
): Promise<string[]> {
  try {
    const raw = await api.kernel.invoke<unknown>(pluginId, command, {})
    return stringIds(raw, field)
  } catch (err) {
    clientLogger.warn(`[nexus.osArchitecture] ${pluginId}::${command} failed; drift skipped`, err)
    return []
  }
}
