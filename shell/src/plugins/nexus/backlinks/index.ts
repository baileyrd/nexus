import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { BacklinksView } from './BacklinksView'
import { backlinkViewCreator } from './BacklinkView'
import { useBacklinksStore, type Backlink } from './backlinksStore'
import { useEditorStore } from '../editor/editorStore'
import { getEditorRuntime } from '../editor/runtime'
import type { EditorChangedPayload } from '../editor/types'

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
  /** BL-049 phase 3 — `^<block-id>` for block-anchored links, the
   *  heading slug for heading-anchored links, absent for plain
   *  wikilinks. The Rust side adds `#[serde(skip_serializing_if =
   *  "Option::is_none")]` so older snapshots stay JSON-compatible. */
  fragment?: unknown
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
export function decode(raw: unknown, currentRelpath: string): Backlink[] {
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
    const fragment =
      typeof item.fragment === 'string' && item.fragment.length > 0
        ? item.fragment
        : null
    out.push({
      sourceRelpath,
      sourceName: basename(sourceRelpath) || sourceRelpath,
      linkText,
      linkType,
      fragment,
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
    // Phase 7: legacy SlotRegistry slot:'rightPanelContent' entry removed.
    viewRegistry.register(
      'backlink',
      backlinkViewCreator(() => createElement(BacklinksView)),
    )

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

    /**
     * Silent refresh — re-queries the backlinks index for `relpath`
     * without flashing the UI through a `loading` state. Used by the
     * Phase 7 change-event hook: an in-progress edit to the active
     * file does not change its *inbound* backlinks (those live on
     * other files), but a future write-through reindex triggered off
     * an editor change could. Keeping this as a silent refresh means
     * we pick up any such update without the UI shimmering to
     * "Loading…" on every keystroke.
     *
     * Request-id guarded against tab-switch races (same pattern as
     * `load` above).
     */
    const refresh = async (relpath: string) => {
      const requestId = ++currentRequestId
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) return
      if (requestId !== currentRequestId) return
      try {
        const raw = await api.kernel.invoke<KernelBacklink[]>(
          STORAGE_PLUGIN_ID,
          BACKLINKS_COMMAND,
          { path: relpath },
        )
        if (requestId !== currentRequestId) return
        const store = useBacklinksStore.getState()
        // Only write through if we're still looking at this file —
        // otherwise the user has switched tabs and `load` for the new
        // tab has already taken over.
        if (store.currentRelpath !== relpath) return
        store.setLinks(decode(raw, relpath))
        store.setError(null)
      } catch {
        // Silent refresh — failures here shouldn't clobber the
        // currently-displayed results. The next explicit tab switch
        // will surface any persistent error.
      }
    }

    // Subscribe to active-tab changes. A tab switch does a full
    // `load` (flashes "Loading…"); a content edit on the same tab
    // does a silent `refresh` (see onChanged below). We skip the
    // subscribe-driven reload for same-relpath mutations so this
    // handler only fires on actual file switches.
    useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        void load(state.activeRelpath)
      }
    })

    // Phase 7: subscribe to editor change events for parity with the
    // outline plugin. Note the architectural caveat: backlinks-TO a
    // file change when OTHER files' link tables update, not when the
    // file itself is edited. Today the `com.nexus.editor.changed`
    // channel only fires on tree mutations of the same file, so this
    // subscription is largely a no-op for typing — but it gives us a
    // well-defined hook for future cross-file change events
    // (e.g. a future "storage:reindexed" fan-out after `save` writes
    // through). We run a coalesced silent refresh so the UI stays
    // stable even if events fire rapidly.
    let rafHandle: number | null = null
    let unsubscribeChanged: (() => void) | null = null
    queueMicrotask(() => {
      const runtime = getEditorRuntime()
      if (!runtime) return
      unsubscribeChanged = runtime.sessionManager.onChanged(
        (payload: EditorChangedPayload) => {
          const active = useEditorStore.getState().activeRelpath
          if (payload.relpath !== active) return
          if (rafHandle !== null) return
          rafHandle = requestAnimationFrame(() => {
            rafHandle = null
            const relpath = useEditorStore.getState().activeRelpath
            if (!relpath) return
            void refresh(relpath)
          })
        },
      )
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
      if (rafHandle !== null) {
        cancelAnimationFrame(rafHandle)
        rafHandle = null
      }
      useBacklinksStore.getState().clear()
    })

    // Focus command — ensures the right panel is visible and selects
    // the Backlinks tab. Titlebar shortcut + command palette entry.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('backlink', 'right')
      workspace.revealLeaf(leaf)
    })

    // Keep a reference so tree-shaking doesn't strip the teardown.
    void unsubscribeChanged
  },
}
