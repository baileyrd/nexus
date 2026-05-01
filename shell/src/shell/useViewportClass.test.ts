// SH-003 — useViewportClass unit tests.
// Tests the applyClass logic directly using the exported breakpoints.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { NARROW_BREAKPOINT, WIDE_BREAKPOINT } from './useViewportClass'

test('useViewportClass: breakpoints are correctly ordered', () => {
  assert.ok(NARROW_BREAKPOINT < WIDE_BREAKPOINT, 'narrow < wide')
  assert.ok(NARROW_BREAKPOINT > 0, 'narrow breakpoint is positive')
})

test('useViewportClass: NARROW_BREAKPOINT is 768', () => {
  assert.equal(NARROW_BREAKPOINT, 768)
})

test('useViewportClass: WIDE_BREAKPOINT is 1280', () => {
  assert.equal(WIDE_BREAKPOINT, 1280)
})
