import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { GraphGlobalView } from './GraphGlobalView'
import { graphGlobalPaneViewCreator } from './GraphGlobalPaneView'
import { useGlobalGraphStore } from './graphGlobalStore'

const VIEW_TYPE = 'graph-global'
const VIEW_ID = 'nexus.graph.global.view'

const COMMAND_OPEN = 'nexus.graph.openGlobal'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const LIST_ALL_LINKS = 'list_all_links'

const TOPIC_FILE_CREATED = 'com.nexus.storage.file_created'
const TOPIC_FILE_MODIFIED = 'com.nexus.storage.file_modified'
const TOPIC_FILE_DELETED = 'com.nexus.storage.file_deleted'
const TOPIC_FILE_RENAMED = 'com.nexus.storage.file_renamed'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Coalesce bursty index events; force-sim thrashing on every save in a
// large vault is ugly and pointless.
const REFRESH_DEBOUNCE_MS = 500

interface KernelNode {
  path?: unknown
  is_phantom?: unknown
}

interface KernelEdge {
  source?: unknown
  target?: unknown
  is_resolved?: unknown
}

interface KernelSnapshot {
  nodes?: unknown
  edges?: unknown
}

function decode(raw: unknown): {
  nodes: { path: string; isPhantom: boolean }[]
  edges: { source: string; target: string; isResolved: boolean }[]
} {
  const obj = raw as KernelSnapshot
  const nodes: { path: string; isPhantom: boolean }[] = []
  const edges: { source: string; target: string; isResolved: boolean }[] = []
  if (Array.isArray(obj?.nodes)) {
    for (const n of obj.nodes as KernelNode[]) {
      if (!n || typeof n !== 'object') continue
      if (typeof n.path !== 'string') continue
      nodes.push({ path: n.path, isPhantom: n.is_phantom === true })
    }
  }
  if (Array.isArray(obj?.edges)) {
    for (const e of obj.edges as KernelEdge[]) {
      if (!e || typeof e !== 'object') continue
      if (typeof e.source !== 'string' || typeof e.target !== 'string') continue
      edges.push({
        source: e.source,
        target: e.target,
        isResolved: e.is_resolved === true,
      })
    }
  }
  return { nodes, edges }
}

export const graphGlobalPlugin: Plugin = {
  manifest: {
    id: 'nexus.graph.global',
    name: 'Global Graph',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.activityBar'],
    contributes: {
      commands: [
        { id: COMMAND_OPEN, title: 'Open Global Graph', category: 'Graph' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.viewRegistry.register(
      VIEW_TYPE,
      graphGlobalPaneViewCreator(() => createElement(GraphGlobalView)),
    )

    let inFlight = 0
    const refresh = async () => {
      const store = useGlobalGraphStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setLoading(false)
        store.setError('Open a workspace to load the graph.')
        return
      }
      const reqId = ++inFlight
      store.setLoading(true)
      store.setError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(
          STORAGE_PLUGIN_ID,
          LIST_ALL_LINKS,
          {},
        )
        if (reqId !== inFlight) return
        const { nodes, edges } = decode(raw)
        useGlobalGraphStore.getState().setSnapshot(nodes, edges)
        useGlobalGraphStore.getState().setLoading(false)
      } catch (err) {
        if (reqId !== inFlight) return
        const message = err instanceof Error ? err.message : String(err)
        useGlobalGraphStore.getState().setError(message)
        useGlobalGraphStore.getState().setLoading(false)
      }
    }

    let debounceTimer: ReturnType<typeof setTimeout> | null = null
    const scheduleRefresh = () => {
      if (debounceTimer) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(() => {
        debounceTimer = null
        void refresh()
      }, REFRESH_DEBOUNCE_MS)
    }

    api.commands.register(COMMAND_OPEN, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'main')
      workspace.revealLeaf(leaf)
      void refresh()
    })

    api.activityBar.addItem({
      id: 'nexus.graph.global.activityItem',
      icon: '',
      iconName: 'graph',
      title: 'Graph',
      viewId: VIEW_ID,
      priority: 55,
      command: COMMAND_OPEN,
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      if (debounceTimer) {
        clearTimeout(debounceTimer)
        debounceTimer = null
      }
      useGlobalGraphStore.getState().clear()
    })

    // Keep the graph in sync with vault edits — debounced so a git checkout
    // burst doesn't trigger N separate force-sim resets.
    const subscribeFs = async () => {
      try {
        await Promise.all([
          api.kernel.on(TOPIC_FILE_CREATED, scheduleRefresh),
          api.kernel.on(TOPIC_FILE_MODIFIED, scheduleRefresh),
          api.kernel.on(TOPIC_FILE_DELETED, scheduleRefresh),
          api.kernel.on(TOPIC_FILE_RENAMED, scheduleRefresh),
        ])
      } catch {
        // Subscriptions fail when the kernel is down on first boot; the
        // workspace:opened handler will trigger a load shortly after.
      }
    }
    void subscribeFs()

    if (await api.kernel.available()) {
      void refresh()
    }
  },
}
