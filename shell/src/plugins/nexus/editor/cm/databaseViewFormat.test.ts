// shell/src/plugins/nexus/editor/cm/databaseViewFormat.test.ts
//
// BL-069 — type-aware cell formatter unit tests. Mirrors the
// `nexus_types::bases::FieldType` matrix so a renderer regression
// (e.g. someone reverting to the legacy stringify path) flips at
// least one of these.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { formatCell, lookupFieldDef } from './databaseViewFormat.ts'

test('formatCell: null / undefined render as empty string regardless of field type', () => {
  assert.equal(formatCell(null), '')
  assert.equal(formatCell(undefined, { type: 'text' }), '')
  assert.equal(formatCell(null, { type: 'number' }), '')
})

test('formatCell: text / long-text / url / email pass strings through verbatim', () => {
  for (const type of ['text', 'long-text', 'url', 'email', 'uuid'] as const) {
    assert.equal(formatCell('hello', { type }), 'hello')
  }
})

test('formatCell: number formats with locale grouping', () => {
  assert.equal(formatCell(1234567, { type: 'number' }), '1,234,567')
  // String numerics get coerced.
  assert.equal(formatCell('42.5', { type: 'number' }), '42.5')
  // NaN / non-numeric strings fall back to primitive rendering.
  assert.equal(formatCell('abc', { type: 'number' }), 'abc')
})

test('formatCell: currency picks USD by default and honours `currency` override', () => {
  // Use a regex to dodge minor differences in Intl output across
  // locales / Node versions ($1,234.50 / US$1,234.50).
  assert.match(formatCell(1234.5, { type: 'currency' }), /\$1,234\.50/)
  assert.match(
    formatCell(1234.5, { type: 'currency', currency: 'EUR' }),
    /€?1,234\.50|1,234\.50.*€/,
  )
})

test('formatCell: percent appends `%` without re-multiplying', () => {
  // 45 means 45%, not 4500%. The Rust side stores the displayable
  // value already.
  assert.equal(formatCell(45, { type: 'percent' }), '45%')
  assert.equal(formatCell(0.5, { type: 'percent' }), '0.5%')
})

test('formatCell: checkbox renders ✓ / empty', () => {
  assert.equal(formatCell(true, { type: 'checkbox' }), '✓')
  assert.equal(formatCell(false, { type: 'checkbox' }), '')
})

test('formatCell: date / datetime / time render in ISO form', () => {
  assert.equal(formatCell('2026-05-07T12:34:56Z', { type: 'date' }), '2026-05-07')
  assert.equal(
    formatCell('2026-05-07T12:34:56Z', { type: 'datetime' }),
    '2026-05-07 12:34',
  )
  // A bare HH:MM string is preserved verbatim by the time formatter.
  assert.equal(formatCell('09:30', { type: 'time' }), '09:30')
  // Epoch seconds also round-trip.
  assert.equal(
    formatCell(1746576000, { type: 'date' }),
    '2025-05-07',
  )
})

test('formatCell: select pulls .label / .name / .id from object selectors', () => {
  assert.equal(formatCell('Done', { type: 'select' }), 'Done')
  assert.equal(formatCell({ label: 'Done' }, { type: 'select' }), 'Done')
  assert.equal(formatCell({ name: 'Doing' }, { type: 'select' }), 'Doing')
  assert.equal(formatCell({ id: 'opt-1' }, { type: 'select' }), 'opt-1')
})

test('formatCell: multi-select joins labels with comma', () => {
  assert.equal(
    formatCell(['urgent', 'review'], { type: 'multi-select' }),
    'urgent, review',
  )
  assert.equal(
    formatCell([{ label: 'A' }, { name: 'B' }], { type: 'multi-select' }),
    'A, B',
  )
})

test('formatCell: relation renders single, array, or object form', () => {
  assert.equal(formatCell('rec-7', { type: 'relation' }), 'rec-7')
  assert.equal(
    formatCell({ name: 'Acme' }, { type: 'relation' }),
    'Acme',
  )
  assert.equal(
    formatCell([{ name: 'Acme' }, { id: 'r2' }], { type: 'relation' }),
    'Acme, r2',
  )
})

test('formatCell: missing field def falls back to legacy primitive-or-stringify', () => {
  assert.equal(formatCell('hi'), 'hi')
  assert.equal(formatCell(42), '42')
  assert.equal(formatCell(true), 'true')
  // Objects truncate at 200 chars.
  const big = { tag: 'x'.repeat(500) }
  const out = formatCell(big)
  assert.ok(out.endsWith('…'))
  assert.ok(out.length <= 200)
})

test('lookupFieldDef: returns the schema entry as an object, undefined otherwise', () => {
  const schema = { status: { type: 'select' }, count: { type: 'number' } }
  assert.deepEqual(lookupFieldDef(schema, 'status'), { type: 'select' })
  assert.equal(lookupFieldDef(schema, 'absent'), undefined)
  assert.equal(lookupFieldDef(undefined, 'status'), undefined)
  // Non-object entries (a stray `null` slipped through serde) yield
  // undefined rather than a crashy passthrough.
  assert.equal(lookupFieldDef({ x: null } as Record<string, unknown>, 'x'), undefined)
})
