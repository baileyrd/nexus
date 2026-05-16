// shell/src/plugins/nexus/recall/recallRuntime.ts
//
// BL-044 — search-side plumbing for the MEM recall overlay.
//
// Hand-off contract:
//
//   onQueryChange(query)  — debounce 200ms, then call
//                           `com.nexus.ai::semantic_search` with
//                           `{ query, limit: 10 }`. Filters the
//                           returned matches to the configured
//                           inbox-path scope (BL-043's
//                           `memory.inboxPath`). If no inbox path is
//                           set OR no matches survive the filter and
//                           the unfiltered list is non-empty,
//                           degrades to surfacing all matches with a
//                           console warning — recall is more useful
//                           even if not strictly inbox-scoped, and
//                           the full filter shape is BL-046
//                           territory.
//
//   submitInsert(api)     — read the active CodeMirror view (via
//                           `editor/runtime.getActiveCmView`),
//                           splice the formatted snippet at the
//                           current selection head. No-op when no
//                           editor is active.
//
//   submitCopy()          — write the formatted snippet to
//                           `navigator.clipboard`.
//
// The runtime is functional, not class-based, to mirror cmdIRuntime.ts.

import type { PluginAPI } from '../../../types/plugin'
import { getActiveCmView } from '../editor/runtime'
import { formatRecallLink, formatRecallSnippet } from './insertFormat'
import { useRecallStore, type RecallMatch } from './recallStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_SEMANTIC_SEARCH = 'semantic_search'
const SEARCH_LIMIT = 10

/** Debounce window — short enough that typing-then-pausing feels
 *  responsive, long enough that fast typists don't fan out a search
 *  per keystroke. 200ms is the same value the BL-039 link-suggest
 *  surface settled on after an internal poll. */
export const RECALL_DEBOUNCE_MS = 200

/** Inbox-path config key — owned by the memory plugin (BL-043). We
 *  read it through `api.configuration.getValue` rather than importing
 *  the constant so the plugins stay decoupled. */
const CONFIG_INBOX_PATH = 'memory.inboxPath'
const DEFAULT_INBOX_PATH = 'Inbox.md'

/** Tag a recall request id so future correlation noise (e.g. logging
 *  on the kernel side) can tell recall traffic apart from semantic
 *  searches issued from the palette / chat. */
const RECALL_REQUEST_PREFIX = 'recall-'

function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${RECALL_REQUEST_PREFIX}${crypto.randomUUID()}`
  }
  return `${RECALL_REQUEST_PREFIX}${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

interface SemanticSearchResult {
  matches?: unknown
}

/** Coerce raw JSON into `RecallMatch[]`. Same defensive decode pattern
 *  the semanticSearch palette plugin uses. */
function decodeMatches(raw: unknown): RecallMatch[] {
  if (!raw || typeof raw !== 'object') return []
  const wrapped = (raw as SemanticSearchResult).matches
  if (!Array.isArray(wrapped)) return []
  const out: RecallMatch[] = []
  for (const item of wrapped) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const file_path = typeof r.file_path === 'string' ? r.file_path : null
    if (!file_path) continue
    out.push({
      file_path,
      block_id: typeof r.block_id === 'number' ? r.block_id : undefined,
      chunk_text: typeof r.chunk_text === 'string' ? r.chunk_text : '',
      score: typeof r.score === 'number' ? r.score : 0,
    })
  }
  return out
}

/**
 * Filter matches to the inbox scope.
 *
 * v1 contract: the only filterable signal we have without a forge
 * scan is `file_path === inboxPath` (BL-043 writes every capture to
 * the same single file). If the user has no inbox configured, OR the
 * filter would empty a non-empty list (so the user sees nothing
 * useful for an in-scope query), we surface the unfiltered set with
 * a console warning. The closed-backlog entry calls this out as a
 * known v1 limitation tracked under BL-046 (code-aware capture adds
 * the "from project" filter primitives needed for richer scoping).
 */
export function filterToInboxScope(
  matches: RecallMatch[],
  inboxPath: string | null,
): RecallMatch[] {
  if (!inboxPath || inboxPath.length === 0) return matches
  const filtered = matches.filter((m) => m.file_path === inboxPath)
  if (filtered.length === 0 && matches.length > 0) {
    // Degrade gracefully — recall with no results is worse than
    // recall with out-of-scope results when the user explicitly
    // typed a query.
    return matches
  }
  return filtered
}

// ── Debounced search ────────────────────────────────────────────────────────

let debounceTimer: ReturnType<typeof setTimeout> | null = null

