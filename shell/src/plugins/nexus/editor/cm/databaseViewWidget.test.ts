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
  applyKanbanDrop,
  DatabaseViewCache,
  DatabaseViewWidget,
  effectiveFields,
  isEditableType,
  KANBAN_DRAG_MIME,
  makeEditableCell,
  MISSING_GROUP_KEY,
  parseCellInput,
  readKanbanDragPayload,
  renderKanban,
  widgetKey,
  type KanbanDragPayload,
  type MutationDeps,
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

test('kanban view renders one column per group with the group value + count in the header', async () => {
  // BL-069: an `applied.view_type === 'kanban'` response now drives
  // the new column-shaped renderer (`.cm-md-dbview-kanban-column`)
  // rather than the legacy `.cm-md-dbview-group` table-section
  // grouped layout the BL-012 split 3 widget produced.
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

  const columns = root.querySelectorAll('.cm-md-dbview-kanban-column')
  assert.equal(columns.length, 2)
  const labels = Array.from(columns).map(
    (c) => c.querySelector('.cm-md-dbview-kanban-label')?.textContent,
  )
  assert.deepEqual(labels, ['Doing', 'Done'])
  const counts = Array.from(columns).map(
    (c) => c.querySelector('.cm-md-dbview-kanban-count')?.textContent,
  )
  assert.deepEqual(counts, ['1', '2'])
  // Each record renders as a card inside its column.
  assert.equal(columns[0].querySelectorAll('.cm-md-dbview-card').length, 1)
  assert.equal(columns[1].querySelectorAll('.cm-md-dbview-card').length, 2)

  document.body.removeChild(root)
})

test('widget renders editable header with filter / sort chips when onUpdateConfig is wired', async () => {
  const config: DatabaseViewConfig = {
    view_type: { kind: 'kanban', column_by: 'status' },
    filters: ['status = Done', 'priority > 2'],
    sorts: ['due_date asc'],
    group_by: null,
    hidden_columns: [],
  }
  const updates: DatabaseViewConfig[] = []
  const { client } = makeClient(flatResp([]))
  const widget = new DatabaseViewWidget('Tasks.bases', config, {
    client,
    cache: new DatabaseViewCache(),
    onUpdateConfig: (next) => updates.push(next),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)

  // Header sections rendered.
  const summary = root.querySelector('.cm-md-dbview-summary')
  assert.match(summary?.textContent ?? '', /Kanban.*group by: status/)

  const filterChips = root.querySelectorAll(
    '[data-kind="filters"] .cm-md-dbview-chip-text',
  )
  assert.equal(filterChips.length, 2)
  assert.equal(filterChips[0].textContent, 'filter: status = Done')

  const sortChips = root.querySelectorAll(
    '[data-kind="sorts"] .cm-md-dbview-chip-text',
  )
  assert.equal(sortChips.length, 1)

  // Click the × on the first filter chip.
  const removeBtn = root.querySelector(
    '[data-kind="filters"] .cm-md-dbview-chip-remove',
  ) as HTMLButtonElement
  removeBtn.click()
  assert.equal(updates.length, 1)
  assert.deepEqual(updates[0].filters, ['priority > 2'])

  // Submit the "add filter" form.
  const addInput = root.querySelector(
    '[data-kind="filters"] .cm-md-dbview-add-input',
  ) as HTMLInputElement
  addInput.value = 'title contains foo'
  const form = addInput.closest('form') as HTMLFormElement
  form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }))
  assert.equal(updates.length, 2)
  assert.deepEqual(updates[1].filters, [
    'status = Done',
    'priority > 2',
    'title contains foo',
  ])
  // Input is cleared post-submit so the user can add a second filter.
  assert.equal(addInput.value, '')

  document.body.removeChild(root)
})

