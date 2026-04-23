// WI-10 closing — shell-side tests for the bases plugin. Three areas:
//
//   1. Soft-delete + restore IPC routing — the kernel client wraps
//      the seventeen `base_*` handlers; these tests assert that
//      `softDeleteRecord` / `restoreRecord` reach the right plugin
//      with the right command + args (the plugin's UI now uses both,
//      where pre-WI-10 they were defined-but-unused).
//
//   2. Trash mode store-state — toggling `trashOpen` resets selection
//      and lets `BasesView` flip the visible-records filter. A pure
//      store test sidesteps the React tree but locks the contract
//      the view depends on.
//
//   3. Phase 5 view round-trip — `viewFromTabState` now persists
//      `fields` (visible-column allowlist derived from
//      `hiddenFields`) and `filter` (per-view filter chips). These
//      were silently dropped pre-fix; the round-trip test exercises
//      the full save → reload → re-apply path through the in-memory
//      kernel.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/bases-store.test.ts` re-export shim).

import type { Base, FilterRule } from './kernelClient.ts'
import { makeBasesKernelClient, STORAGE_PLUGIN_ID } from './kernelClient.ts'
import { useBasesStore } from './basesStore.ts'
import {
  applyView,
  filtersFromView,
  hiddenFieldsFromView,
  viewFromTabState,
} from './viewMapping.ts'
import {
  inMemoryBaseHandlers,
  makeMockKernel,
  type InMemoryStore,
} from './testKernel.ts'

// @ts-expect-error — shell tsconfig omits @types/node; the test runner has it
import { test } from 'node:test'
// @ts-expect-error
import assert from 'node:assert/strict'

// ── helpers ─────────────────────────────────────────────────────────────────

function resetTab(relpath: string): void {
  // Drop every tab so each test starts from a clean store.
  useBasesStore.setState({ tabs: {} })
  useBasesStore.getState().ensureTab(relpath)
}

function freshBase(path: string): Base {
  return {
    name: path,
    schema: {
      fields: {
        id: { type: 'uuid', primary: true },
        title: { type: 'text' },
        status: { type: 'select', options: ['todo', 'doing', 'done'] },
        notes: { type: 'long-text' },
      },
    },
    records: [
      { id: 'r1', title: 'first', status: 'todo' },
      { id: 'r2', title: 'second', status: 'doing' },
    ],
    views: [],
    relations: [],
    metadata: { version: '1', created_at: 0, modified_at: 0 },
  }
}

function seededStore(path: string): InMemoryStore {
  return { bases: { [path]: freshBase(path) } }
}

// ── 1. Soft-delete / restore IPC routing ────────────────────────────────────

test('softDeleteRecord reaches base_record_soft_delete with the documented args', async () => {
  const path = 'team/work.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  await client.softDeleteRecord(path, 'r1')

  const calls = kernel.callsTo('base_record_soft_delete')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, STORAGE_PLUGIN_ID)
  assert.deepEqual(calls[0].args, { path, record_id: 'r1' })
  // The mock kernel mutated the in-memory record — the audit's missing
  // UI path was the *invocation*, not the kernel side, but a round-trip
  // assertion catches future regressions where the wire shape drifts.
  const r1 = store.bases[path].records.find((r) => r.id === 'r1')
  assert.ok(r1?.deletedAt, 'expected deletedAt to be set after soft-delete')
})

test('restoreRecord reaches base_record_restore and clears deletedAt', async () => {
  const path = 'team/work.bases'
  const store = seededStore(path)
  // Seed a soft-deleted record so restore has something to undo.
  store.bases[path].records[0].deletedAt = 1_700_000_000
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  await client.restoreRecord(path, 'r1')

  const calls = kernel.callsTo('base_record_restore')
  assert.equal(calls.length, 1)
  assert.deepEqual(calls[0].args, { path, record_id: 'r1' })
  const r1 = store.bases[path].records.find((r) => r.id === 'r1')
  assert.equal(r1?.deletedAt, null)
})

