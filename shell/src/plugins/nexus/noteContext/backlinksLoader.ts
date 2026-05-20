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
// What's NOT in this module:
//   - On-edit silent refresh (subscribing to editor session changes
//     and re-running load without a loading flash). Lands in a
//     follow-up commit; legacy code itself flagged the in-process
//     refresh as "largely a no-op" because editing file A doesn't
//     change its incoming backlinks.
//   - Block-filter mode (BL-049 phase 4 — toggle between `backlinks`
//     and `backlinks_to_block` IPCs). Lands in a follow-up commit;
//     the data store will gain a `blockFilter` field at that point.

import type { PluginAPI } from '../../../types/plugin'
import { useEditorStore } from '../editor/editorStore'
import { useBacklinksDataStore, type Backlink } from './backlinksDataStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const BACKLINKS_COMMAND = 'backlinks'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

interface KernelBacklink {
  source_path?: unknown
  link_text?: unknown
  link_type?: unknown
  fragment?: unknown
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** Decode a kernel `Vec<BacklinkResult>` into our `Backlink[]`,
 *  filtering self-references (a file can in principle link to itself
 *  but the inspector is more useful when those are excluded). */
function decode(raw: unknown, currentRelpath: string): Backlink[] {
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
      const raw = await api.kernel.invoke<KernelBacklink[]>(
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

  // React to editor tab switches.
  useEditorStore.subscribe((state, prev) => {
    if (state.activeRelpath !== prev.activeRelpath) {
      void load(state.activeRelpath)
    }
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
