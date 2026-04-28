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

import type { Base, BaseRecord, BaseView, FilterRule } from './kernelClient.ts'
import { makeBasesKernelClient, STORAGE_PLUGIN_ID } from './kernelClient.ts'
import { useBasesStore, type HistoryEntry } from './basesStore.ts'
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
import {
  getActiveBases,
  setActiveBases,
  withActiveBases,
} from './activeBases.ts'
import { UNDO_HISTORY_CAP } from '../constants.ts'

import { test } from 'node:test'
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

// ── ADR 0019 — Obsidian `.base` read-only support ───────────────────────────

test('setReadOnly stores the flag and unsupported-filter list per tab', () => {
  const path = 'reading.base'
  useBasesStore.setState({ tabs: {} })
  useBasesStore.getState().ensureTab(path)
  useBasesStore.getState().setReadOnly(path, true, ['formula(x) > 1'])
  const tab = useBasesStore.getState().tabs[path]
  assert.equal(tab.readOnly, true)
  assert.deepEqual(tab.unsupportedFilters, ['formula(x) > 1'])
})

test('loadObsidianBase routes to obsidian_base_query and adapts the result', async () => {
  // The mock kernel returns the wire shape; the adapter must build
  // a Base whose schema columns, records, and views are all derived
  // from the IPC response.
  const calls: Array<{ command: string; args: unknown }> = []
  const kernel = {
    invoke: async (_pluginId: string, command: string, args: unknown) => {
      calls.push({ command, args })
      return {
        columns: ['file.name', 'title', 'year'],
        display_names: { title: 'Title', year: 'Year' },
        rows: [
          { id: 'books/dune.md', fields: { 'file.name': 'dune', title: 'Dune', year: 1965 } },
        ],
        views: [
          {
            name: 'Library',
            type: 'table',
            order: ['title', 'year'],
            sort: [{ property: 'year', direction: 'DESC' }],
          },
        ],
        unsupported_filters: ['formula(x) > 1'],
      } as unknown
    },
    subscribe: () => () => {},
  }
  const client = makeBasesKernelClient(
    kernel as unknown as Parameters<typeof makeBasesKernelClient>[0],
  )
  const load = await client.loadObsidianBase('books/reading.base')

  assert.equal(calls.length, 1)
  assert.equal(calls[0].command, 'obsidian_base_query')
  assert.deepEqual(calls[0].args, { path: 'books/reading.base' })

  assert.equal(load.base.name, 'reading')
  assert.deepEqual(Object.keys(load.base.schema.fields), ['file.name', 'title', 'year'])
  // displayName overrides flow into the synthesized field def so the
  // table header can render the friendly label.
  assert.equal(
    (load.base.schema.fields.title as { displayName?: string }).displayName,
    'Title',
  )
  assert.equal(load.base.records.length, 1)
  assert.equal(load.base.records[0].id, 'books/dune.md')
  assert.equal(load.base.records[0].title, 'Dune')
  // Sort direction round-trips lower-case so it matches the existing
  // `BaseView.sort.direction` contract used by the view layer.
  assert.equal(load.base.views[0].sort?.[0].direction, 'desc')
  assert.deepEqual(load.unsupportedFilters, ['formula(x) > 1'])
})

// ── BL-030 — per-surface undo/redo history stack ────────────────────────────

/** Build a no-op history entry that records its forward/inverse fires
 *  in the supplied counter map so tests can assert ordering. */
function counterEntry(label: string, counts: Map<string, number>): HistoryEntry {
  return {
    label,
    forward: async () => {
      counts.set(`${label}:forward`, (counts.get(`${label}:forward`) ?? 0) + 1)
    },
    inverse: async () => {
      counts.set(`${label}:inverse`, (counts.get(`${label}:inverse`) ?? 0) + 1)
    },
  }
}