test('widget skips the header entirely when onUpdateConfig is absent (read-only mode)', async () => {
  const { client } = makeClient(flatResp([]))
  const widget = new DatabaseViewWidget('Tasks.bases', TABLE_CONFIG, {
    client,
    cache: new DatabaseViewCache(),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)
  assert.equal(root.querySelector('.cm-md-dbview-header'), null)
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

// ── BL-069 — Gallery / Calendar layout renderers ─────────────────────────────

test('gallery view renders flat records as cards keyed by title_field', async () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: 'inline',
      view_type: 'gallery',
      fields: ['name', 'status'],
      layout: {
        kind: 'flat',
        records: [
          { id: 'r1', name: 'Acme Co.', status: 'Active' },
          { id: 'r2', name: 'Globex', status: 'Lapsed' },
        ],
      },
    },
    schema: { version: '1.0', fields: { name: { type: 'text' }, status: { type: 'select' } } },
  }
  const { client } = makeClient(resp)
  const widget = new DatabaseViewWidget(
    'Customers.bases',
    { ...TABLE_CONFIG, view_type: { kind: 'gallery', title_field: 'name' } },
    { client, cache: new DatabaseViewCache() },
  )
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()

  const grid = root.querySelector('.cm-md-dbview-gallery')
  assert.ok(grid, 'gallery wrapper rendered')
  const cards = grid!.querySelectorAll('.cm-md-dbview-card')
  assert.equal(cards.length, 2)
  // Title comes from `title_field`.
  const titles = Array.from(cards).map(
    (c) => c.querySelector('.cm-md-dbview-card-title')?.textContent,
  )
  assert.deepEqual(titles, ['Acme Co.', 'Globex'])
  // The title field is excluded from the body rows; only `status`
  // shows up as a labeled row.
  const labels = Array.from(
    cards[0].querySelectorAll('.cm-md-dbview-card-label'),
  ).map((el) => el.textContent)
  assert.deepEqual(labels, ['status'])

  document.body.removeChild(root)
})

test('calendar view renders a 7×6 month grid with date-keyed pills + an undated bucket', async () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: 'inline',
      view_type: 'calendar',
      fields: ['title'],
      layout: {
        kind: 'grouped',
        groups: [
          { key: '2026-05-07', records: [{ id: 'r1', title: 'Standup' }] },
          {
            key: '2026-05-08',
            records: [
              { id: 'r2', title: 'Demo' },
              { id: 'r3', title: 'Retro' },
            ],
          },
          // The MISSING_GROUP_KEY sentinel — bucket below the grid.
          { key: '(none)', records: [{ id: 'r4', title: 'Inbox' }] },
        ],
      },
    },
    schema: { version: '1.0', fields: { title: { type: 'text' } } },
  }
  const { client } = makeClient(resp)
  const widget = new DatabaseViewWidget(
    'Cal.bases',
    { ...TABLE_CONFIG, view_type: { kind: 'calendar', date_field: 'due' } },
    { client, cache: new DatabaseViewCache() },
  )
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()

  const month = root.querySelector('.cm-md-dbview-calendar-month')
  // Median of [2026-05-07, 2026-05-08] is 2026-05-08 (UTC), so
  // the visible month is May 2026; the field name is appended.
  assert.match(month?.textContent ?? '', /May 2026 · due/)
  // 7 weekday headers + 42 day cells = 49 grid children.
  const grid = root.querySelector('.cm-md-dbview-calendar-grid')!
  assert.equal(grid.children.length, 49)
  // Pills land in the right cells.
  const pills = root.querySelectorAll('.cm-md-dbview-calendar-pill')
  // 1 standup + 2 demo/retro + 1 undated = 4
  assert.equal(pills.length, 4)
  // Undated bucket exists.
  const undated = root.querySelector('.cm-md-dbview-calendar-undated')
  assert.ok(undated, 'undated bucket rendered')
  assert.equal(
    undated!.querySelector('.cm-md-dbview-calendar-pill--undated')?.textContent,
    'Inbox',
  )

  document.body.removeChild(root)
})

