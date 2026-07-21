// #384 — unit tests for the pure export-filename slugifier behind
// "export session as note" (aiRuntime.exportSessionAsNote).

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { slugForExport } from './aiRuntime.ts'

test('lowercases and hyphenates a normal title', () => {
  assert.equal(slugForExport('Project X Planning'), 'project-x-planning')
})

test('collapses runs of non-alphanumeric characters into a single hyphen', () => {
  assert.equal(slugForExport('foo!!  bar??baz'), 'foo-bar-baz')
})

test('trims leading and trailing hyphens', () => {
  assert.equal(slugForExport('  --hello--  '), 'hello')
})

test('falls back to "session" when nothing alphanumeric survives', () => {
  assert.equal(slugForExport('!!!'), 'session')
  assert.equal(slugForExport(''), 'session')
})

test('passes an already-safe id through unchanged', () => {
  assert.equal(slugForExport('s-abc123-def456'), 's-abc123-def456')
})
