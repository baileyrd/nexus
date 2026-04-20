import type { Plugin, PluginAPI } from '../../../types/plugin'
import { BacklinksView } from './BacklinksView'
import { useBacklinksStore, type Backlink } from './backlinksStore'
import { useEditorStore } from '../editor/editorStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useRightPanelStore } from '../rightPanel/rightPanelStore'

const VIEW_ID = 'nexus.backlinks.view'
const COMMAND_FOCUS = 'nexus.backlinks.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const BACKLINKS_COMMAND = 'backlinks'

/**
 * Kernel response shape for `com.nexus.storage::backlinks`, verified
 * from crates/nexus-storage/src/graph.rs::BacklinkResult:
 *   { source_path: String, link_text: String, link_type: String }
 * No line numbers, no content excerpts — the graph stores edge
 * metadata, not index-time snippets.
 */
interface KernelBacklink {
  source_path?: unknown
  link_text?: unknown
  link_type?: unknown
}

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Map the kernel's `Vec<BacklinkResult>` into our `Backlink[]`.
 *
 * Self-references are filtered out — a file can, in principle, contain
 * a wikilink to itself; the inspector is more useful when it excludes
 * the file it's describing. The kernel's `KnowledgeGraph.backlinks`
 * walks incoming edges only, so a self-link would have to be explicit
 * for this to fire, but we filter defensively.
 */
function decode(raw: unknown, currentRelpath: string): Backlink[] {
  if (!Array.isArray(raw)) return []
  const out: Backlink[] = []
  for (const item of raw as KernelBacklink[]) {
    if (!item || typeof item !== 'object') continue
    const sourceRelpath =
      typeof item.source_path === 'string' ? item.source_path : null
    if (!sourceRelpath) continue
    if (sourceRelpath === currentRelpath) continue
    const linkText = typeof item.link_text === 'string' ? item.link_text : ''
    const linkType = typeof item.link_type === 'string' ? item.link_type : ''
    out.push({
      sourceRelpath,
      sourceName: basename(sourceRelpath) || sourceRelpath,
      linkText,
      linkType,
    })
  }
  return out
}

export const backlinksPlugin: Plugin = {
  manifest: {
    id: 'nexus.backlinks',
    name: 'Backlinks',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Backlinks', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    // Contribute the body into the rightPanelContent slot. Priority 20
    // places us after nexus.outline (priority 10), so Outline stays the
    // first-registered-wins default tab.
    api.views.register(VIEW_ID, {
      slot: 'rightPanelContent',
      component: BacklinksView,
      priority: 20,
    })

    // Advertise the tab label to the rightPanel host.
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Backlinks',
      priority: 20,
      iconName: 'linkIn',
    })

    // ── Loader + requestId guard ─────────────────────────────────────
    //
    // A fast switch between editor tabs would otherwise race: response
    // for file A can arrive after the user has already switched to
    // file B. Tag every call with a monotonic id and drop late
    // responses whose id is stale.
    let currentRequestId = 0

    const load = async (relpath: string | null) => {
      const store = useBacklinksStore.getState()
      if (!relpath) {
        // Bump the id so any late response gets dropped, then clear.
        currentRequestId++
        store.setCurrent(null)
        store.setLinks([])
        store.setLoading(false)
        store.setError(null)
        return
      }

      const requestId = ++currentRequestId
      store.setCurrent(relpath)
      store.setLinks([])
      store.setError(null)
      store.setLoading(true)

      // Kernel-availability guard — during workspace teardown /
      // boot windows `available()` returns false and invoke would
      // reject with "no workspace open", which isn't user-actionable.
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
        const raw = await api.kernel.invoke<KernelBacklink[]>(
          STORAGE_PLUGIN_ID,
          BACKLINKS_COMMAND,
          { path: relpath },
        )
        if (requestId !== currentRequestId) return
        useBacklinksStore.getState().setLinks(decode(raw, relpath))
        useBacklinksStore.getState().setLoading(false)
      } catch (err) {
        if (requestId !== currentRequestId) return
        const message = err instanceof Error ? err.message : String(err)
        useBacklinksStore.getState().setLinks([])
        useBacklinksStore.getState().setError(message)
        useBacklinksStore.getState().setLoading(false)
      }
    }

    // Subscribe to active-tab changes. Only kick a reload when
    // `activeRelpath` itself changes — content edits on the same tab
    // don't affect the backlinks set for this one-shot inspector.
    useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        void load(state.activeRelpath)
      }
    })

    // Seed with whatever is active at activation time. Covers the
    // workspace-restore path where the editor already has a tab open
    // by the time we mount. Deferred to the next microtask so the
    // kernel's `available()` call happens after the host finishes
    // wiring up every plugin.
    queueMicrotask(() => {
      const initial = useEditorStore.getState().activeRelpath
      if (initial) void load(initial)
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      currentRequestId++
      useBacklinksStore.getState().clear()
    })

    // Focus command — ensures the right panel is visible and selects
    // the Backlinks tab. Titlebar shortcut + command palette entry.
    api.commands.register(COMMAND_FOCUS, () => {
      useLayoutStore.setState((s) => ({
        rightPanel: { ...s.rightPanel, visible: true },
      }))
      useRightPanelStore.getState().setActive(VIEW_ID)
    })
  },
}
