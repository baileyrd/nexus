// Pure-logic tests for the BL-012 split-3 inline `[[{db:…}]]`
// decoration source. Re-exported via
// `shell/tests/database-view-decorations.test.ts` so the default
// `pnpm test` glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorSelection, EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import type { EditorKernelClient } from '../kernelClient.ts'
import {
  buildDatabaseViewDecorations,
  databaseViewInvalidate,
  type KernelEventSubscriber,
  makeBasesChangeWatcher,
  parseDatabaseViewBlocks,
  pathToBasePath,
} from './databaseViewDecorations.ts'
import { DatabaseViewCache } from './databaseViewWidget.ts'

// ── parseDatabaseViewBlocks ─────────────────────────────────────────────────

test('parser returns no blocks for plain markdown', () => {
  const out = parseDatabaseViewBlocks('# Heading\n\nsome [[wikilink]] text\n')
  assert.equal(out.blocks.length, 0)
  assert.equal(out.errors.length, 0)
})

test('parser recognises bare table-form spec', () => {
  const text = 'before\n[[{db:Tasks.bases}]]\nafter\n'
  const out = parseDatabaseViewBlocks(text)
  assert.equal(out.blocks.length, 1)
  const b = out.blocks[0]
  assert.equal(b.databasePath, 'Tasks.bases')
  assert.deepEqual(b.config.view_type, { kind: 'table' })
  assert.deepEqual(b.config.filters, [])
  assert.deepEqual(b.config.sorts, [])
  assert.equal(b.config.group_by, null)
  // Range covers exactly the literal `[[{db:Tasks.bases}]]`.
  assert.equal(text.slice(b.from, b.to), '[[{db:Tasks.bases}]]')
})

test('parser handles multiple filters / sorts via repeated query params', () => {
  const out = parseDatabaseViewBlocks(
    '[[{db:Tasks.bases?filter=status%20%3D%20Done&filter=priority%20%3E%202&sort=due_date%20asc&sort=title}]]',
  )
  assert.equal(out.blocks.length, 1)
  const b = out.blocks[0]
  assert.deepEqual(b.config.filters, ['status = Done', 'priority > 2'])
  assert.deepEqual(b.config.sorts, ['due_date asc', 'title'])
})

test('parser maps view=kanban + group= into the structured Kanban view_type', () => {
  const out = parseDatabaseViewBlocks('[[{db:Board.bases?view=kanban&group=status}]]')
  assert.equal(out.blocks.length, 1)
  assert.deepEqual(out.blocks[0].config.view_type, {
    kind: 'kanban',
    column_by: 'status',
  })
  // Kanban's column_by takes precedence; generic group_by stays null.
  assert.equal(out.blocks[0].config.group_by, null)
})

test('parser maps view=calendar + date= into the structured Calendar view_type', () => {
  const out = parseDatabaseViewBlocks('[[{db:Cal.bases?view=calendar&date=due}]]')
  assert.deepEqual(out.blocks[0].config.view_type, {
    kind: 'calendar',
    date_field: 'due',
  })
})

test('parser surfaces malformed specs as errors with a helpful message', () => {
  const out = parseDatabaseViewBlocks(
    '[[{db:}]] [[{db:Tasks.bases?view=spaceship}]] [[{db:../escape.bases}]]',
  )
  assert.equal(out.blocks.length, 0)
  assert.equal(out.errors.length, 3)
  assert.match(out.errors[0].message, /empty/i)
  assert.match(out.errors[1].message, /unknown view kind/i)
  assert.match(out.errors[2].message, /invalid database path/i)
})

test('parser is reentrant — two consecutive scans return the same result', () => {
  const text = 'a [[{db:A.bases}]] b [[{db:B.bases}]] c'
  const a = parseDatabaseViewBlocks(text)
  const b = parseDatabaseViewBlocks(text)
  assert.equal(a.blocks.length, 2)
  assert.equal(b.blocks.length, 2)
  assert.equal(a.blocks[1].databasePath, b.blocks[1].databasePath)
})

test('parser respects the offset param for line-relative scans', () => {
  const out = parseDatabaseViewBlocks('[[{db:Tasks.bases}]]', 100)
  assert.equal(out.blocks[0].from, 100)
  assert.equal(out.blocks[0].to, 120)
})

// ── buildDatabaseViewDecorations ────────────────────────────────────────────

const stubClient = {} as EditorKernelClient

function makeState(doc: string, cursor = 0): EditorState {
  return EditorState.create({
    doc,
    selection: EditorSelection.single(cursor),
  })
}

