import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { GraphView } from './GraphView'
import { graphPaneViewCreator } from './GraphPaneView'
import {
  useGraphStore,
  type EdgeDirection,
  type GraphNeighbour,
} from './graphStore'
import { useEditorStore } from '../editor/editorStore'

const VIEW_ID = 'nexus.graph.view'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const OUTGOING_COMMAND = 'outgoing_links'
const BACKLINKS_COMMAND = 'backlinks'

/**
 * Kernel response shape for `com.nexus.storage::outgoing_links`,
 * verified from crates/nexus-storage/src/graph.rs::OutgoingLink:
 *   { target_path: String, link_text: String, link_type: String,
 *     is_resolved: bool, fragment: Option<String> }
 * We only need `target_path` for the neighbourhood graph.
 */
interface KernelOutgoing {
  target_path?: unknown
}

/**
 * Kernel response shape for `com.nexus.storage::backlinks`, verified
 * from crates/nexus-storage/src/graph.rs::BacklinkResult:
 *   { source_path: String, link_text: String, link_type: String }
 */
interface KernelBacklink {
  source_path?: unknown
}

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Merge outgoing + incoming kernel hits into a single neighbour list.
 *
 * Keyed on neighbour relpath so the same file appearing on both
 * sides collapses to a single node with `direction='both'`. Self-
 * references (neighbour relpath == current relpath) are filtered —
 * the kernel's graph shouldn't normally surface those, but a
 * defensively-authored note with an explicit self-link would
 * otherwise draw an edge-to-nowhere.
 */
function mergeNeighbours(
  outgoing: unknown,
  incoming: unknown,
  currentRelpath: string,
): GraphNeighbour[] {
  const map = new Map<string, EdgeDirection>()

  if (Array.isArray(outgoing)) {
    for (const item of outgoing as KernelOutgoing[]) {
      if (!item || typeof item !== 'object') continue
      const target =
        typeof item.target_path === 'string' ? item.target_path : null
      if (!target || target === currentRelpath) continue
      const existing = map.get(target)
      map.set(target, existing === 'incoming' ? 'both' : 'outgoing')
    }
  }

  if (Array.isArray(incoming)) {
    for (const item of incoming as KernelBacklink[]) {
      if (!item || typeof item !== 'object') continue
      const source =
        typeof item.source_path === 'string' ? item.source_path : null
      if (!source || source === currentRelpath) continue
      const existing = map.get(source)
      map.set(source, existing === 'outgoing' ? 'both' : 'incoming')
    }
  }

  const out: GraphNeighbour[] = []
  for (const [relpath, direction] of map) {
    out.push({
      relpath,
      name: basename(relpath) || relpath,
      direction,
    })
  }
  return out
}

export const graphPlugin: Plugin = {
  manifest: {
    id: 'nexus.graph',
    name: 'Graph',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      configuration: {
        pluginId: 'nexus.graph',
        title: 'Graph',
        order: 35,
        category: 'navigation',
        schema: [
          {
            key: 'nexus.graph.labelWidth',
            title: 'Node label width (characters)',
            description: 'Maximum characters shown on right-panel graph node labels before truncating with an ellipsis. Applied live.',
            type: 'number',
            default: 14,
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    // Third right-panel tab after Outline (10) and Backlinks (20).
    // First-registered-wins keeps Outline as the default tab; this
    // inspector is the most expensive to render, so parking it last
    // also means it doesn't win by accident if something reorders
    // activation.
    // Phase 7: legacy SlotRegistry slot:'rightPanelContent' entry removed.
    api.viewRegistry.register(
      'graph',
      graphPaneViewCreator(() => createElement(GraphView)),
    )

    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Graph',
      priority: 30,
      iconName: 'graph',
    })

    // ── Loader + requestId guard ─────────────────────────────────────
    //
    // Identical pattern to nexus.backlinks — a fast editor-tab switch
    // otherwise races two kernel responses for two different files.
    // Tag every call with a monotonic id and drop late ones.
    let currentRequestId = 0

    const load = async (relpath: string | null) => {
      const store = useGraphStore.getState()
      if (!relpath) {
        currentRequestId++
        store.setCurrent(null, null)
        store.setNeighbours([])
        store.setLoading(false)
        store.setError(null)
        return
      }

      const requestId = ++currentRequestId
      const name = basename(relpath) || relpath
      store.setCurrent(relpath, name)
      store.setNeighbours([])
      store.setError(null)
      store.setLoading(true)

      // Kernel-availability guard — during workspace teardown /
      // boot windows `available()` returns false and the kernel
      // invoke would reject with "no workspace open", which isn't a
      // useful message in the Graph tab.
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (requestId !== currentRequestId) return
      if (!available) {
        store.setLoading(false)
        store.setError('Kernel not ready.')
        return
      }

      try {
        // Parallel — the two calls share no state and the merge is
        // cheap, so there's no reason to serialise them.
        const [outgoing, incoming] = await Promise.all([
          api.kernel.invoke<KernelOutgoing[]>(
            STORAGE_PLUGIN_ID,
            OUTGOING_COMMAND,
            { path: relpath },
          ),
          api.kernel.invoke<KernelBacklink[]>(
            STORAGE_PLUGIN_ID,
            BACKLINKS_COMMAND,
            { path: relpath },
          ),
        ])
        if (requestId !== currentRequestId) return
        const merged = mergeNeighbours(outgoing, incoming, relpath)
        useGraphStore.getState().setNeighbours(merged)
        useGraphStore.getState().setLoading(false)
      } catch (err) {
        if (requestId !== currentRequestId) return
        const message = err instanceof Error ? err.message : String(err)
        useGraphStore.getState().setNeighbours([])
        useGraphStore.getState().setError(message)
        useGraphStore.getState().setLoading(false)
      }
    }

    // Reload whenever the editor's active tab changes. Content edits
    // on the same tab don't affect the link graph until the file is
    // re-indexed; the kernel handles that via its own storage events
    // elsewhere, not from inside this plugin.
    useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        void load(state.activeRelpath)
      }
    })

    // Seed with whatever is active at activation time. Covers the
    // workspace-restore path where the editor has already reopened a
    // tab by the time we mount. Deferred to the next microtask so
    // `kernel.available()` runs after the host finishes wiring every
    // plugin.
    queueMicrotask(() => {
      const initial = useEditorStore.getState().activeRelpath
      if (initial) void load(initial)
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      currentRequestId++
      useGraphStore.getState().clear()
    })
  },
}
