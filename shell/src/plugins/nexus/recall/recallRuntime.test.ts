// shell/src/plugins/nexus/recall/recallRuntime.test.ts
//
// BL-044 — runtime coverage. Stubs the kernel + configuration surfaces
// so the search machinery exercises end-to-end without reaching Tauri
// or the actual semantic_search backend.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  RECALL_DEBOUNCE_MS,
  cancelPendingSearch,
  filterToInboxScope,
  runSearch,
  searchDebounced,
} from './recallRuntime.ts'
import { useRecallStore, type RecallMatch } from './recallStore.ts'

interface SearchCall {
  query: string
  limit: number
}

function reset(): void {
  cancelPendingSearch()
  useRecallStore.setState({
    visible: false,
    query: '',
    results: [],
    selectedIndex: 0,
    status: 'idle',
    error: null,
    currentRequestId: null,
  })
}

/**
 * Build a stub `RecallApi` (the narrow surface declared in
 * `recallApi.ts` — Phase 4.1). The runtime under test calls
 * `semanticSearch(query, limit)` and `getInboxPath()`; this stub
 * captures the search calls and returns `opts.matches` as the kernel
 * reply.
 */
function stubApi(opts: {
  matches?: unknown
  inboxPath?: string | null
  searchImpl?: (call: SearchCall) => Promise<{ matches?: unknown }>
}) {
  const calls: SearchCall[] = []
  const api = {
    semanticSearch: async (query: string, limit: number) => {
      const call: SearchCall = { query, limit }
      calls.push(call)
      if (opts.searchImpl) return opts.searchImpl(call)
      return { matches: opts.matches ?? [] }
    },
    getInboxPath: () => opts.inboxPath ?? null,
  }
  return { api, calls }
}

// ── filterToInboxScope ──────────────────────────────────────────────────────

const M = (file_path: string, chunk_text = ''): RecallMatch => ({
  file_path,
  chunk_text,
  score: 1,
})

test('filterToInboxScope: keeps only matches whose file_path equals the inbox', () => {
  const matches = [M('Inbox.md', 'a'), M('Other.md', 'b'), M('Inbox.md', 'c')]
  const out = filterToInboxScope(matches, 'Inbox.md')
  assert.equal(out.length, 2)
  assert.deepEqual(out.map((m) => m.chunk_text), ['a', 'c'])
})

test('filterToInboxScope: null/empty inbox path → passthrough', () => {
  const matches = [M('Inbox.md'), M('Other.md')]
  assert.equal(filterToInboxScope(matches, null).length, 2)
  assert.equal(filterToInboxScope(matches, '').length, 2)
})

test('filterToInboxScope: empty filtered set degrades to unfiltered (v1 limitation)', () => {
  // Captured in the closed-backlog entry: when the inbox filter
  // would zero out a non-empty result list, we surface everything
  // rather than show "no results" for a query that did hit the index.
  const matches = [M('Other.md'), M('AnotherNote.md')]
  const out = filterToInboxScope(matches, 'Inbox.md')
  assert.equal(out.length, 2)
})

// ── runSearch ───────────────────────────────────────────────────────────────

test('runSearch: maps semantic_search response into recallStore.results', async () => {
  reset()
  const { api, calls } = stubApi({
    inboxPath: null,
    matches: [
      { file_path: 'A.md', block_id: 1, chunk_text: 'aaa', score: 0.9 },
      { file_path: 'B.md', chunk_text: 'bbb', score: 0.8 },
      // Malformed entry: no file_path → dropped by the decoder.
      { chunk_text: 'orphan', score: 0.1 },
    ],
  })
  await runSearch(api, 'query')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].query, 'query')
  assert.equal(calls[0].limit, 10)
  const results = useRecallStore.getState().results
  assert.equal(results.length, 2)
  assert.equal(results[0].file_path, 'A.md')
  assert.equal(results[0].block_id, 1)
  assert.equal(results[1].file_path, 'B.md')
  assert.equal(results[1].block_id, undefined)
})

test('runSearch: applies the inbox-path filter when configured', async () => {
  reset()
  const { api } = stubApi({
    inboxPath: 'Inbox.md',
    matches: [
      { file_path: 'Inbox.md', chunk_text: 'kept', score: 0.9 },
      { file_path: 'Other.md', chunk_text: 'dropped', score: 0.5 },
    ],
  })
  await runSearch(api, 'q')
  const results = useRecallStore.getState().results
  assert.equal(results.length, 1)
  assert.equal(results[0].file_path, 'Inbox.md')
})

test('runSearch: kernel rejection flips status → error', async () => {
  reset()
  const { api } = stubApi({
    searchImpl: async () => {
      throw new Error('provider not configured')
    },
  })
  await runSearch(api, 'q')
  const s = useRecallStore.getState()
  assert.equal(s.status, 'error')
  assert.equal(s.error?.message, 'provider not configured')
})

// ── searchDebounced ─────────────────────────────────────────────────────────

test('searchDebounced: empty query short-circuits to a clean state', async () => {
  reset()
  useRecallStore.setState({
    results: [M('X.md', 'old')],
    status: 'idle',
  })
  const { api, calls } = stubApi({ matches: [] })
  await searchDebounced(api, '   ')
  assert.equal(calls.length, 0, 'no kernel call for whitespace-only query')
  assert.equal(useRecallStore.getState().results.length, 0)
})

test('searchDebounced: collapses bursts — only the last query reaches the kernel', async () => {
  reset()
  const { api, calls } = stubApi({
    inboxPath: null,
    matches: [{ file_path: 'A.md', chunk_text: 'final', score: 0.9 }],
  })
  // Three rapid keystrokes; only the last fires after the debounce window.
  void searchDebounced(api, 'a')
  void searchDebounced(api, 'ab')
  await searchDebounced(api, 'abc')
  assert.equal(calls.length, 1, 'debounce collapses bursts')
  assert.equal(calls[0].query, 'abc')
  assert.equal(useRecallStore.getState().results[0].chunk_text, 'final')
})

test('RECALL_DEBOUNCE_MS is the documented 200ms value', () => {
  assert.equal(RECALL_DEBOUNCE_MS, 200)
})