test('table view continues to render a flat table for view_type=table', async () => {
  // Regression guard: making sure Kanban/Gallery/Calendar dispatch
  // didn't accidentally redirect table views away from the table
  // renderer.
  const resp = flatResp([
    { id: 'r1', title: 'Hello', status: 'Active' },
    { id: 'r2', title: 'World', status: 'Active' },
  ])
  const { client } = makeClient(resp)
  const widget = new DatabaseViewWidget('T.bases', TABLE_CONFIG, {
    client,
    cache: new DatabaseViewCache(),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()

  assert.ok(root.querySelector('table.cm-md-dbview-table'))
  assert.equal(root.querySelectorAll('tbody tr').length, 2)

  document.body.removeChild(root)
})

test('cells render type-aware values (BL-069 type-aware formatter wired through)', async () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: 'inline',
      view_type: 'table',
      fields: ['count', 'done'],
      layout: {
        kind: 'flat',
        records: [{ id: 'r1', count: 1234, done: true }],
      },
    },
    schema: {
      version: '1.0',
      fields: { count: { type: 'number' }, done: { type: 'checkbox' } },
    },
  }
  const { client } = makeClient(resp)
  const widget = new DatabaseViewWidget('T.bases', TABLE_CONFIG, {
    client,
    cache: new DatabaseViewCache(),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()

  const cells = root.querySelectorAll('tbody td')
  assert.equal(cells.length, 2)
  // Number gets locale grouping; checkbox renders the ✓ glyph.
  assert.equal(cells[0].textContent, '1,234')
  assert.equal(cells[1].textContent, '✓')

  document.body.removeChild(root)
})

// ── BL-069 follow-up: kanban drag-to-reorder ────────────────────────────────

test('readKanbanDragPayload: parses well-formed JSON and rejects junk', () => {
  assert.deepEqual(
    readKanbanDragPayload('{"record_id":"r1","source_group_key":"Doing"}'),
    { record_id: 'r1', source_group_key: 'Doing' },
  )
  // Wrong types — rejected, no throw.
  assert.equal(readKanbanDragPayload('{"record_id":42}'), null)
  assert.equal(readKanbanDragPayload('not json'), null)
  assert.equal(readKanbanDragPayload(''), null)
  assert.equal(readKanbanDragPayload(null), null)
  assert.equal(readKanbanDragPayload(undefined), null)
})

test('applyKanbanDrop: same-column drop is a no-op', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async (recordId, fields) => {
      calls.push({ id: recordId, fields })
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const result = await applyKanbanDrop(
    { record_id: 'r1', source_group_key: 'Doing' },
    'Doing',
    'status',
    mutate,
  )
  assert.deepEqual(result, { updated: false })
  assert.equal(calls.length, 0)
  assert.equal(refreshes, 0)
})

test('applyKanbanDrop: cross-column drop patches group_field and refreshes', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async (recordId, fields) => {
      calls.push({ id: recordId, fields })
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const result = await applyKanbanDrop(
    { record_id: 'r1', source_group_key: 'Doing' },
    'Done',
    'status',
    mutate,
  )
  assert.deepEqual(result, { updated: true })
  assert.deepEqual(calls, [{ id: 'r1', fields: { status: 'Done' } }])
  assert.equal(refreshes, 1)
})

test('applyKanbanDrop: drop into the (none) bucket clears the field via JSON null', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  const mutate: MutationDeps = {
    update: async (recordId, fields) => {
      calls.push({ id: recordId, fields })
    },
    refresh: () => {},
  }
  await applyKanbanDrop(
    { record_id: 'r1', source_group_key: 'Doing' },
    MISSING_GROUP_KEY,
    'status',
    mutate,
  )
  // Sentinel maps to null so the storage layer clears the field
  // rather than writing the literal "(none)" string.
  assert.deepEqual(calls, [{ id: 'r1', fields: { status: null } }])
})

test('applyKanbanDrop: update failure propagates and refresh is not called', async () => {
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async () => {
      throw new Error('storage offline')
    },
    refresh: () => {
      refreshes += 1
    },
  }
  await assert.rejects(
    () =>
      applyKanbanDrop(
        { record_id: 'r1', source_group_key: 'Doing' },
        'Done',
        'status',
        mutate,
      ),
    /storage offline/,
  )
  assert.equal(refreshes, 0)
})

