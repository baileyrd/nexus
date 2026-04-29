// shell/src/plugins/nexus/semanticSearch/merge.test.ts
//
// BL-040 — unit tests for the keyword/semantic ranking merger.
// Run:
//   pnpm --filter nexus-shell test
// or:
//   node --import tsx --test \
//     shell/src/plugins/nexus/semanticSearch/merge.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mergeResults } from './merge.ts'

test('merger blends co-occurring hits at 0.5/0.5 of normalised scores', () => {
  const keyword = [
    { file_path: 'a.md', score: 10, excerpt: 'kw a' },
    { file_path: 'b.md', score: 5, excerpt: 'kw b' },
  ]
  const semantic = [
    { file_path: 'a.md', score: 0.9, chunk_text: 'sem a' },
    { file_path: 'b.md', score: 0.45, chunk_text: 'sem b' },
  ]
  const out = mergeResults(keyword, semantic)
  // a: kw_norm=1, sem_norm=1 -> 0.5*1+0.5*1 = 1.0
  // b: kw_norm=0.5, sem_norm=0.5 -> 0.5*0.5+0.5*0.5 = 0.5
  assert.equal(out.length, 2)
  assert.equal(out[0].file_path, 'a.md')
  assert.ok(Math.abs(out[0].score - 1.0) < 1e-9)
  assert.equal(out[0].hasKeyword, true)
  assert.equal(out[0].hasSemantic, true)
  // Snippet preference: keyword excerpt wins.
  assert.equal(out[0].snippet, 'kw a')
  assert.equal(out[1].file_path, 'b.md')
  assert.ok(Math.abs(out[1].score - 0.5) < 1e-9)
})

test('single-source hits are damped to half their normalised score', () => {
  const keyword = [{ file_path: 'k-only.md', score: 8, excerpt: 'k' }]
  const semantic = [{ file_path: 's-only.md', score: 0.9, chunk_text: 's' }]
  const out = mergeResults(keyword, semantic)
  assert.equal(out.length, 2)
  // Both normalise to 1.0 within their own list, then *0.5 damping.
  for (const row of out) {
    assert.ok(Math.abs(row.score - 0.5) < 1e-9)
  }
  // Tie order is map-iteration-stable, but both should report a single
  // source flag.
  const ko = out.find((r) => r.file_path === 'k-only.md')!
  assert.equal(ko.hasKeyword, true)
  assert.equal(ko.hasSemantic, false)
  assert.equal(ko.snippet, 'k')
  const so = out.find((r) => r.file_path === 's-only.md')!
  assert.equal(so.hasKeyword, false)
  assert.equal(so.hasSemantic, true)
  assert.equal(so.snippet, 's')
})

test('co-occurrence beats single-source even when raw scores would not', () => {
  // 'co.md' has tiny raw scores in both lists; 'k-only.md' dominates
  // the keyword list. Per BL-040, the co-occurrence's blend (0.5*1+0.5*1=1
  // after normalising relative to its own list maxima) wins over the
  // single-source damped 0.5.
  const keyword = [
    { file_path: 'k-only.md', score: 100, excerpt: 'big' },
    { file_path: 'co.md', score: 1, excerpt: 'co' },
  ]
  const semantic = [{ file_path: 'co.md', score: 0.5, chunk_text: 'sem' }]
  const out = mergeResults(keyword, semantic)
  assert.equal(out[0].file_path, 'co.md')
  // co.md kw_norm = 1/100 = 0.01; sem_norm = 1; blend = 0.5*0.01+0.5*1=0.505
  // k-only.md kw_norm = 1, damped 0.5 = 0.5. So co.md wins (just barely).
  assert.ok(out[0].score > out[1].score)
})

test('limit caps the merged list', () => {
  const keyword = Array.from({ length: 50 }, (_, i) => ({
    file_path: `k${i}.md`,
    score: 50 - i,
    excerpt: '',
  }))
  const out = mergeResults(keyword, [], 5)
  assert.equal(out.length, 5)
  assert.equal(out[0].file_path, 'k0.md')
})

test('empty inputs return empty list', () => {
  assert.deepEqual(mergeResults([], []), [])
})

test('zero-score lists do not divide by zero', () => {
  const keyword = [{ file_path: 'a.md', score: 0, excerpt: '' }]
  const out = mergeResults(keyword, [])
  assert.equal(out.length, 1)
  assert.equal(out[0].score, 0)
})

test('keyword excerpt is preferred over semantic chunk_text when both exist', () => {
  const out = mergeResults(
    [{ file_path: 'a.md', score: 1, excerpt: 'KW' }],
    [{ file_path: 'a.md', score: 1, chunk_text: 'SEM' }],
  )
  assert.equal(out[0].snippet, 'KW')
})

test('semantic chunk_text used when keyword excerpt missing', () => {
  const out = mergeResults(
    [{ file_path: 'a.md', score: 1 }],
    [{ file_path: 'a.md', score: 1, chunk_text: 'SEM' }],
  )
  assert.equal(out[0].snippet, 'SEM')
})
