// Pure-logic + DOM tests for the BL-012 split-2 database-view widget.
// Run via `pnpm --filter nexus-shell test`; the wrapper at
// `shell/tests/database-view-widget.test.ts` re-exports this so the
// default test glob picks it up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type {
  DatabaseViewConfig,
  EditorKernelClient,
  ExecuteDatabaseViewResponse,
} from '../kernelClient.ts'
import {
  DatabaseViewCache,
  DatabaseViewWidget,
  effectiveFields,
  widgetKey,
} from './databaseViewWidget.ts'

// ── fixtures ─────────────────────────────────────────────────────────────────

const TABLE_CONFIG: DatabaseViewConfig = {
  view_type: { kind: 'table' },
  filters: [],
  sorts: [],
  group_by: null,
  hidden_columns: [],
}

function flatResp(records: Array<Record<string, unknown>>): ExecuteDatabaseViewResponse {
  return {
    applied: {
      view_name: 'inline',
      view_type: 'table',
      fields: ['title', 'status'],
      layout: {
        kind: 'flat',
        records: records.map((r, i) => ({ id: r.id ?? `r${i}`, ...r })) as never,
      },
    },
    schema: { version: '1.0', fields: { title: {}, status: {} } },
  }
}

function makeClient(
  resp: ExecuteDatabaseViewResponse | Promise<ExecuteDatabaseViewResponse>,
): { client: EditorKernelClient; calls: Array<{ path: string; config: DatabaseViewConfig }> } {
  const calls: Array<{ path: string; config: DatabaseViewConfig }> = []
  const client = {
    executeDatabaseView(path: string, config: DatabaseViewConfig) {
      calls.push({ path, config })
      return Promise.resolve(resp)
    },
  } as unknown as EditorKernelClient
  return { client, calls }
}

// ── widgetKey ────────────────────────────────────────────────────────────────

test('widgetKey is stable across config key reordering', () => {
  const a = widgetKey('Tasks.bases', {
    view_type: { kind: 'table' },
    filters: ['status = Done'],
    sorts: [],
    group_by: null,
    hidden_columns: [],
  })
  const b = widgetKey('Tasks.bases', {
    hidden_columns: [],
    group_by: null,
    sorts: [],
    filters: ['status = Done'],
    view_type: { kind: 'table' },
  })
  assert.equal(a, b)
})

test('widgetKey distinguishes path, filters, view_type', () => {
  const base = widgetKey('Tasks.bases', TABLE_CONFIG)
  assert.notEqual(base, widgetKey('Other.bases', TABLE_CONFIG))
  assert.notEqual(
    base,
    widgetKey('Tasks.bases', { ...TABLE_CONFIG, filters: ['status = Done'] }),
  )
  assert.notEqual(
    base,
    widgetKey('Tasks.bases', {
      ...TABLE_CONFIG,
      view_type: { kind: 'kanban', column_by: 'status' },
    }),
  )
})

// ── effectiveFields ──────────────────────────────────────────────────────────

test('effectiveFields prefers the view fields list', () => {
  const resp = flatResp([{ id: 'r1', title: 'a', status: 'Done', extra: 1 }])
  assert.deepEqual(effectiveFields(resp), ['title', 'status'])
})

test('effectiveFields falls back to schema field order when view has none', () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: '',
      view_type: 'table',
      fields: [],
      layout: { kind: 'flat', records: [{ id: 'r1', a: 1, b: 2 }] },
    },
    schema: { version: '1.0', fields: { a: {}, b: {}, c: {} } },
  }
  assert.deepEqual(effectiveFields(resp), ['a', 'b', 'c'])
})

test('effectiveFields falls back to record keys (sans id/deletedAt) when nothing else available', () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: '',
      view_type: 'table',
      fields: [],
      layout: {
        kind: 'flat',
        records: [{ id: 'r1', deletedAt: null, title: 'x', priority: 3 }],
      },
    },
    schema: { version: '1.0', fields: {} },
  }
  assert.deepEqual(effectiveFields(resp), ['title', 'priority'])
})

// ── DatabaseViewCache ────────────────────────────────────────────────────────

