// BL-029 Phase 2a — unit tests for the popout-mode shell helpers.
//
// We test the pure logic surface (`isPopoutMode`, `readPopoutInfo` via
// `isPopoutMode`, the exported event-name constant). The
// `installCloseHandshake` path drives Tauri's `getCurrentWindow()` /
// `emit()` which require the Tauri runtime, so we don't assert on it
// here — its contract is exercised end-to-end by the e2e suite once
// popout flows ship.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { isPopoutMode, POPOUT_CLOSED_EVENT } from './PopoutShell'

test('POPOUT_CLOSED_EVENT name is the documented `nexus:popout-closed`', () => {
  // The main-window listener in `main.tsx` and the popout emitter in
  // `PopoutShell.tsx` both reach for this constant. A typo on either
  // side would silently break popout close sync; pin the literal.
  assert.equal(POPOUT_CLOSED_EVENT, 'nexus:popout-closed')
})

test('isPopoutMode is false when the search string has no `popout` param', () => {
  assert.equal(isPopoutMode(''), false)
  assert.equal(isPopoutMode('?leaf=leaf-456'), false)
  assert.equal(isPopoutMode('?other=value'), false)
})

test('isPopoutMode is true when `popout` is present in the search string', () => {
  assert.equal(isPopoutMode('?popout=fw-123&leaf=leaf-456'), true)
  assert.equal(isPopoutMode('?popout=abc'), true)
})

test('isPopoutMode is true even when `popout` is empty (presence-only check)', () => {
  // Defensive: a malformed URL `?popout=` still routes the boot path
  // into popout mode rather than booting the full shell, which would
  // race the main window. The PopoutShell's render path renders
  // `(none)` for the fwId and sits idle.
  assert.equal(isPopoutMode('?popout='), true)
})
