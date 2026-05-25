// Always-on backlinks loader for noteContext.
//
// Subscribes to the editor's active relpath and fetches the backlinks
// list whenever it changes, writing into `useBacklinksDataStore`. Runs
// regardless of whether the Backlinks accordion section is expanded —
// the count + loading indicator surfaced in `RightPanelFooter` and
// `FileStats` consume the same store, so freshness matters even when
// the panel itself isn't visible.
//
// The legacy `nexus.backlinks` plugin owned this subscriber. After
// the Phase 4.3 merge, it lives here.
//
// BL-049 phase 4 — when the store's `blockFilter` is set, the load
// path switches from the `backlinks` IPC to `backlinks_to_block`.
// Toggling the filter re-runs the load with the new mode. Switching
// to a different file clears the filter (a block id from file A is
// meaningless against file B's inbound table).
//
// On-edit silent refresh — also wired here. The editor's
// `sessionManager.onChanged(...)` fires after every committed mutation
// to a markdown session. We rAF-coalesce same-file events and silently
// re-query the backlinks index without flashing the loading flag. The
// architectural caveat the legacy plugin noted still applies: editing
// file A doesn't change file A's *incoming* backlinks (those live on
// other files), so today this is largely a no-op. It exists as the
// well-defined hook for a future cross-file reindex event.

import type { PluginAPI } from '../../../types/plugin'
import { useEditorStore } from '../editor/editorStore'
import { getEditorRuntime } from '../editor/runtime'
import type { EditorChangedPayload } from '../editor/types'
import { useBacklinksDataStore, type Backlink } from './backlinksDataStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const BACKLINKS_COMMAND = 'backlinks'
/** BL-049 phase 4 — handler is registered as `backlinks_to_block` in
 *  nexus-bootstrap (delegates to `KnowledgeGraph::backlinks_to_block`). */
const BACKLINKS_TO_BLOCK_COMMAND = 'backlinks_to_block'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

interface KernelBacklink {
  source_path?: unknown
  link_text?: unknown
  link_type?: unknown
  fragment?: unknown
}

export function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** Decode a kernel `Vec<BacklinkResult>` into our `Backlink[]`,
 *  filtering self-references (a file can in principle link to itself
 *  but the inspector is more useful when those are excluded). */
export function decode(raw: unknown, currentRelpath: string): Backlink[] {
  if (!Array.isArray(raw)) return []
  const out: Backlink[] = []
  for (const item of raw as KernelBacklink[]) {
    if (!item || typeof item !== 'object') continue
    const sourceRelpath =
      typeof item.source_path === 'string' ? item.source_path : null
    if (!sourceRelpath || sourceRelpath === currentRelpath) continue
    out.push({
      sourceRelpath,
      sourceName: basename(sourceRelpath) || sourceRelpath,
      linkText: typeof item.link_text === 'string' ? item.link_text : '',
      linkType: typeof item.link_type === 'string' ? item.link_type : '',
      fragment:
        typeof item.fragment === 'string' && item.fragment.length > 0
          ? item.fragment
          : null,
    })
  }
  return out
}

/**
 * Wire the always-on backlinks subscriber. Call once from
 * `noteContext.activate()`.
 *
 * Tab-switch races are guarded with a monotonic request-id — a slow
 * response for file A that arrives after the user has switched to
 * file B is dropped before it can overwrite B's data.
 */
