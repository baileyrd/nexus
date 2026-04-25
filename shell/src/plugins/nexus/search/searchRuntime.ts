// Module-scoped holder for the kernel API handle and the search
// input's focus callback.
//
// Held out of React so:
//   * the plugin's `activate` can stash them once
//   * `index.ts`'s debounced invoker can reach the kernel without
//     threading PluginAPI through the component tree
//   * the `nexus.search.focus` command can call the input's `focus()`
//     method even when the sidebar is hidden (the view is lazily
//     mounted — the focus callback is only present while SearchView
//     is mounted, so we gate on that).

import type { KernelAPI } from '../../../types/plugin'
import { configStore } from '../../../stores/configStore'
import type { SearchHit } from './searchStore'
import { useSearchStore } from './searchStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const SEARCH_COMMAND = 'search'
const MAX_SEARCH_RESULTS = 50
const CONFIG_KEY_SEARCH_LIMIT = 'search.maxResultsLimit'

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI) {
  kernel = api
}

export function getKernel(): KernelAPI | null {
  return kernel
}

// ── Focus plumbing ──────────────────────────────────────────────────────
//
// SearchView only exists in the DOM while the search Leaf is mounted
// in a sidedock. The `nexus.search.focus` command needs to (a) raise
// the view if it isn't showing and (b) focus the input. We handle (a)
// by calling `workspace.ensureLeafOfType + revealLeaf` from the focus
// command itself; we handle (b) via this registered focuser callback,
// set by SearchView in a layout effect. If the view was previously
// unmounted, the callback is null — we set a pending flag so the next
// time the view mounts it auto-focuses. That flag is also set on
// initial open from the activity bar so the input is focused when the
// view first appears.

type Focuser = () => void
let focuser: Focuser | null = null
let pendingFocus = false

export function registerFocuser(fn: Focuser | null) {
  focuser = fn
  if (fn && pendingFocus) {
    pendingFocus = false
    fn()
  }
}

/**
 * Focus the search input if it's mounted; otherwise remember the
 * request and focus on next mount.
 */
export function requestFocus() {
  if (focuser) {
    focuser()
  } else {
    pendingFocus = true
  }
}

// ── Debounced kernel invocation ─────────────────────────────────────────
//
// Each keystroke schedules a 150ms trailing-debounced `search` call.
// A monotonically incrementing `requestId` tags the in-flight call —
// responses whose id is no longer current are dropped, so a slow
// response that arrives after the user has typed further never
// clobbers newer results.

const DEBOUNCE_MS = 150
let currentRequestId = 0
let pendingTimer: ReturnType<typeof setTimeout> | null = null

/** Cancel any in-flight / pending call. Called on workspace close. */
export function cancelInFlight() {
  if (pendingTimer) {
    clearTimeout(pendingTimer)
    pendingTimer = null
  }
  currentRequestId++
}

/**
 * Decode the kernel response into `SearchHit[]`.
 *
 * The handler returns `Vec<SearchResult>` where each element is
 * `{ file_path, block_id, block_type, excerpt, score }`. Block-level
 * duplicates (multiple blocks matching in the same file) are
 * deduped by taking the highest-scoring hit per file — the sidebar
 * is file-oriented, not block-oriented.
 */
function decode(raw: unknown): SearchHit[] {
  if (!Array.isArray(raw)) return []
  const byPath = new Map<string, SearchHit>()
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const relpath = typeof r.file_path === 'string' ? r.file_path : null
    if (!relpath) continue
    const snippet = typeof r.excerpt === 'string' ? r.excerpt : ''
    const score = typeof r.score === 'number' ? r.score : 0
    const existing = byPath.get(relpath)
    if (!existing || score > existing.score) {
      byPath.set(relpath, { relpath, snippet, score })
    }
  }
  return Array.from(byPath.values()).sort((a, b) => b.score - a.score)
}

/**
 * Schedule a trailing-debounced search for `query`. If `query` is
 * empty, clears results immediately and cancels any pending call.
 */
export function scheduleSearch(query: string) {
  if (pendingTimer) {
    clearTimeout(pendingTimer)
    pendingTimer = null
  }

  const trimmed = query.trim()
  if (!trimmed) {
    // Bump the request id so any in-flight response gets dropped
    // before it can land and flash stale results.
    currentRequestId++
    const store = useSearchStore.getState()
    store.setResults([])
    store.setLoading(false)
    store.setError(null)
    return
  }

  pendingTimer = setTimeout(() => {
    pendingTimer = null
    void runSearch(trimmed)
  }, DEBOUNCE_MS)
}

async function runSearch(query: string) {
  const requestId = ++currentRequestId
  const store = useSearchStore.getState()

  const k = kernel
  if (!k) {
    store.setLoading(false)
    store.setError('Kernel not ready.')
    return
  }

  // Guard against kernel calls between workspace-closed and the next
  // workspace-opened. `api.kernel.available()` returns false during
  // that window and would reject the invoke with "no workspace open".
  let available = false
  try {
    available = await k.available()
  } catch {
    available = false
  }
  if (requestId !== currentRequestId) return
  if (!available) {
    store.setLoading(false)
    store.setError('Kernel not ready.')
    return
  }

  store.setLoading(true)
  store.setError(null)

  try {
    const raw = await k.invoke(STORAGE_PLUGIN_ID, SEARCH_COMMAND, {
      query,
      limit: configStore.get<number>(CONFIG_KEY_SEARCH_LIMIT, MAX_SEARCH_RESULTS) ?? MAX_SEARCH_RESULTS,
    })
    if (requestId !== currentRequestId) return
    const hits = decode(raw)
    store.setResults(hits)
    store.setLoading(false)
  } catch (err) {
    if (requestId !== currentRequestId) return
    const message = err instanceof Error ? err.message : String(err)
    store.setResults([])
    store.setError(message)
    store.setLoading(false)
  }
}