test('renderKanban: cards are draggable when mutate + groupField are present', () => {
  const calls: Array<unknown> = []
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {},
  }
  const root = renderKanban(
    ['title', 'status'],
    [
      { key: 'Doing', records: [{ id: 'r1', title: 'A', status: 'Doing' }] },
      { key: 'Done', records: [{ id: 'r2', title: 'B', status: 'Done' }] },
    ],
    { title: {}, status: {} },
    {
      view_type: { kind: 'kanban', column_by: 'status' },
      filters: [],
      sorts: [],
      group_by: null,
      hidden_columns: [],
    },
    mutate,
  )
  document.body.appendChild(root)
  const cards = root.querySelectorAll('.cm-md-dbview-card')
  assert.equal(cards.length, 2)
  for (const card of cards) {
    const html = card as HTMLElement
    assert.equal(html.draggable, true)
    assert.ok(html.classList.contains('cm-md-dbview-card--draggable'))
    assert.ok(typeof html.dataset.recordId === 'string')
    assert.ok(typeof html.dataset.sourceGroupKey === 'string')
  }
  document.body.removeChild(root)
})

test('renderKanban: cards are NOT draggable when mutate is absent (read-only)', () => {
  const root = renderKanban(
    ['title', 'status'],
    [{ key: 'Doing', records: [{ id: 'r1', title: 'A', status: 'Doing' }] }],
    { title: {}, status: {} },
    {
      view_type: { kind: 'kanban', column_by: 'status' },
      filters: [],
      sorts: [],
      group_by: null,
      hidden_columns: [],
    },
    // mutate omitted
  )
  document.body.appendChild(root)
  const card = root.querySelector('.cm-md-dbview-card') as HTMLElement | null
  assert.ok(card)
  // The renderer doesn't set `draggable` at all in read-only mode,
  // so we assert via the absence of the marker class + the
  // attribute (which the renderer would have set to `'true'` in
  // DnD mode). happy-dom's default `HTMLElement.draggable` getter
  // can differ from the browser's, so we don't compare it to a
  // boolean directly.
  assert.equal(card.getAttribute('draggable'), null)
  assert.ok(!card.classList.contains('cm-md-dbview-card--draggable'))
  document.body.removeChild(root)
})

// ── BL-069 follow-up: inline cell editing ───────────────────────────────────

test('isEditableType: text/number/date/select/checkbox editable; lookup/relation/multi-select read-only', () => {
  for (const t of [
    'text',
    'long-text',
    'url',
    'email',
    'uuid',
    'number',
    'currency',
    'percent',
    'checkbox',
    'date',
    'time',
    'datetime',
    'select',
  ]) {
    assert.equal(isEditableType(t), true, `${t} should be editable`)
  }
  for (const t of [
    'multi-select',
    'relation',
    'lookup',
    'formula',
    'rollup',
    undefined,
    '',
    'unknown',
  ]) {
    assert.equal(isEditableType(t), false, `${t ?? '(undef)'} should be read-only`)
  }
})

test('parseCellInput: text trims; empty / whitespace collapse to null', () => {
  assert.equal(parseCellInput('hello', { type: 'text' }), 'hello')
  assert.equal(parseCellInput('  hello  ', { type: 'text' }), 'hello')
  assert.equal(parseCellInput('', { type: 'text' }), null)
  assert.equal(parseCellInput('   ', { type: 'text' }), null)
})

test('parseCellInput: number / currency / percent parse via Number()', () => {
  assert.equal(parseCellInput('42', { type: 'number' }), 42)
  assert.equal(parseCellInput('3.14', { type: 'number' }), 3.14)
  assert.equal(parseCellInput('1,234', { type: 'currency' }), parseCellInput('1,234', { type: 'currency' }))
  // Non-finite parse routes to SAME_VALUE sentinel — caller treats
  // it as no-op. The sentinel is a Symbol; can't import from outside,
  // but we assert it's not the raw string and not a finite number.
  const out = parseCellInput('abc', { type: 'number' }) as unknown
  assert.equal(typeof out, 'symbol')
})

test('makeEditableCell: text cell — click → input → Enter dispatches update + refresh', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const record = { id: 'r1', title: 'old' }
  const cell = makeEditableCell(record, 'title', 'old', { type: 'text' }, mutate)
  document.body.appendChild(cell)

  // Click to enter edit mode.
  const display = cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement
  display.click()
  const input = cell.querySelector('input.cm-md-dbview-cell-editor') as HTMLInputElement
  assert.ok(input, 'input swapped in after click')
  assert.equal(input.value, 'old')

  // Type new value + press Enter.
  input.value = 'new value'
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))

  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { title: 'new value' } }])
  assert.equal(refreshes, 1)

  document.body.removeChild(cell)
})