test('DatabaseViewCache.run dedupes concurrent fetches for the same key', async () => {
  const cache = new DatabaseViewCache()
  let invocations = 0
  const fetcher = () => {
    invocations++
    return Promise.resolve(flatResp([]))
  }
  const [a, b] = await Promise.all([cache.run('k', fetcher), cache.run('k', fetcher)])
  assert.equal(invocations, 1)
  assert.equal(a, b)
  // Subsequent peek returns the resolved response without re-running.
  const peeked = cache.peek('k')
  assert.ok(peeked?.response, 'cache should hold a resolved response')
})

test('DatabaseViewCache.invalidate forces a re-fetch', async () => {
  const cache = new DatabaseViewCache()
  let invocations = 0
  const fetcher = () => {
    invocations++
    return Promise.resolve(flatResp([]))
  }
  await cache.run('k', fetcher)
  cache.invalidate('k')
  await cache.run('k', fetcher)
  assert.equal(invocations, 2)
})

test('DatabaseViewCache.invalidatePath drops every key targeting a base path', async () => {
  const cache = new DatabaseViewCache()
  await cache.run('Tasks.bases {"a":1}', () => Promise.resolve(flatResp([])))
  await cache.run('Tasks.bases {"a":2}', () => Promise.resolve(flatResp([])))
  await cache.run('Other.bases {"a":1}', () => Promise.resolve(flatResp([])))
  assert.equal(cache.size(), 3)

  const dropped = cache.invalidatePath('Tasks.bases')
  assert.equal(dropped, 2)
  assert.equal(cache.size(), 1)
  assert.equal(cache.peek('Other.bases {"a":1}')?.response !== undefined, true)
})

test('DatabaseViewCache.invalidatePath returns 0 when nothing matched (no spurious recompute)', async () => {
  const cache = new DatabaseViewCache()
  await cache.run('Tasks.bases {}', () => Promise.resolve(flatResp([])))
  assert.equal(cache.invalidatePath('Other.bases'), 0)
  assert.equal(cache.size(), 1)
})

test('DatabaseViewCache.invalidatePath does not partial-match a longer base path', async () => {
  const cache = new DatabaseViewCache()
  await cache.run('Tasks.bases {}', () => Promise.resolve(flatResp([])))
  await cache.run('Tasks.bases.archive {}', () => Promise.resolve(flatResp([])))
  // Trailing-space terminator on the prefix prevents `Tasks.bases`
  // from also dropping `Tasks.bases.archive`.
  const dropped = cache.invalidatePath('Tasks.bases')
  assert.equal(dropped, 1)
  assert.equal(cache.peek('Tasks.bases.archive {}')?.response !== undefined, true)
})

test('DatabaseViewCache surfaces fetch errors via peek and rejects subsequent run() callers', async () => {
  const cache = new DatabaseViewCache()
  const boom = new Error('storage offline')
  await assert.rejects(
    cache.run('k', () => Promise.reject(boom)),
    /storage offline/,
  )
  const peeked = cache.peek('k')
  assert.ok(peeked?.error, 'error should be cached')
  assert.equal(peeked?.error?.message, 'storage offline')
})

// ── DatabaseViewWidget ───────────────────────────────────────────────────────

test('widget renders pending placeholder synchronously, then fills with the response', async () => {
  const resp = flatResp([
    { id: 'r1', title: 'A', status: 'Done' },
    { id: 'r2', title: 'B', status: 'Todo' },
  ])
  const { client, calls } = makeClient(resp)
  const cache = new DatabaseViewCache()
  const widget = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, { client, cache })

  const root = widget.toDOM()
  // Mount so isConnected becomes true — happy-dom requires a parent.
  document.body.appendChild(root)

  // Synchronous: pending placeholder.
  assert.equal(root.querySelector('.cm-md-dbview-pending')?.textContent, 'Loading…')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].path, 'Tasks.bases')

  // Drain pending microtasks so the IPC promise resolves.
  await Promise.resolve()
  await Promise.resolve()

  const table = root.querySelector('table.cm-md-dbview-table')
  assert.ok(table, 'table should render after fetch resolves')
  const headers = Array.from(table!.querySelectorAll('thead th')).map(
    (th) => th.textContent,
  )
  assert.deepEqual(headers, ['title', 'status'])
  const rows = Array.from(table!.querySelectorAll('tbody tr'))
  assert.equal(rows.length, 2)
  const firstRowCells = Array.from(rows[0].querySelectorAll('td')).map(
    (td) => td.textContent,
  )
  assert.deepEqual(firstRowCells, ['A', 'Done'])

  document.body.removeChild(root)
})

