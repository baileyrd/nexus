// Unit tests for ViewRegistry. Uses node:test so no extra devDep is needed;
// run with: node --experimental-strip-types --test src/workspace/ViewRegistry.test.ts
//
// node:test and node:assert are loaded dynamically so this file type-checks
// without @types/node installed.

import type { Leaf, View } from './types.ts'
import { viewRegistry } from './ViewRegistry.ts'

// String-indirected imports keep tsc from trying to resolve node built-ins
// (which would need @types/node). Runtime still loads them normally.
import { test } from 'node:test'
import assert from 'node:assert/strict'

const fakeLeaf = {} as Leaf

const makeCreator = (viewType: string) => (leaf: Leaf): View => ({
  viewType,
  leaf,
  getState: () => ({}),
  setState: () => {},
  onOpen: () => {},
  onClose: () => {},
})

test('register then getCreator returns the same creator', () => {
  const creator = makeCreator('markdown')
  const dispose = viewRegistry.register('markdown', creator)
  assert.equal(viewRegistry.getCreator('markdown'), creator)
  dispose()
})

test('unregister removes the creator', () => {
  const creator = makeCreator('graph')
  const dispose = viewRegistry.register('graph', creator)
  assert.equal(viewRegistry.getCreator('graph'), creator)
  dispose()
  assert.equal(viewRegistry.getCreator('graph'), null)
})

test('empty is always resolvable after module load', () => {
  const empty = viewRegistry.getCreator('empty')
  assert.ok(empty, 'empty creator should be registered at module load')
  const view = empty!(fakeLeaf)
  assert.equal(view.viewType, 'empty')
})

test('registerExtensions maps all extensions and disposer removes them', () => {
  const dispose = viewRegistry.registerExtensions(['md', 'markdown'], 'markdown')
  assert.equal(viewRegistry.getTypeForExt('md'), 'markdown')
  assert.equal(viewRegistry.getTypeForExt('markdown'), 'markdown')
  dispose()
  assert.equal(viewRegistry.getTypeForExt('md'), null)
  assert.equal(viewRegistry.getTypeForExt('markdown'), null)
})

test('getTypeForExt returns null for unknown extensions', () => {
  assert.equal(viewRegistry.getTypeForExt('xyzzy-unknown'), null)
})

test('registerExtensions disposer keeps mappings overwritten by a later call', () => {
  const disposeA = viewRegistry.registerExtensions(['csv'], 'table')
  viewRegistry.registerExtensions(['csv'], 'spreadsheet')
  disposeA()
  assert.equal(viewRegistry.getTypeForExt('csv'), 'spreadsheet')
})
