// shell/src/plugins/nexus/editor/cm/gitBlame.test.ts
//
// BL-079 — pure-function tests for the blame annotation.
// Integration concerns (decoration set rebuilding, kernel IPC) are
// covered by the live editor; the formatting here is the part a
// future change is most likely to break.

import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { formatBlameRow, formatRelativeDate } from './gitBlame.ts'

describe('formatBlameRow', () => {
  it('combines author / hash / date / summary into a single line', () => {
    const out = formatBlameRow({
      commit_hash: 'abc1234',
      author: 'Jane Doe',
      date: new Date(Date.now() - 60_000 * 5).toISOString(),
      message: 'Add feature flag',
      start_line: 1,
      end_line: 3,
    })
    assert.match(out, /^Jane · abc1234 · 5m ago · Add feature flag$/)
  })

  it('truncates summaries longer than 60 characters with an ellipsis', () => {
    const long = 'a'.repeat(80)
    const out = formatBlameRow({
      commit_hash: 'fedcba9',
      author: 'A',
      date: new Date().toISOString(),
      message: long,
      start_line: 1,
      end_line: 1,
    })
    assert.ok(out.endsWith('…'), `summary not truncated: ${out}`)
    // 57 chars of summary plus the ellipsis = 58.
    const summary = out.split(' · ').at(-1) ?? ''
    assert.equal(summary.length, 58)
  })

  it('uses the first whitespace-separated word as author', () => {
    const out = formatBlameRow({
      commit_hash: '1234567',
      author: 'Mary Jane Doe',
      date: new Date().toISOString(),
      message: 'fix',
      start_line: 1,
      end_line: 1,
    })
    assert.match(out, /^Mary · /)
  })
})

describe('formatRelativeDate', () => {
  it('returns "just now" for very-recent timestamps', () => {
    assert.equal(
      formatRelativeDate(new Date(Date.now() - 1_000).toISOString()),
      'just now',
    )
  })

  it('returns minutes / hours / days / months / years scaled', () => {
    const m = (n: number) =>
      formatRelativeDate(new Date(Date.now() - n * 60_000).toISOString())
    const h = (n: number) =>
      formatRelativeDate(new Date(Date.now() - n * 3600_000).toISOString())
    const d = (n: number) =>
      formatRelativeDate(new Date(Date.now() - n * 86400_000).toISOString())
    assert.equal(m(5), '5m ago')
    assert.equal(h(2), '2h ago')
    assert.equal(d(3), '3d ago')
    assert.equal(d(45), '1mo ago')
    assert.equal(d(800), '2y ago')
  })

  it('returns empty string for unparseable input', () => {
    assert.equal(formatRelativeDate('not-a-date'), '')
  })
})