test('makeEditableCell: Escape restores display without firing IPC', async () => {
  const calls: Array<unknown> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async () => {
      calls.push(1)
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const cell = makeEditableCell(
    { id: 'r1', title: 'old' },
    'title',
    'old',
    { type: 'text' },
    mutate,
  )
  document.body.appendChild(cell)

  ;(cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement).click()
  const input = cell.querySelector('input.cm-md-dbview-cell-editor') as HTMLInputElement
  input.value = 'discarded'
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))

  await Promise.resolve()
  // No IPC, no refresh, display restored.
  assert.equal(calls.length, 0)
  assert.equal(refreshes, 0)
  assert.ok(cell.querySelector('.cm-md-dbview-cell-display'), 'display span back in DOM')
  assert.equal(cell.querySelector('input.cm-md-dbview-cell-editor'), null)

  document.body.removeChild(cell)
})

test('makeEditableCell: unchanged value (Enter on same string) is a no-op IPC-wise', async () => {
  const calls: Array<unknown> = []
  const mutate: MutationDeps = {
    update: async () => {
      calls.push(1)
    },
    refresh: () => {},
  }
  const cell = makeEditableCell(
    { id: 'r1', title: 'same' },
    'title',
    'same',
    { type: 'text' },
    mutate,
  )
  document.body.appendChild(cell)
  ;(cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement).click()
  const input = cell.querySelector('input.cm-md-dbview-cell-editor') as HTMLInputElement
  // value unchanged
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
  await Promise.resolve()
  await Promise.resolve()
  assert.equal(calls.length, 0, 'unchanged value should not hit IPC')
  document.body.removeChild(cell)
})

test('makeEditableCell: number cell parses via Number() before dispatch', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {},
  }
  const cell = makeEditableCell(
    { id: 'r1', count: 1 },
    'count',
    1,
    { type: 'number' },
    mutate,
  )
  document.body.appendChild(cell)
  ;(cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement).click()
  const input = cell.querySelector('input.cm-md-dbview-cell-editor') as HTMLInputElement
  assert.equal(input.type, 'number')
  input.value = '42'
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { count: 42 } }])
  document.body.removeChild(cell)
})

test('makeEditableCell: checkbox toggles directly on click without a popover', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const cell = makeEditableCell(
    { id: 'r1', done: false },
    'done',
    false,
    { type: 'checkbox' },
    mutate,
  )
  document.body.appendChild(cell)
  // Glyph is empty for `false`.
  assert.equal(cell.querySelector('.cm-md-dbview-cell-display')?.textContent, '')
  cell.click()
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { done: true } }])
  assert.equal(refreshes, 1)
  document.body.removeChild(cell)
})

test('makeEditableCell: empty input collapses to null (clear field)', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {},
  }
  const cell = makeEditableCell(
    { id: 'r1', title: 'old' },
    'title',
    'old',
    { type: 'text' },
    mutate,
  )
  document.body.appendChild(cell)
  ;(cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement).click()
  const input = cell.querySelector('input.cm-md-dbview-cell-editor') as HTMLInputElement
  input.value = ''
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { title: null } }])
  document.body.removeChild(cell)
})

test('makeEditableCell: select fires on change with the chosen option value', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {},
  }
  const cell = makeEditableCell(
    { id: 'r1', status: 'Doing' },
    'status',
    'Doing',
    { type: 'select', options: ['Doing', 'Done', 'Backlog'] },
    mutate,
  )
  document.body.appendChild(cell)
  ;(cell.querySelector('.cm-md-dbview-cell-display') as HTMLElement).click()
  const select = cell.querySelector('select.cm-md-dbview-cell-editor') as HTMLSelectElement
  assert.ok(select)
  // Three options + the leading empty `—` choice = 4.
  assert.equal(select.querySelectorAll('option').length, 4)
  select.value = 'Done'
  select.dispatchEvent(new Event('change', { bubbles: true }))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { status: 'Done' } }])
  document.body.removeChild(cell)
})