test('pushHistory caps at UNDO_HISTORY_CAP and drops the oldest entry', () => {
  const path = 'team/cap.bases'
  resetTab(path)
  const counts = new Map<string, number>()
  // Push CAP+5 entries — the first five should be evicted.
  for (let i = 0; i < UNDO_HISTORY_CAP + 5; i += 1) {
    useBasesStore.getState().pushHistory(path, counterEntry(`e${i}`, counts))
  }
  const stack = useBasesStore.getState().tabs[path].undoStack
  assert.equal(stack.length, UNDO_HISTORY_CAP)
  // First retained entry should be e5 (entries 0..4 were dropped).
  assert.equal(stack[0].label, 'e5')
  assert.equal(stack[stack.length - 1].label, `e${UNDO_HISTORY_CAP + 5 - 1}`)
})

test('undo runs inverse and moves the entry to redoStack; new edit clears redoStack', async () => {
  const path = 'team/redo.bases'
  resetTab(path)
  const counts = new Map<string, number>()
  useBasesStore.getState().pushHistory(path, counterEntry('a', counts))
  useBasesStore.getState().pushHistory(path, counterEntry('b', counts))

  // Undo the last entry; its inverse runs and it moves to redo.
  const ok = await useBasesStore.getState().undo(path)
  assert.equal(ok, true)
  assert.equal(counts.get('b:inverse'), 1)
  let t = useBasesStore.getState().tabs[path]
  assert.equal(t.undoStack.length, 1)
  assert.equal(t.redoStack.length, 1)
  assert.equal(t.redoStack[0].label, 'b')

  // A fresh push clears redo (LIFO undo semantics).
  useBasesStore.getState().pushHistory(path, counterEntry('c', counts))
  t = useBasesStore.getState().tabs[path]
  assert.equal(t.redoStack.length, 0)
  assert.equal(t.undoStack.length, 2)
  assert.equal(t.undoStack[t.undoStack.length - 1].label, 'c')
})

test('cell-edit history round-trips through the mock kernel calling base_record_update with the right args', async () => {
  const path = 'team/edit.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  // Mirror BasesTable.commitEdit — capture prev, kernel write, push entry.
  const prev = useBasesStore
    .getState()
    .tabs[path].base!.records.find((r) => r.id === 'r1')!.title
  await client.updateRecord(path, 'r1', { title: 'edited' })
  useBasesStore.getState().patchRecord(path, 'r1', { title: 'edited' })
  useBasesStore.getState().pushHistory(path, {
    label: 'Edit title',
    forward: async () => {
      await client.updateRecord(path, 'r1', { title: 'edited' })
      useBasesStore.getState().patchRecord(path, 'r1', { title: 'edited' })
    },
    inverse: async () => {
      await client.updateRecord(path, 'r1', { title: prev })
      useBasesStore.getState().patchRecord(path, 'r1', { title: prev })
    },
  })

  // Undo: kernel writes prev value back.
  const ok = await useBasesStore.getState().undo(path)
  assert.equal(ok, true)
  const updates = kernel.callsTo('base_record_update')
  // 1 forward + 1 inverse = 2 updates.
  assert.equal(updates.length, 2)
  assert.deepEqual(updates[0].args, { path, record_id: 'r1', fields: { title: 'edited' } })
  assert.deepEqual(updates[1].args, { path, record_id: 'r1', fields: { title: 'first' } })
  assert.equal(store.bases[path].records[0].title, 'first')
})

test('add-row history: undo deletes the row; redo recreates with the same id', async () => {
  const path = 'team/add.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  // Mirror BasesTable.handleAddRow — kernel mints id, then push entry.
  const stored = await client.createRecord(path, { id: '', title: 'new' } as BaseRecord)
  useBasesStore.getState().appendRecord(path, stored)
  useBasesStore.getState().pushHistory(path, {
    label: 'Add row',
    forward: async () => {
      await client.createRecord(path, stored)
      useBasesStore.getState().appendRecord(path, stored)
    },
    inverse: async () => {
      await client.deleteRecord(path, stored.id)
      useBasesStore.getState().removeRecord(path, stored.id)
    },
  })

  // Undo deletes through base_record_delete.
  await useBasesStore.getState().undo(path)
  const dels = kernel.callsTo('base_record_delete')
  assert.equal(dels.length, 1)
  assert.deepEqual(dels[0].args, { path, record_id: stored.id })
  assert.equal(
    store.bases[path].records.find((r) => r.id === stored.id),
    undefined,
  )

  // Redo recreates with the same id (verifies the redo path replays
  // the captured `stored` rather than minting a new one).
  await useBasesStore.getState().redo(path)
  const creates = kernel.callsTo('base_record_create')
  assert.equal(creates.length, 2)
  assert.equal((creates[1].args as { record: BaseRecord }).record.id, stored.id)
  assert.ok(store.bases[path].records.find((r) => r.id === stored.id))
})