test('builder emits a block-replace decoration for off-active-line blocks', () => {
  const state = makeState('intro\n\n[[{db:Tasks.bases}]]\n\noutro\n', 0)
  const set = buildDatabaseViewDecorations(state, {
    client: stubClient,
    cache: new DatabaseViewCache(),
  })
  // Walk the decoration set — it ought to carry exactly one
  // (replace) range for the dbview line.
  const ranges: Array<{ from: number; to: number }> = []
  set.between(0, state.doc.length, (from, to) => {
    ranges.push({ from, to })
  })
  assert.equal(ranges.length, 1)
  const block = ranges[0]
  assert.equal(state.doc.sliceString(block.from, block.to), '[[{db:Tasks.bases}]]')
})

test('builder reveals the source when the cursor is on the same line', () => {
  const doc = 'intro\n\n[[{db:Tasks.bases}]]\n\noutro\n'
  const dbviewLineStart = doc.indexOf('[[')
  const state = makeState(doc, dbviewLineStart + 2)
  const set = buildDatabaseViewDecorations(state, {
    client: stubClient,
    cache: new DatabaseViewCache(),
  })
  const ranges: Array<{ from: number; to: number }> = []
  set.between(0, state.doc.length, (from, to) => ranges.push({ from, to }))
  assert.equal(ranges.length, 0, 'cursor on the line ⇒ no replace decoration')
})

test('builder marks malformed specs with a syntax-error class instead of replacing them', () => {
  const state = makeState('header\n\n[[{db:}]]\n\nfooter\n', 0)
  const set = buildDatabaseViewDecorations(state, {
    client: stubClient,
    cache: new DatabaseViewCache(),
  })
  const collected: Array<{ from: number; to: number; kind: string }> = []
  set.between(0, state.doc.length, (from, to, deco) => {
    const spec = deco.spec as { class?: string; widget?: unknown; block?: boolean }
    collected.push({
      from,
      to,
      kind: spec.widget ? 'replace' : spec.class ?? 'unknown',
    })
  })
  assert.equal(collected.length, 1)
  assert.equal(collected[0].kind, 'cm-md-dbview-syntax-error')
})

test('builder emits multiple decorations when multiple blocks live on different lines', () => {
  const doc = '[[{db:A.bases}]]\nspacer\n[[{db:B.bases}]]\n'
  const state = makeState(doc, doc.length)
  const set = buildDatabaseViewDecorations(state, {
    client: stubClient,
    cache: new DatabaseViewCache(),
  })
  const ranges: Array<{ from: number; to: number }> = []
  set.between(0, state.doc.length, (from, to) => ranges.push({ from, to }))
  assert.equal(ranges.length, 2)
  // Order is deterministic — sorted by `from` ascending.
  assert.ok(ranges[0].from < ranges[1].from)
})

// ── pathToBasePath ──────────────────────────────────────────────────────────

test('pathToBasePath maps inside-bases paths to the directory itself', () => {
  assert.equal(pathToBasePath('Tasks.bases'), 'Tasks.bases')
  assert.equal(pathToBasePath('Tasks.bases/records.json'), 'Tasks.bases')
  assert.equal(pathToBasePath('Tasks.bases/views/board.json'), 'Tasks.bases')
  assert.equal(pathToBasePath('nested/Board.bases/records.json'), 'nested/Board.bases')
})

test('pathToBasePath returns null for paths outside any .bases directory', () => {
  assert.equal(pathToBasePath(''), null)
  assert.equal(pathToBasePath('notes/A.md'), null)
  assert.equal(pathToBasePath('Tasks.basesy/records.json'), null)
})

// ── makeBasesChangeWatcher ──────────────────────────────────────────────────

class FakeEvents implements KernelEventSubscriber {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  emit: ((topic: string, payload: any) => void) | null = null
  unsubscribed = false
  on<T>(_prefix: string, handler: (topic: string, payload: T) => void): Promise<() => void> {
    this.emit = handler as never
    return Promise.resolve(() => {
      this.unsubscribed = true
    })
  }
}