export function startBacklinksLoader(api: PluginAPI): void {
  let currentRequestId = 0

  const load = async (relpath: string | null): Promise<void> => {
    const store = useBacklinksDataStore.getState()
    if (!relpath) {
      currentRequestId++
      store.clear()
      return
    }

    const requestId = ++currentRequestId
    store.setCurrent(relpath)
    store.setLinks([])
    store.setError(null)
    store.setLoading(true)

    // Kernel-availability guard — during workspace teardown / boot
    // windows `available()` returns false and the invoke would
    // reject with "no workspace open".
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
      const blockFilter = useBacklinksDataStore.getState().blockFilter
      const raw = blockFilter
        ? await api.kernel.invoke<KernelBacklink[]>(
            STORAGE_PLUGIN_ID,
            BACKLINKS_TO_BLOCK_COMMAND,
            { path: relpath, block_id: blockFilter },
          )
        : await api.kernel.invoke<KernelBacklink[]>(
            STORAGE_PLUGIN_ID,
            BACKLINKS_COMMAND,
            { path: relpath },
          )
      if (requestId !== currentRequestId) return
      store.setLinks(decode(raw, relpath))
      store.setLoading(false)
    } catch (err) {
      if (requestId !== currentRequestId) return
      const message = err instanceof Error ? err.message : String(err)
      store.setLinks([])
      store.setError(message)
      store.setLoading(false)
    }
  }

  /**
   * Silent refresh — re-queries the inbound-link index for `relpath`
   * without flipping `loading` to true. Used by the editor-change
   * subscription below so an in-progress edit picks up any future
   * cross-file reindex without flashing "Loading…" on every
   * keystroke. Guarded by the same request-id pattern as `load`.
   *
   * Bails silently on error: a transient failure here shouldn't
   * clobber the displayed list. The next explicit tab switch will
   * surface any persistent problem.
   */
  const refresh = async (relpath: string): Promise<void> => {
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
      const blockFilter = useBacklinksDataStore.getState().blockFilter
      const raw = blockFilter
        ? await api.kernel.invoke<KernelBacklink[]>(
            STORAGE_PLUGIN_ID,
            BACKLINKS_TO_BLOCK_COMMAND,
            { path: relpath, block_id: blockFilter },
          )
        : await api.kernel.invoke<KernelBacklink[]>(
            STORAGE_PLUGIN_ID,
            BACKLINKS_COMMAND,
            { path: relpath },
          )
      if (requestId !== currentRequestId) return
      const store = useBacklinksDataStore.getState()
      // Drop the result if the user switched files while we were in
      // flight — `load()` for the new file has already taken over.
      if (store.currentRelpath !== relpath) return
      store.setLinks(decode(raw, relpath))
      store.setError(null)
    } catch {
      // Silent.
    }
  }

  // React to editor tab switches. A different file invalidates the
  // active block filter (a UUID stamped in file A doesn't apply to
  // file B's inbound table).
  useEditorStore.subscribe((state, prev) => {
    if (state.activeRelpath !== prev.activeRelpath) {
      const cur = useBacklinksDataStore.getState()
      if (cur.blockFilter !== null) cur.setBlockFilter(null)
      void load(state.activeRelpath)
    }
  })

  // React to block-filter toggling — re-issue the load against the
  // same file with the new mode.
  useBacklinksDataStore.subscribe((state, prev) => {
    if (state.blockFilter !== prev.blockFilter && state.currentRelpath) {
      void load(state.currentRelpath)
    }
  })

  // On-edit silent refresh. The editor's session manager publishes
  // a `com.nexus.editor.changed.<relpath>` event after each
  // committed mutation; subscribe and rAF-coalesce so a flurry of
  // keystrokes collapses into one refresh per frame.
  //
  // Deferred to the next microtask: the editor plugin sets up its
  // runtime in its own activate(), and `register_all` boot order
  // doesn't guarantee that runs before us (it should — `nexus.editor`
  // is in our dependsOn — but `getEditorRuntime()` returning null
  // is a defensible failure mode if the editor plugin isn't loaded
  // for any reason).
  let rafHandle: number | null = null
  queueMicrotask(() => {
    const runtime = getEditorRuntime()
    if (!runtime) return
    runtime.sessionManager.onChanged((payload: EditorChangedPayload) => {
      const active = useEditorStore.getState().activeRelpath
      if (payload.relpath !== active) return
      if (rafHandle !== null) return
      rafHandle = requestAnimationFrame(() => {
        rafHandle = null
        const relpath = useEditorStore.getState().activeRelpath
        if (!relpath) return
        void refresh(relpath)
      })
    })
  })

  // Seed with whatever is active at activation time — covers the
  // workspace-restore path where the editor already has a tab open
  // by the time we mount. Deferred to the next microtask so the
  // kernel's `available()` call runs after the host finishes wiring
  // every plugin.
  queueMicrotask(() => {
    const initial = useEditorStore.getState().activeRelpath
    if (initial) void load(initial)
  })

  api.events.on(EVENT_WORKSPACE_CLOSED, () => {
    currentRequestId++
    useBacklinksDataStore.getState().clear()
  })
}
