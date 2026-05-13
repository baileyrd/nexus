import { test } from 'node:test'
import assert from 'node:assert/strict'
import { flattenTree, isBundleDir } from './flattenTree'
import type { FilesDirEntry } from './filesStore'

const dir = (name: string, relpath: string): FilesDirEntry => ({
  name,
  relpath,
  isDir: true,
})
const file = (name: string, relpath: string): FilesDirEntry => ({
  name,
  relpath,
  isDir: false,
})

test('flattenTree: collapsed root yields only top-level rows', () => {
  const root: FilesDirEntry[] = [dir('docs', 'docs'), file('readme.md', 'readme.md')]
  const out = flattenTree(root, {}, new Set(), 'nameAsc')
  assert.equal(out.length, 2)
  assert.deepEqual(
    out.map((r) => [r.entry.relpath, r.depth]),
    [
      ['docs', 0],
      ['readme.md', 0],
    ],
  )
})

test('flattenTree: expanded dir with cached children walks recursively', () => {
  const root: FilesDirEntry[] = [dir('docs', 'docs')]
  const children = {
    docs: [dir('adr', 'docs/adr'), file('intro.md', 'docs/intro.md')],
    'docs/adr': [file('0001.md', 'docs/adr/0001.md')],
  }
  const expanded = new Set(['docs', 'docs/adr'])
  const out = flattenTree(root, children, expanded, 'nameAsc')
  assert.deepEqual(
    out.map((r) => [r.entry.relpath, r.depth]),
    [
      ['docs', 0],
      ['docs/adr', 1],
      ['docs/adr/0001.md', 2],
      ['docs/intro.md', 1],
    ],
  )
})

test('flattenTree: expanded dir without cached children stops at the dir', () => {
  const root: FilesDirEntry[] = [dir('docs', 'docs')]
  const out = flattenTree(root, {}, new Set(['docs']), 'nameAsc')
  assert.deepEqual(out.map((r) => r.entry.relpath), ['docs'])
})

test('flattenTree: bundle dirs never recurse even when expanded', () => {
  const root: FilesDirEntry[] = [dir('mydb.bases', 'mydb.bases')]
  const children = { 'mydb.bases': [file('inner.md', 'mydb.bases/inner.md')] }
  const out = flattenTree(root, children, new Set(['mydb.bases']), 'nameAsc')
  assert.deepEqual(out.map((r) => r.entry.relpath), ['mydb.bases'])
})

test('flattenTree: sort mode applied at every level', () => {
  const root: FilesDirEntry[] = [file('b.md', 'b.md'), file('a.md', 'a.md')]
  const desc = flattenTree(root, {}, new Set(), 'nameDesc')
  assert.deepEqual(desc.map((r) => r.entry.relpath), ['b.md', 'a.md'])
})

test('flattenTree: dirs always come before files at the same level', () => {
  const root: FilesDirEntry[] = [file('a.md', 'a.md'), dir('z-folder', 'z-folder')]
  const out = flattenTree(root, {}, new Set(), 'nameAsc')
  assert.deepEqual(out.map((r) => r.entry.relpath), ['z-folder', 'a.md'])
})

test('isBundleDir: only fires on .bases directories', () => {
  assert.equal(isBundleDir(dir('x.bases', 'x.bases')), true)
  assert.equal(isBundleDir(dir('plain', 'plain')), false)
  assert.equal(isBundleDir(file('x.bases', 'x.bases')), false)
})
