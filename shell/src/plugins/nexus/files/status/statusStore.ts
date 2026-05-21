// BL-053 Phase 4 — per-file status cache.
//
// Wraps `com.nexus.storage::read_frontmatter` (BL-053 Phase 4
// handler) so the file-tree dot renderer can fetch a single value
// per row without re-parsing the file from disk on every scroll
// tick. Keyed by forge-relative path; entries record the most
// recently-fetched `status` plus a sentinel for "no frontmatter / no
// status".
//
// Cache is bounded at 256 entries — old keys evicted FIFO when the
// limit is exceeded — so a forge with 50 000 markdown files doesn't
// retain every status it ever read. An external mutation
// (`files:saved` / `files:modified` / `files:deleted` /
// `files:renamed`) invalidates the matching key so the next request
// re-fetches.

import { create } from 'zustand'

import type { KernelAPI } from '../../../../types/plugin'
import { clientLogger } from '../../../../clientLogger'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const READ_FRONTMATTER = 'read_frontmatter'
const CACHE_LIMIT = 256

/** A path's known status. `null` means "we fetched and there's no
 *  status set" — distinct from "we haven't fetched yet" (the
 *  cache key is absent entirely). */
export type CachedStatus = string | null

interface StatusState {
  /** Path → fetched status. `null` = no status; absent = unfetched. */
  cache: Map<string, CachedStatus>
  /** Path → in-flight promise so concurrent reads coalesce. */
  inflight: Map<string, Promise<CachedStatus>>
  /** Order of insertion for FIFO eviction. */
  order: string[]
  /** Bump on each cache mutation so React subscribers re-render. */
  revision: number
  setCached: (relpath: string, status: CachedStatus) => void
  invalidate: (relpath: string) => void
  clear: () => void
}

export const useStatusStore = create<StatusState>((set, get) => ({
  cache: new Map(),
  inflight: new Map(),
  order: [],
  revision: 0,
  setCached: (relpath, status) => {
    set((s) => {
      const cache = new Map(s.cache)
      const order = [...s.order]
      const wasCached = cache.has(relpath)
      cache.set(relpath, status)
      if (!wasCached) order.push(relpath)
      // FIFO eviction once we exceed the limit.
      while (order.length > CACHE_LIMIT) {
        const oldest = order.shift()
        if (oldest != null) cache.delete(oldest)
      }
      return { cache, order, revision: s.revision + 1 }
    })
  },
  invalidate: (relpath) => {
    set((s) => {
      if (!s.cache.has(relpath)) return s
      const cache = new Map(s.cache)
      cache.delete(relpath)
      return {
        cache,
        order: s.order.filter((p) => p !== relpath),
        revision: s.revision + 1,
      }
    })
    // Also drop any in-flight fetch so a race doesn't write back the
    // stale value after our caller has already kicked a new fetch.
    const next = get()
    if (next.inflight.has(relpath)) {
      const inflight = new Map(next.inflight)
      inflight.delete(relpath)
      set({ inflight })
    }
  },
  clear: () => set({ cache: new Map(), inflight: new Map(), order: [], revision: 0 }),
}))

interface ReadFrontmatterReply {
  status: string | null
  fields: Record<string, string>
}

/** Look up the cached status for `relpath`. Returns `undefined` when
 *  the path hasn't been fetched yet. Tests + UI both call this. */
export function getCachedStatus(relpath: string): CachedStatus | undefined {
  return useStatusStore.getState().cache.get(relpath)
}

/** Fetch the status for `relpath`, caching the result. Concurrent
 *  callers for the same path coalesce into one IPC. Errors are
 *  swallowed (status caches as `null`) so a transient failure
 *  doesn't bubble through to the UI as an alert. */
export async function fetchStatus(
  kernel: KernelAPI,
  relpath: string,
): Promise<CachedStatus> {
  const store = useStatusStore.getState()
  const cached = store.cache.get(relpath)
  if (cached !== undefined) return cached
  const inflight = store.inflight.get(relpath)
  if (inflight != null) return inflight
  const promise = (async () => {
    try {
      const reply = await kernel.invoke<ReadFrontmatterReply>(
        STORAGE_PLUGIN_ID,
        READ_FRONTMATTER,
        { path: relpath },
      )
      const status = reply?.status ?? null
      useStatusStore.getState().setCached(relpath, status)
      return status
    } catch (err) {
      clientLogger.debug('[nexus.status] read_frontmatter failed', relpath, err)
      useStatusStore.getState().setCached(relpath, null)
      return null
    } finally {
      const after = useStatusStore.getState()
      if (after.inflight.has(relpath)) {
        const next = new Map(after.inflight)
        next.delete(relpath)
        useStatusStore.setState({ inflight: next })
      }
    }
  })()
  // Stash the promise so the next concurrent caller reuses it.
  useStatusStore.setState((s) => {
    const next = new Map(s.inflight)
    next.set(relpath, promise)
    return { inflight: next }
  })
  return promise
}