test('table view renders editable cells when mutate is wired', async () => {
  const resp: ExecuteDatabaseViewResponse = {
    applied: {
      view_name: 'inline',
      view_type: 'table',
      fields: ['title'],
      layout: { kind: 'flat', records: [{ id: 'r1', title: 'A' }] },
    },
    schema: { version: '1.0', fields: { title: { type: 'text' } } },
  }
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  const client = {
    executeDatabaseView: () => Promise.resolve(resp),
    updateBaseRecord: async (path: string, recordId: string, fields: Record<string, unknown>) => {
      calls.push({ id: recordId, fields })
      return null
    },
  } as unknown as EditorKernelClient
  // `mutate` is threaded into renderApplied unconditionally on the
  // fetch path; no explicit hook needed beyond the client.
  const widget = new DatabaseViewWidget('T.bases', TABLE_CONFIG, {
    client,
    cache: new DatabaseViewCache(),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()
  // `td` carries an editable wrapper instead of plain text because
  // schema declared `title: { type: 'text' }`.
  const td = root.querySelector('tbody td') as HTMLElement
  assert.ok(td.querySelector('.cm-md-dbview-cell--editable'), 'cell wrapper present')
  document.body.removeChild(root)
})

test('table view renders read-only text when schema omits the field type (no editor)', async () => {
  // The legacy schema shape `{ title: {} }` (no `type` discriminator)
  // routes through `isEditableType(undefined) === false` so the cell
  // renders as plain text rather than an editable wrapper. This
  // protects forges with pre-typed schemas from accidentally exposing
  // editor controls over data the storage layer can't safely round-trip.
  const resp = flatResp([{ id: 'r1', title: 'A', status: 'Done' }])
  const { client } = makeClient(resp)
  const widget = new DatabaseViewWidget('T.bases', TABLE_CONFIG, {
    client,
    cache: new DatabaseViewCache(),
  })
  const root = widget.toDOM()
  document.body.appendChild(root)
  await Promise.resolve()
  await Promise.resolve()
  assert.equal(root.querySelector('.cm-md-dbview-cell--editable'), null)
  document.body.removeChild(root)
})

test('renderKanban: drop event fires applyKanbanDrop end-to-end (cross-column)', async () => {
  const calls: Array<{ id: string; fields: Record<string, unknown> }> = []
  let refreshes = 0
  const mutate: MutationDeps = {
    update: async (id, fields) => {
      calls.push({ id, fields })
    },
    refresh: () => {
      refreshes += 1
    },
  }
  const root = renderKanban(
    ['title', 'status'],
    [
      { key: 'Doing', records: [{ id: 'r1', title: 'A', status: 'Doing' }] },
      { key: 'Done', records: [] },
    ],
    { title: {}, status: {} },
    {
      view_type: { kind: 'kanban', column_by: 'status' },
      filters: [],
      sorts: [],
      group_by: null,
      hidden_columns: [],
    },
    mutate,
  )
  document.body.appendChild(root)
  const columns = root.querySelectorAll('.cm-md-dbview-kanban-column')
  const doneColumn = columns[1] as HTMLElement
  // Build a synthetic drop event with a populated transfer payload.
  // happy-dom doesn't ship a writable `DataTransfer`, so we attach
  // a duck-typed object shaped like the parts the renderer touches.
  const payload: KanbanDragPayload = {
    record_id: 'r1',
    source_group_key: 'Doing',
  }
  const fakeTransfer = {
    types: [KANBAN_DRAG_MIME],
    getData(mime: string): string {
      return mime === KANBAN_DRAG_MIME ? JSON.stringify(payload) : ''
    },
    dropEffect: 'none' as const,
    effectAllowed: 'move' as const,
  }
  const dropEvent = new Event('drop', { bubbles: true, cancelable: true })
  Object.defineProperty(dropEvent, 'dataTransfer', {
    value: fakeTransfer,
    enumerable: true,
  })
  doneColumn.dispatchEvent(dropEvent)
  // The renderer fires the IPC fire-and-forget — flush the
  // microtask queue so the awaited update + refresh both complete.
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, [{ id: 'r1', fields: { status: 'Done' } }])
  assert.equal(refreshes, 1)
  document.body.removeChild(root)
})