test('widget hits the cache on second toDOM for the same key (no extra IPC)', async () => {
  const resp = flatResp([{ id: 'r1', title: 'A', status: 'Done' }])
  const { client, calls } = makeClient(resp)
  const cache = new DatabaseViewCache()
  const w1 = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, { client, cache })
  const root1 = w1.toDOM()
  document.body.appendChild(root1)
  await Promise.resolve()
  await Promise.resolve()
  document.body.removeChild(root1)

  // Same path + config — cached. New widget instance simulates a
  // decoration rebuild after a selection move.
  const w2 = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, { client, cache })
  const root2 = w2.toDOM()
  document.body.appendChild(root2)

  // Cached response is rendered synchronously — no pending state.
  assert.equal(
    root2.querySelector('.cm-md-dbview-pending'),
    null,
    'cache hit should skip the pending placeholder',
  )
  assert.ok(
    root2.querySelector('table.cm-md-dbview-table'),
    'cache hit should render the table immediately',
  )
  assert.equal(calls.length, 1, 'IPC should run exactly once across both widgets')

  document.body.removeChild(root2)
})

test('widget renders an error box and routes to onError when the IPC rejects', async () => {
  const calls: Array<{ msg: string; err: unknown }> = []
  const failingClient = {
    executeDatabaseView: () => Promise.reject(new Error('not found: Tasks.bases')),
  } as unknown as EditorKernelClient
  const widget = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, {
    client: failingClient,
    cache: new DatabaseViewCache(),
    onError: (msg, err) => calls.push({ msg, err }),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)

  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()

  const errBox = root.querySelector('.cm-md-dbview-error')
  assert.ok(errBox, 'error box should replace pending')
  assert.equal(
    errBox!.querySelector('.cm-md-dbview-error-msg')?.textContent,
    'not found: Tasks.bases',
  )
  assert.equal(calls.length, 1)
  assert.match(calls[0].msg, /execute_database_view failed/)

  document.body.removeChild(root)
})

test('grouped layout renders one section per group with record counts in the heading', async () => {
  const grouped: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: 'board',
      view_type: 'kanban',
      fields: ['title'],
      layout: {
        kind: 'grouped',
        groups: [
          { key: 'Doing', records: [{ id: 'r1', title: 'A' }] },
          {
            key: 'Done',
            records: [
              { id: 'r2', title: 'B' },
              { id: 'r3', title: 'C' },
            ],
          },
        ],
      },
    },
    schema: { version: '1.0', fields: { title: {} } },
  }
  const { client } = makeClient(grouped)
  const widget = new DatabaseViewWidget(
    'Board.bases',
    { ...TABLE_CONFIG, view_type: { kind: 'kanban', column_by: 'status' } },
    { client, cache: new DatabaseViewCache() },
  )
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()

  const sections = root.querySelectorAll('section.cm-md-dbview-group')
  assert.equal(sections.length, 2)
  const headings = Array.from(sections).map(
    (s) => s.querySelector('.cm-md-dbview-group-heading')?.textContent,
  )
  assert.deepEqual(headings, ['Doing (1)', 'Done (2)'])

  document.body.removeChild(root)
})

test('two widgets with the same key are .eq()', () => {
  const w1 = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, {
    client: {} as EditorKernelClient,
  })
  const w2 = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, {
    client: {} as EditorKernelClient,
  })
  const w3 = new DatabaseViewWidget('Other.bases', TABLE_CONFIG, {
    client: {} as EditorKernelClient,
  })
  assert.equal(w1.eq(w2), true)
  assert.equal(w1.eq(w3), false)
})