/** Cancel any pending debounce. Exposed for tests + for the close path
 *  on the overlay (so a stale fire can't repopulate a closed overlay). */
export function cancelPendingSearch(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
    debounceTimer = null
  }
}

/**
 * Schedule a debounced semantic_search call. Subsequent calls within
 * `RECALL_DEBOUNCE_MS` collapse into one — the trailing edge wins.
 *
 * Returns a Promise that resolves once the eventual search completes
 * (or rejects on error). The Promise is primarily for tests; the UI
 * reads results out of the store directly.
 */
export async function searchDebounced(
  api: PluginAPI,
  query: string,
): Promise<void> {
  cancelPendingSearch()

  const trimmed = query.trim()
  if (trimmed.length === 0) {
    // Empty query → clear results immediately (no spinner-then-empty
    // flicker). Don't transition to 'searching'.
    useRecallStore.setState({ results: [], status: 'idle', error: null })
    return
  }

  return new Promise<void>((resolve, reject) => {
    debounceTimer = setTimeout(() => {
      debounceTimer = null
      void runSearch(api, trimmed).then(resolve, reject)
    }, RECALL_DEBOUNCE_MS)
  })
}

/** Single search round-trip. Exported so tests can call it without
 *  waiting on the debounce timer. */
export async function runSearch(api: PluginAPI, query: string): Promise<void> {
  const requestId = newRequestId()
  useRecallStore.getState().beginSearch(requestId)

  let inboxPath: string | null = null
  try {
    inboxPath = api.configuration.getValue<string>(
      CONFIG_INBOX_PATH,
      DEFAULT_INBOX_PATH,
    )
  } catch {
    // Configuration registry not active (memory plugin disabled). Fall
    // through with no scope filter.
    inboxPath = null
  }

  try {
    const raw = await api.kernel.invoke<SemanticSearchResult>(
      AI_PLUGIN_ID,
      HANDLER_SEMANTIC_SEARCH,
      { query, limit: SEARCH_LIMIT },
    )
    const decoded = decodeMatches(raw)
    const scoped = filterToInboxScope(decoded, inboxPath)
    useRecallStore.getState().setResults(requestId, scoped)
  } catch (err) {
    const cur = useRecallStore.getState()
    if (cur.currentRequestId !== requestId) return // stale
    useRecallStore.getState().setError(
      err instanceof Error ? err : new Error(String(err)),
    )
  }
}

// ── Insert / copy actions ───────────────────────────────────────────────────

/** Splice the formatted snippet at the active editor's caret. Returns
 *  `true` when an editor was reachable and the splice fired,
 *  `false` when no editor is active. */
export function insertSelectedSnippet(): boolean {
  return insertSelectedFormatted(formatRecallSnippet)
}

/** AIG-06 — splice a bare `[[basename]]` link to the source note at
 *  the active editor's caret. Same return semantics as
 *  [`insertSelectedSnippet`]. */
export function insertSelectedAsLink(): boolean {
  return insertSelectedFormatted(formatRecallLink)
}

function insertSelectedFormatted(
  format: (match: RecallMatchInternal) => string,
): boolean {
  const state = useRecallStore.getState()
  const match = state.results[state.selectedIndex]
  if (!match) return false
  const view = getActiveCmView()
  if (!view) return false
  const snippet = format(match)
  const head = view.state.selection.main.head
  view.dispatch({
    changes: { from: head, to: head, insert: snippet },
    // Place the caret AFTER the insertion so the user can keep
    // typing without manually escaping a quote block / link.
    selection: { anchor: head + snippet.length },
  })
  // Pull focus back to the editor so the next keystroke lands in the
  // document, not in whatever the recall overlay leaves focused.
  view.focus()
  return true
}

// Local alias — the formatter helpers are typed against the same
// RecallMatch the store uses; importing the type by name keeps the
// helper signature legible without a runtime hop.
type RecallMatchInternal = Parameters<typeof formatRecallSnippet>[0]

/** Write the formatted snippet of the selected match to the
 *  clipboard. Returns the snippet so callers can show a confirmation
 *  toast. Returns `null` when no match is selected or the platform
 *  clipboard is unavailable. */
export async function copySelectedSnippet(): Promise<string | null> {
  const state = useRecallStore.getState()
  const match = state.results[state.selectedIndex]
  if (!match) return null
  if (typeof navigator === 'undefined' || !navigator.clipboard) return null
  const snippet = formatRecallSnippet(match)
  try {
    await navigator.clipboard.writeText(snippet)
    return snippet
  } catch {
    return null
  }
}