test('hard-delete still reaches base_record_delete unchanged (trash "Delete forever" path)', async () => {
  const path = 'team/work.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  await client.deleteRecord(path, 'r2')

  const calls = kernel.callsTo('base_record_delete')
  assert.equal(calls.length, 1)
  assert.deepEqual(calls[0].args, { path, record_id: 'r2' })
  assert.equal(store.bases[path].records.find((r) => r.id === 'r2'), undefined)
})

// ── 2. Trash mode store contract ────────────────────────────────────────────

test('trashOpen defaults to false and toggles via setTrashOpen', () => {
  const path = 'team/work.bases'
  resetTab(path)
  let t = useBasesStore.getState().tabs[path]
  assert.equal(t.trashOpen, false)

  useBasesStore.getState().setTrashOpen(path, true)
  t = useBasesStore.getState().tabs[path]
  assert.equal(t.trashOpen, true)

  useBasesStore.getState().setTrashOpen(path, false)
  t = useBasesStore.getState().tabs[path]
  assert.equal(t.trashOpen, false)
})

test('toggling trashOpen clears the selected record id (live ↔ trash sets are disjoint)', () => {
  const path = 'team/work.bases'
  resetTab(path)
  useBasesStore.getState().setSelectedRecordId(path, 'r1')
  assert.equal(useBasesStore.getState().tabs[path].selectedRecordId, 'r1')

  useBasesStore.getState().setTrashOpen(path, true)
  assert.equal(
    useBasesStore.getState().tabs[path].selectedRecordId,
    null,
    'expected selection to clear when entering trash mode',
  )
})

test('soft-delete patch flips visible-records filter (deletedAt set ⇒ hidden in live view)', () => {
  const path = 'team/work.bases'
  resetTab(path)
  useBasesStore.getState().setBase(path, freshBase(path))
  // Mirror the BasesTable soft-delete success path: kernel mutated,
  // store patches `deletedAt` locally so the next render hides it.
  useBasesStore.getState().patchRecord(path, 'r1', { deletedAt: 1_700_000_000 })
  const records = useBasesStore.getState().tabs[path].base!.records
  const live = records.filter((r) => !r.deletedAt)
  const trash = records.filter((r) => !!r.deletedAt)
  assert.equal(live.length, 1)
  assert.equal(live[0].id, 'r2')
  assert.equal(trash.length, 1)
  assert.equal(trash[0].id, 'r1')
})

// ── 3. View round-trip — fields + filter ────────────────────────────────────

test('viewFromTabState includes `fields` derived from hiddenFields when allFields is supplied', () => {
  const path = 'team/work.bases'
  resetTab(path)
  useBasesStore.getState().setHiddenFields(path, ['notes'])
  const tab = useBasesStore.getState().tabs[path]
  const view = viewFromTabState('Compact', 'table', tab, [
    'title',
    'status',
    'notes',
  ])
  assert.deepEqual(view.fields, ['title', 'status'])
})

test('viewFromTabState omits `fields` when no columns are hidden (kernel allowlist semantics)', () => {
  const path = 'team/work.bases'
  resetTab(path)
  // hiddenFields = null ⇒ "show all"; `view.fields` must be absent so
  // the kernel falls back to the schema field order.
  const tab = useBasesStore.getState().tabs[path]
  const view = viewFromTabState('All', 'table', tab, ['title', 'status', 'notes'])
  assert.equal(view.fields, undefined)
})

test('viewFromTabState includes `filter` rules verbatim with defensive copies', () => {
  const path = 'team/work.bases'
  resetTab(path)
  const rules: FilterRule[] = [
    { field: 'status', operator: 'eq', value: 'todo' },
  ]
  useBasesStore.getState().setViewFilters(path, rules)
  const tab = useBasesStore.getState().tabs[path]
  const view = viewFromTabState('Open', 'table', tab, ['title', 'status'])
  assert.deepEqual(view.filter, rules)
  // Defensive copy — mutating the snapshot must not bleed back into
  // the store.
  view.filter![0].value = 'mutated'
  assert.equal(
    useBasesStore.getState().tabs[path].viewFilters[0].value,
    'todo',
  )
})