test('watcher invalidates by base path and dispatches a recompute effect on file_modified', async () => {
  const cache = new DatabaseViewCache()
  // Seed with two cached layouts under Tasks.bases plus an untouched one.
  await cache.run('Tasks.bases {"a":1}', () =>
    Promise.resolve({
      applied: { view_name: '', view_type: 'table' as const, fields: [], layout: { kind: 'flat' as const, records: [] } },
      schema: { version: '1.0', fields: {} },
    }),
  )
  await cache.run('Tasks.bases {"a":2}', () =>
    Promise.resolve({
      applied: { view_name: '', view_type: 'table' as const, fields: [], layout: { kind: 'flat' as const, records: [] } },
      schema: { version: '1.0', fields: {} },
    }),
  )
  await cache.run('Other.bases {}', () =>
    Promise.resolve({
      applied: { view_name: '', view_type: 'table' as const, fields: [], layout: { kind: 'flat' as const, records: [] } },
      schema: { version: '1.0', fields: {} },
    }),
  )
  assert.equal(cache.size(), 3)

  const events = new FakeEvents()
  const view = new EditorView({ state: EditorState.create({ doc: '' }) })
  const dispatched: unknown[] = []
  const origDispatch = view.dispatch.bind(view)
  // Spy on dispatch to confirm the effect type.
  view.dispatch = (...args) => {
    dispatched.push(args[0])
    return origDispatch(...args)
  }

  const watcher = makeBasesChangeWatcher(view, {
    client: stubClient,
    cache,
    events,
  })

  // Wait for the subscribe promise to resolve.
  await Promise.resolve()
  await Promise.resolve()
  assert.ok(events.emit, 'subscribe should have wired the handler')

  // Simulate a record-write touching `Tasks.bases/records.json`.
  events.emit!('com.nexus.storage.file_modified', {
    path: 'Tasks.bases/records.json',
    content_hash: 'deadbeef',
  })

  assert.equal(cache.size(), 1, 'two Tasks.bases entries dropped')
  assert.ok(cache.peek('Other.bases {}')?.response, 'untouched base survives')
  // Exactly one dispatch carrying the invalidate effect. Spec
  // shape is `{ effects: StateEffect | StateEffect[] }`; normalise
  // to an array before testing each member's `.is()`.
  const matched = dispatched.filter((d) => {
    const t = d as { effects?: unknown }
    const effects = Array.isArray(t.effects)
      ? t.effects
      : t.effects !== undefined
        ? [t.effects]
        : []
    return effects.some(
      (e: unknown) =>
        typeof (e as { is?: (x: unknown) => boolean }).is === 'function' &&
        (e as { is: (x: unknown) => boolean }).is(databaseViewInvalidate),
    )
  })
  assert.equal(matched.length, 1)

  watcher.destroy()
  view.destroy()
  assert.equal(events.unsubscribed, true)
})

test('watcher skips dispatch when the changed path is outside any .bases directory', async () => {
  const cache = new DatabaseViewCache()
  await cache.run('Tasks.bases {}', () =>
    Promise.resolve({
      applied: { view_name: '', view_type: 'table' as const, fields: [], layout: { kind: 'flat' as const, records: [] } },
      schema: { version: '1.0', fields: {} },
    }),
  )

  const events = new FakeEvents()
  const view = new EditorView({ state: EditorState.create({ doc: '' }) })
  const dispatched: unknown[] = []
  const origDispatch = view.dispatch.bind(view)
  view.dispatch = (...args) => {
    dispatched.push(args[0])
    return origDispatch(...args)
  }
  makeBasesChangeWatcher(view, { client: stubClient, cache, events })
  await Promise.resolve()
  await Promise.resolve()

  events.emit!('com.nexus.storage.file_modified', {
    path: 'notes/Diary.md',
    content_hash: 'feedface',
  })
  assert.equal(cache.size(), 1, 'unrelated edit must not evict cache')
  assert.equal(dispatched.length, 0, 'no recompute dispatched')
  view.destroy()
})

test('watcher handles file_renamed (both from + to) so a rename into / out of a base flushes', async () => {
  const cache = new DatabaseViewCache()
  await cache.run('Tasks.bases {}', () =>
    Promise.resolve({
      applied: { view_name: '', view_type: 'table' as const, fields: [], layout: { kind: 'flat' as const, records: [] } },
      schema: { version: '1.0', fields: {} },
    }),
  )
  const events = new FakeEvents()
  const view = new EditorView({ state: EditorState.create({ doc: '' }) })
  makeBasesChangeWatcher(view, { client: stubClient, cache, events })
  await Promise.resolve()
  await Promise.resolve()

  // A rename moving a file *out* of Tasks.bases — `from` is the
  // base-relevant path here.
  events.emit!('com.nexus.storage.file_renamed', {
    from: 'Tasks.bases/records.json',
    to: 'archive/records.json',
    content_hash: 'a1b2',
  })
  assert.equal(cache.size(), 0, 'rename-out should flush the cached view')
  view.destroy()
})

test('watcher is a no-op when deps.events is absent', () => {
  const view = new EditorView({ state: EditorState.create({ doc: '' }) })
  const cache = new DatabaseViewCache()
  const handle = makeBasesChangeWatcher(view, { client: stubClient, cache })
  // No subscribe, no error — destroy is a clean no-op.
  handle.destroy()
  view.destroy()
})
