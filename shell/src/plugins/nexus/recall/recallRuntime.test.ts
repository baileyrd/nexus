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

interface InvokeCall {
  pluginId: string
  commandId: string
  args: Record<string, unknown>
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

/** Build a stub PluginAPI with kernel.invoke + configuration.getValue. */
function stubApi(opts: {
  matches?: unknown
  inboxPath?: string | null
  invokeImpl?: (call: InvokeCall) => Promise<unknown>
}) {
  const calls: InvokeCall[] = []
  const inbox = opts.inboxPath
  const api = {
    kernel: {
      invoke: async (
        pluginId: string,
        commandId: string,
        args: unknown,
      ) => {
        const call: InvokeCall = {
          pluginId,
          commandId,
          args: args as Record<string, unknown>,
        }
        calls.push(call)
        if (opts.invokeImpl) return opts.invokeImpl(call)
        return { matches: opts.matches ?? [] }
      },
    },
    configuration: {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      getValue: <T,>(key: string, def: T): T => {
        if (key === 'memory.inboxPath') {
          return (inbox === undefined ? def : (inbox as unknown as T)) as T
        }
        return def
      },
    },
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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await runSearch(api as any, 'query')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.ai')
  assert.equal(calls[0].commandId, 'semantic_search')
  assert.equal((calls[0].args as { query: string }).query, 'query')
  assert.equal((calls[0].args as { limit: number }).limit, 10)
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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await runSearch(api as any, 'q')
  const results = useRecallStore.getState().results
  assert.equal(results.length, 1)
  assert.equal(results[0].file_path, 'Inbox.md')
})

test('runSearch: kernel rejection flips status → error', async () => {
  reset()
  const api = {
    kernel: {
      invoke: async () => {
        throw new Error('provider not configured')
      },
    },
    configuration: { getValue: <T,>(_k: string, d: T) => d },
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await runSearch(api as any, 'q')
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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await searchDebounced(api as any, '   ')
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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  void searchDebounced(api as any, 'a')
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  void searchDebounced(api as any, 'ab')
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await searchDebounced(api as any, 'abc')
  assert.equal(calls.length, 1, 'debounce collapses bursts')
  assert.equal((calls[0].args as { query: string }).query, 'abc')
  assert.equal(useRecallStore.getState().results[0].chunk_text, 'final')
})

test('RECALL_DEBOUNCE_MS is the documented 200ms value', () => {
  assert.equal(RECALL_DEBOUNCE_MS, 200)
})