test('hiddenFieldsFromView inverts the allowlist against allFields', () => {
  const view = {
    name: 'Compact',
    type: 'table' as const,
    fields: ['title', 'status'],
  }
  assert.deepEqual(
    hiddenFieldsFromView(view, ['title', 'status', 'notes']),
    ['notes'],
  )
  // Empty allowlist ⇒ no hidden columns.
  assert.equal(
    hiddenFieldsFromView({ name: 'All', type: 'table' }, ['title', 'notes']),
    null,
  )
})

test('filtersFromView returns a defensive copy of the filter rules', () => {
  const rules: FilterRule[] = [
    { field: 'status', operator: 'eq', value: 'done' },
  ]
  const out = filtersFromView({ name: 'Done', type: 'table', filter: rules })
  assert.deepEqual(out, rules)
  out[0].value = 'mutated'
  assert.equal(rules[0].value, 'done')
})

test('full round-trip — save view through base_view_update + reload preserves fields + filter', async () => {
  const path = 'team/work.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])
  useBasesStore.getState().setHiddenFields(path, ['notes'])
  useBasesStore.getState().setViewFilters(path, [
    { field: 'status', operator: 'eq', value: 'todo' },
  ])
  useBasesStore.getState().setSort(path, { field: 'title', dir: 'asc' })

  // Save as new — base_view_create the first time round.
  const allFields = Object.keys(store.bases[path].schema.fields).filter(
    (f) => f !== 'id',
  )
  const tab1 = useBasesStore.getState().tabs[path]
  const v1 = viewFromTabState('My view', 'table', tab1, allFields)
  await client.createView(path, v1)

  // Now mutate the tab — change a filter rule + un-hide a column.
  useBasesStore.getState().setHiddenFields(path, [])
  useBasesStore.getState().setViewFilters(path, [
    { field: 'status', operator: 'eq', value: 'doing' },
  ])

  // "Save changes" path — base_view_update.
  const tab2 = useBasesStore.getState().tabs[path]
  const v2 = viewFromTabState('My view', 'table', tab2, allFields)
  await client.updateView(path, v2)

  // base_view_update is no longer dead code.
  const updateCalls = kernel.callsTo('base_view_update')
  assert.equal(updateCalls.length, 1)
  assert.deepEqual(
    (updateCalls[0].args as { view: { filter: FilterRule[] } }).view.filter,
    [{ field: 'status', operator: 'eq', value: 'doing' }],
  )

  // Reload from kernel and apply onto a fresh tab — fields + filter
  // must come back unchanged.
  const reloaded = await client.loadBase(path)
  assert.equal(reloaded.views.length, 1)
  const saved = reloaded.views[0]
  assert.equal(saved.name, 'My view')
  assert.deepEqual(saved.filter, [
    { field: 'status', operator: 'eq', value: 'doing' },
  ])
  // hidden=[] ⇒ no `fields` allowlist.
  assert.equal(saved.fields, undefined)

  // Round-trip with a hidden column too.
  useBasesStore.getState().setHiddenFields(path, ['notes'])
  const tab3 = useBasesStore.getState().tabs[path]
  const v3 = viewFromTabState('My view', 'table', tab3, allFields)
  await client.updateView(path, v3)
  const r2 = await client.loadBase(path)
  assert.deepEqual(r2.views[0].fields, ['title', 'status'])

  // applyView re-derives the hidden list and filter set on a fresh
  // tab (mirror of the BasesViewBar.handleApply path).
  const otherPath = 'team/other.bases'
  resetTab(otherPath)
  const applied = applyView(r2.views[0])
  assert.equal(applied.mode, 'table')
  useBasesStore
    .getState()
    .setHiddenFields(otherPath, hiddenFieldsFromView(r2.views[0], allFields))
  useBasesStore.getState().setViewFilters(otherPath, filtersFromView(r2.views[0]))
  const otherTab = useBasesStore.getState().tabs[otherPath]
  assert.deepEqual(otherTab.hiddenFields, ['notes'])
  assert.deepEqual(otherTab.viewFilters, [
    { field: 'status', operator: 'eq', value: 'doing' },
  ])
})
