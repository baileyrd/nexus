import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  decodeFiles,
  subsequenceScore,
  filterFiles,
  isAttachment,
  pushRecent,
  type FileEntry,
} from './fileMatch'

test('decodeFiles decodes a well-formed array of file rows', () => {
  const files = decodeFiles([
    { path: 'notes/a.md', file_type: 'markdown', modified_at: 100 },
    { path: 'attachments/img.png', file_type: 'attachment', modified_at: 200 },
  ])
  assert.strictEqual(files.length, 2)
  assert.deepStrictEqual(files[0], { path: 'notes/a.md', file_type: 'markdown', modified_at: 100 })
})

test('decodeFiles defaults missing file_type/modified_at and drops rows without a path', () => {
  const files = decodeFiles([{ path: 'a.md' }, { file_type: 'markdown' }, 'nope', null])
  assert.strictEqual(files.length, 1)
  assert.strictEqual(files[0]?.file_type, 'markdown')
  assert.strictEqual(files[0]?.modified_at, 0)
})

test('decodeFiles tolerates non-array input', () => {
  assert.deepStrictEqual(decodeFiles(null), [])
  assert.deepStrictEqual(decodeFiles({ files: [] }), [])
})

test('subsequenceScore matches in-order non-contiguous characters', () => {
  assert.strictEqual(subsequenceScore('notes/daily/2026.md', 'ndm'), 17)
  assert.strictEqual(subsequenceScore('readme.md', 'xyz'), null)
})

test('subsequenceScore treats an empty query as matching everything at score -1', () => {
  assert.strictEqual(subsequenceScore('anything', ''), -1)
})

function file(path: string, overrides: Partial<FileEntry> = {}): FileEntry {
  return { path, file_type: 'markdown', modified_at: 0, ...overrides }
}

test('filterFiles with an empty query returns recents first, then the rest alphabetically', () => {
  const files = [file('z.md'), file('a.md'), file('m.md')]
  const results = filterFiles(files, '', ['m.md', 'z.md'])
  assert.deepStrictEqual(
    results.map((r) => r.entry.path),
    ['m.md', 'z.md', 'a.md'],
  )
})

test('filterFiles fuzzy-matches on path and excludes non-matches', () => {
  const files = [file('notes/daily/2026-07-03.md'), file('notes/project-x.md'), file('readme.md')]
  const results = filterFiles(files, 'proj', [])
  assert.strictEqual(results.length, 1)
  assert.strictEqual(results[0]?.entry.path, 'notes/project-x.md')
})

test('filterFiles breaks score ties by recency, then alphabetically', () => {
  // Both paths score identically against 'a.md' (exact tail match at the
  // same index) — recency must decide, then alphabetical order.
  const files = [file('z/a.md'), file('b/a.md'), file('c/a.md')]
  const results = filterFiles(files, 'a.md', ['b/a.md'])
  assert.deepStrictEqual(
    results.map((r) => r.entry.path),
    ['b/a.md', 'c/a.md', 'z/a.md'],
  )
})

test('isAttachment recognizes only the attachment file_type', () => {
  assert.strictEqual(isAttachment('attachment'), true)
  assert.strictEqual(isAttachment('markdown'), false)
  assert.strictEqual(isAttachment('canvas'), false)
})

test('pushRecent moves an existing entry to the front without duplicating', () => {
  const recents = pushRecent(['a.md', 'b.md', 'c.md'], 'b.md')
  assert.deepStrictEqual(recents, ['b.md', 'a.md', 'c.md'])
})

test('pushRecent caps the list at 20 entries', () => {
  const many = Array.from({ length: 25 }, (_, i) => `f${i}.md`)
  const recents = pushRecent(many, 'new.md')
  assert.strictEqual(recents.length, 20)
  assert.strictEqual(recents[0], 'new.md')
})