test('soft-delete history: undo issues base_record_restore', async () => {
  const path = 'team/soft.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  await client.softDeleteRecord(path, 'r1')
  useBasesStore.getState().pushHistory(path, {
    label: 'Soft-delete row',
    forward: async () => {
      await client.softDeleteRecord(path, 'r1')
    },
    inverse: async () => {
      await client.restoreRecord(path, 'r1')
    },
  })

  await useBasesStore.getState().undo(path)
  const restores = kernel.callsTo('base_record_restore')
  assert.equal(restores.length, 1)
  assert.deepEqual(restores[0].args, { path, record_id: 'r1' })
  assert.equal(store.bases[path].records[0].deletedAt, null)
})

test('schema-rename history: undo issues base_property_rename(new → old)', async () => {
  const path = 'team/rename.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  await client.renameProperty(path, 'title', 'heading')
  useBasesStore.getState().pushHistory(path, {
    label: 'Rename column title → heading',
    forward: async () => {
      await client.renameProperty(path, 'title', 'heading')
    },
    inverse: async () => {
      await client.renameProperty(path, 'heading', 'title')
    },
  })

  await useBasesStore.getState().undo(path)
  const renames = kernel.callsTo('base_property_rename')
  assert.equal(renames.length, 2)
  assert.deepEqual(renames[1].args, { path, old_name: 'heading', new_name: 'title' })
  // Field re-appears under the original name in the in-memory store.
  assert.ok('title' in store.bases[path].schema.fields)
  assert.ok(!('heading' in store.bases[path].schema.fields))
})

test('schema-delete history: undo recreates the column AND restores cell values via base_record_update for each snapshotted record', async () => {
  const path = 'team/delcol.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  // Snapshot before the destructive op (mirrors SchemaEditor.handleDelete).
  const prevDef = { ...(store.bases[path].schema.fields.status as Record<string, unknown>) }
  const prevValues = new Map<string, unknown>()
  for (const r of store.bases[path].records) {
    prevValues.set(r.id, r.status)
  }

  await client.deleteProperty(path, 'status')
  useBasesStore.getState().pushHistory(path, {
    label: 'Delete column status',
    forward: async () => {
      await client.deleteProperty(path, 'status')
    },
    inverse: async () => {
      await client.createProperty(path, 'status', prevDef)
      for (const [recordId, value] of prevValues) {
        await client.updateRecord(path, recordId, { status: value })
      }
    },
  })

  // Pre-undo: column is gone.
  assert.ok(!('status' in store.bases[path].schema.fields))

  await useBasesStore.getState().undo(path)
  const creates = kernel.callsTo('base_property_create')
  assert.equal(creates.length, 1)
  assert.deepEqual(creates[0].args, { path, name: 'status', definition: prevDef })

  // Two cell-restore writes — one per record snapshot.
  const updates = kernel.callsTo('base_record_update')
  assert.equal(updates.length, 2)
  const byId = new Map(updates.map((u) => [(u.args as { record_id: string }).record_id, u.args]))
  assert.deepEqual(byId.get('r1'), { path, record_id: 'r1', fields: { status: 'todo' } })
  assert.deepEqual(byId.get('r2'), { path, record_id: 'r2', fields: { status: 'doing' } })
  // Schema regained the column.
  assert.ok('status' in store.bases[path].schema.fields)
})

