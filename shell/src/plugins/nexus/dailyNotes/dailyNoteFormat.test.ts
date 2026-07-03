import { test } from 'node:test'
import assert from 'node:assert/strict'

import { formatDate, dailyNotePath, dailyNoteSkeleton, DEFAULT_DATE_FORMAT } from './dailyNoteFormat'

test('formatDate substitutes YYYY/MM/DD tokens with zero-padded values', () => {
  const d = new Date(2026, 6, 3) // July 3, 2026 (month is 0-indexed)
  assert.strictEqual(formatDate(d, 'YYYY-MM-DD'), '2026-07-03')
  assert.strictEqual(formatDate(d, DEFAULT_DATE_FORMAT), '2026-07-03')
})

test('formatDate passes through literal text and tolerates repeated/rearranged tokens', () => {
  const d = new Date(2026, 0, 9) // Jan 9, 2026
  assert.strictEqual(formatDate(d, 'DD-MM-YYYY'), '09-01-2026')
  assert.strictEqual(formatDate(d, 'notes MM/DD'), 'notes 01/09')
})

test('formatDate leaves a format with no recognized tokens unchanged', () => {
  const d = new Date(2026, 6, 3)
  assert.strictEqual(formatDate(d, 'daily-note'), 'daily-note')
})

test('dailyNotePath joins folder and formatted filename', () => {
  const d = new Date(2026, 6, 3)
  assert.strictEqual(dailyNotePath(d, 'daily', 'YYYY-MM-DD'), 'daily/2026-07-03.md')
  assert.strictEqual(dailyNotePath(d, 'notes/daily', 'YYYY-MM-DD'), 'notes/daily/2026-07-03.md')
})

test('dailyNotePath trims leading/trailing slashes on the folder', () => {
  const d = new Date(2026, 6, 3)
  assert.strictEqual(dailyNotePath(d, '/daily/', 'YYYY-MM-DD'), 'daily/2026-07-03.md')
})

test('dailyNotePath with an empty folder puts the file at the forge root', () => {
  const d = new Date(2026, 6, 3)
  assert.strictEqual(dailyNotePath(d, '', 'YYYY-MM-DD'), '2026-07-03.md')
})

test('dailyNoteSkeleton carries frontmatter date/tags and a Month Day, Year heading', () => {
  const d = new Date(2026, 6, 3)
  const skeleton = dailyNoteSkeleton(d)
  assert.match(skeleton, /^---\ndate: 2026-07-03\ntags: \[daily\]\n---\n/)
  assert.match(skeleton, /# July 3, 2026/)
  assert.match(skeleton, /## Tasks/)
  assert.match(skeleton, /## Notes/)
})
