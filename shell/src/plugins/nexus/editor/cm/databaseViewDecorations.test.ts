// Pure-logic tests for the BL-012 split-3 inline `[[{db:…}]]`
// decoration source. Re-exported via
// `shell/tests/database-view-decorations.test.ts` so the default
// `pnpm test` glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorSelection, EditorState } from '@codemirror/state'

import type { EditorKernelClient } from '../kernelClient.ts'
import {
  buildDatabaseViewDecorations,
  parseDatabaseViewBlocks,
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
