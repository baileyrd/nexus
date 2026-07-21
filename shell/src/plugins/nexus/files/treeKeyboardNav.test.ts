// C73 (#426) — regression coverage for the arrow-key tree-navigation
// index math, independent of React/DOM.
import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  firstChildIndex,
  nextVisibleIndex,
  parentIndex,
  prevVisibleIndex,
} from './treeKeyboardNav'
import type { FlatRow } from './flattenTree'
import type { FilesDirEntry } from './filesStore'

function entry(relpath: string, isDir = false): FilesDirEntry {
  return {
    relpath,
    name: relpath.includes('/') ? relpath.slice(relpath.lastIndexOf('/') + 1) : relpath,
    isDir,
  } as FilesDirEntry
}

// Mirrors a small expanded tree:
// 0 folderA/            (depth 0, dir)
// 1 folderA/child.md     (depth 1)
// 2 folderA/sub/         (depth 1, dir)
// 3 folderA/sub/leaf.md  (depth 2)
// 4 rootFile.md          (depth 0)
const ROWS: FlatRow[] = [
  { entry: entry('folderA', true), depth: 0 },
  { entry: entry('folderA/child.md'), depth: 1 },
  { entry: entry('folderA/sub', true), depth: 1 },
  { entry: entry('folderA/sub/leaf.md'), depth: 2 },
  { entry: entry('rootFile.md'), depth: 0 },
]

test('nextVisibleIndex advances by one and clamps at the last row', () => {
  assert.equal(nextVisibleIndex(ROWS, 0), 1)
  assert.equal(nextVisibleIndex(ROWS, 4), 4)
})

test('prevVisibleIndex retreats by one and clamps at the first row', () => {
  assert.equal(prevVisibleIndex(ROWS, 4), 3)
  assert.equal(prevVisibleIndex(ROWS, 0), 0)
})

test('nextVisibleIndex/prevVisibleIndex are no-ops on an empty row list', () => {
  assert.equal(nextVisibleIndex([], 0), 0)
  assert.equal(prevVisibleIndex([], 0), 0)
})

test('parentIndex finds the nearest shallower ancestor', () => {
  assert.equal(parentIndex(ROWS, 3), 2, 'leaf.md -> sub')
  assert.equal(parentIndex(ROWS, 1), 0, 'child.md -> folderA')
  assert.equal(parentIndex(ROWS, 2), 0, 'sub -> folderA')
})

test('parentIndex returns the same index at root depth (no parent)', () => {
  assert.equal(parentIndex(ROWS, 0), 0)
  assert.equal(parentIndex(ROWS, 4), 4)
})

test('firstChildIndex finds the immediately-following deeper row', () => {
  assert.equal(firstChildIndex(ROWS, 0), 1, 'folderA -> child.md')
  assert.equal(firstChildIndex(ROWS, 2), 3, 'sub -> leaf.md')
})

test('firstChildIndex returns null for a leaf or a row with no expanded children', () => {
  assert.equal(firstChildIndex(ROWS, 1), null, 'child.md is a file')
  assert.equal(firstChildIndex(ROWS, 3), null, 'leaf.md is a file')
  assert.equal(firstChildIndex(ROWS, 4), null, 'rootFile.md is a file, and the last row')
})
