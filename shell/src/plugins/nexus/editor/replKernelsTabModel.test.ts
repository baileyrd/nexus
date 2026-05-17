// BL-142 Phase 3 — pure-factor tests for the REPL Kernels tab.
//
// Covers the rows ↔ JSON round-trip + the per-row validity checks
// that drive the Save button enablement. The React component
// itself is exercised in `replKernelsTab.test.ts` against happy-dom.

import { describe, it, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  blankRow,
  jsonFromRows,
  rowIsValid,
  rowsAreSavable,
  rowsFromJson,
  _resetBlankRowIdCounterForTests,
  type KernelRow,
} from './replKernelsTabModel.ts'

describe('rowsFromJson', () => {
  it('parses the JSON map into ordered rows', () => {
    const rows = rowsFromJson('{"python":"python3 -i","node":"node --interactive"}')
    assert.equal(rows.length, 2)
    assert.equal(rows[0].lang, 'python')
    assert.equal(rows[0].command, 'python3 -i')
    assert.equal(rows[1].lang, 'node')
    assert.equal(rows[1].command, 'node --interactive')
  })

  it('returns [] on malformed JSON', () => {
    assert.deepEqual(rowsFromJson('not json'), [])
    assert.deepEqual(rowsFromJson('{'), [])
    assert.deepEqual(rowsFromJson(''), [])
  })

  it('returns [] when the JSON is an array (must be an object)', () => {
    assert.deepEqual(rowsFromJson('["python","python3 -i"]'), [])
  })

  it('returns [] when the JSON is null', () => {
    assert.deepEqual(rowsFromJson('null'), [])
  })

  it('drops entries with empty keys', () => {
    const rows = rowsFromJson('{"":"python3 -i","python":"python3 -i"}')
    assert.equal(rows.length, 1)
    assert.equal(rows[0].lang, 'python')
  })

  it('drops entries with non-string command values', () => {
    const rows = rowsFromJson('{"python":42,"node":"node --interactive"}')
    assert.equal(rows.length, 1)
    assert.equal(rows[0].lang, 'node')
  })

  it('returns [] for the default ({}) value', () => {
    assert.deepEqual(rowsFromJson('{}'), [])
  })

  it('preserves insertion order', () => {
    const rows = rowsFromJson('{"z":"zsh","a":"ash","m":"mksh"}')
    assert.deepEqual(
      rows.map((r) => r.lang),
      ['z', 'a', 'm'],
    )
  })

  it('assigns stable distinct ids to each row', () => {
    const rows = rowsFromJson('{"python":"p","node":"n","ruby":"r"}')
    const ids = new Set(rows.map((r) => r.id))
    assert.equal(ids.size, 3)
  })
})

describe('jsonFromRows', () => {
  it('serialises a list of valid rows into the canonical JSON shape', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: 'python', command: 'python3 -i' },
      { id: 'b', lang: 'node', command: 'node --interactive' },
    ]
    const json = jsonFromRows(rows)
    const parsed = JSON.parse(json) as Record<string, string>
    assert.equal(parsed.python, 'python3 -i')
    assert.equal(parsed.node, 'node --interactive')
  })

  it('drops rows with empty lang', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: '', command: 'python3 -i' },
      { id: 'b', lang: 'node', command: 'node --interactive' },
    ]
    const json = jsonFromRows(rows)
    assert.equal(JSON.parse(json).python, undefined)
    assert.equal(JSON.parse(json).node, 'node --interactive')
  })

  it('drops rows with empty command', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: 'python', command: '' },
      { id: 'b', lang: 'node', command: 'node --interactive' },
    ]
    assert.equal(JSON.parse(jsonFromRows(rows)).python, undefined)
  })

  it('trims surrounding whitespace before persisting', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: '  python  ', command: '  python3 -i  ' },
    ]
    const parsed = JSON.parse(jsonFromRows(rows))
    assert.equal(parsed.python, 'python3 -i')
    assert.equal(parsed['  python  '], undefined)
  })

  it('drops rows where lang is whitespace-only', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: '   ', command: 'whatever' },
      { id: 'b', lang: 'real', command: 'cmd' },
    ]
    const parsed = JSON.parse(jsonFromRows(rows))
    assert.deepEqual(Object.keys(parsed), ['real'])
  })

  it('last duplicate-lang occurrence wins', () => {
    const rows: KernelRow[] = [
      { id: 'a', lang: 'python', command: 'old-cmd' },
      { id: 'b', lang: 'python', command: 'new-cmd' },
    ]
    assert.equal(JSON.parse(jsonFromRows(rows)).python, 'new-cmd')
  })

  it('returns "{}" for an empty rows array', () => {
    assert.equal(jsonFromRows([]), '{}')
  })

  it('round-trips through rowsFromJson without drift on the canonical form', () => {
    const startRows: KernelRow[] = [
      { id: 'a', lang: 'python', command: 'python3 -i' },
      { id: 'b', lang: 'node', command: 'node --interactive' },
    ]
    const json = jsonFromRows(startRows)
    const reparsed = rowsFromJson(json)
    assert.equal(reparsed.length, 2)
    assert.equal(reparsed[0].lang, 'python')
    assert.equal(reparsed[1].command, 'node --interactive')
  })
})

describe('rowIsValid', () => {
  it('returns true for fully populated rows', () => {
    assert.equal(rowIsValid({ id: 'a', lang: 'python', command: 'python3 -i' }), true)
  })

  it('returns false for empty lang', () => {
    assert.equal(rowIsValid({ id: 'a', lang: '', command: 'python3 -i' }), false)
  })

  it('returns false for empty command', () => {
    assert.equal(rowIsValid({ id: 'a', lang: 'python', command: '' }), false)
  })

  it('returns false for whitespace-only fields', () => {
    assert.equal(rowIsValid({ id: 'a', lang: '   ', command: 'cmd' }), false)
    assert.equal(rowIsValid({ id: 'a', lang: 'python', command: ' ' }), false)
  })
})

describe('rowsAreSavable', () => {
  it('returns true when at least one valid row exists and no duplicates', () => {
    assert.equal(
      rowsAreSavable([{ id: 'a', lang: 'python', command: 'python3 -i' }]),
      true,
    )
  })

  it('returns false when all rows are blank / partial', () => {
    assert.equal(
      rowsAreSavable([
        { id: 'a', lang: '', command: '' },
        { id: 'b', lang: 'partial', command: '' },
      ]),
      false,
    )
  })

  it('returns false when valid rows have duplicate langs', () => {
    assert.equal(
      rowsAreSavable([
        { id: 'a', lang: 'python', command: 'one' },
        { id: 'b', lang: 'python', command: 'two' },
      ]),
      false,
    )
  })

  it('ignores blank rows when computing duplicates', () => {
    // A blank row in the middle shouldn't make the surrounding rows look
    // like duplicates of themselves.
    assert.equal(
      rowsAreSavable([
        { id: 'a', lang: 'python', command: 'p' },
        { id: 'b', lang: '', command: '' },
        { id: 'c', lang: 'node', command: 'n' },
      ]),
      true,
    )
  })

  it('returns false for an empty rows array', () => {
    assert.equal(rowsAreSavable([]), false)
  })
})

describe('blankRow', () => {
  beforeEach(() => {
    _resetBlankRowIdCounterForTests()
  })

  it('returns a row with empty fields and a unique id', () => {
    const a = blankRow()
    const b = blankRow()
    assert.equal(a.lang, '')
    assert.equal(a.command, '')
    assert.notEqual(a.id, b.id)
  })
})