test('view-create / view-update / view-delete history round-trip via base_view_*', async () => {
  const path = 'team/views.bases'
  const store = seededStore(path)
  const kernel = makeMockKernel(inMemoryBaseHandlers(store))
  const client = makeBasesKernelClient(kernel.api)

  resetTab(path)
  useBasesStore.getState().setBase(path, store.bases[path])

  const v: BaseView = { name: 'Open', type: 'table', filter: [] }

  // Create + history entry.
  await client.createView(path, v)
  useBasesStore.getState().pushHistory(path, {
    label: `Create view "${v.name}"`,
    forward: async () => {
      await client.createView(path, v)
    },
    inverse: async () => {
      await client.deleteView(path, v.name)
    },
  })
  // Undo deletes through base_view_delete.
  await useBasesStore.getState().undo(path)
  let deletes = kernel.callsTo('base_view_delete')
  assert.equal(deletes.length, 1)
  assert.deepEqual(deletes[0].args, { path, name: 'Open' })

  // Update — re-create then snapshot a prior copy and update.
  await client.createView(path, v)
  const prev: BaseView = JSON.parse(JSON.stringify(v))
  const next: BaseView = { ...v, filter: [{ field: 'status', operator: 'eq', value: 'todo' }] }
  await client.updateView(path, next)
  useBasesStore.getState().pushHistory(path, {
    label: `Save view "${v.name}"`,
    forward: async () => {
      await client.updateView(path, next)
    },
    inverse: async () => {
      await client.updateView(path, prev)
    },
  })
  await useBasesStore.getState().undo(path)
  const updates = kernel.callsTo('base_view_update')
  assert.equal(updates.length, 2)
  assert.deepEqual(
    (updates[1].args as { view: BaseView }).view.filter,
    [],
    'expected inverse to send the prior empty-filter view',
  )

  // Delete — snapshot, delete, history; undo recreates verbatim.
  const beforeDel: BaseView = JSON.parse(JSON.stringify(store.bases[path].views[0]))
  await client.deleteView(path, beforeDel.name)
  useBasesStore.getState().pushHistory(path, {
    label: `Delete view "${beforeDel.name}"`,
    forward: async () => {
      await client.deleteView(path, beforeDel.name)
    },
    inverse: async () => {
      await client.createView(path, beforeDel)
    },
  })
  await useBasesStore.getState().undo(path)
  // The view came back via base_view_create.
  const creates = kernel.callsTo('base_view_create')
  // 1 setup create + 1 setup re-create + 1 inverse re-create = 3.
  assert.equal(creates.length, 3)
  assert.equal(
    (creates[creates.length - 1].args as { view: BaseView }).view.name,
    beforeDel.name,
  )
  assert.equal(store.bases[path].views.length, 1)

  // Sanity — base_view_delete fired during the inverse of create AND
  // forward of delete. We exercised both.
  deletes = kernel.callsTo('base_view_delete')
  assert.equal(deletes.length, 2)
})

test('commandRegistry undo handler is no-op when no bases tab has focus', () => {
  // No active handle ⇒ withActiveBases returns false and never throws.
  setActiveBases(null)
  assert.equal(getActiveBases(), null)

  let invoked = 0
  const ran = withActiveBases(() => {
    invoked += 1
  })
  assert.equal(ran, false)
  assert.equal(invoked, 0)

  // Register a handle and confirm the same call now dispatches.
  let undoCalls = 0
  setActiveBases({
    undo: () => {
      undoCalls += 1
    },
    redo: () => {},
    cut: () => {},
    copy: () => {},
    paste: () => {},
  })
  const ran2 = withActiveBases((h) => h.undo())
  assert.equal(ran2, true)
  assert.equal(undoCalls, 1)

  // Clean up so other tests don't see a stale handle.
  setActiveBases(null)
})

test('undo failure populates lastUndoError; pushHistory clears it', async () => {
  const path = 'team/err.bases'
  resetTab(path)
  // Inverse throws so undo trips its catch arm.
  useBasesStore.getState().pushHistory(path, {
    label: 'Boom',
    forward: async () => {},
    inverse: async () => {
      throw new Error('kernel offline')
    },
  })
  const ok = await useBasesStore.getState().undo(path)
  assert.equal(ok, false)
  assert.match(
    useBasesStore.getState().tabs[path].lastUndoError ?? '',
    /undo failed: kernel offline/,
  )
  // Next push clears the banner.
  useBasesStore.getState().pushHistory(path, {
    label: 'noop',
    forward: async () => {},
    inverse: async () => {},
  })
  assert.equal(useBasesStore.getState().tabs[path].lastUndoError, null)
})
