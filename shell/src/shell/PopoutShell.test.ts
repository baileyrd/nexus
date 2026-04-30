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

import { isPopoutMode, POPOUT_CLOSED_EVENT, findLeafInNode } from './PopoutShell'
import type {
  FloatingWindow,
  Leaf,
  Split,
  Tabs,
} from '../workspace/types'

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

// BL-029 Phase 2b — findLeafInNode walks a FloatingWindow subtree and
// returns the matching Leaf or null. Pure logic; doesn't touch the
// workspace store. Used by `resolveLeaf` during popout hydration.
function makeLeafStub(id: string): Leaf {
  return { id } as unknown as Leaf
}

test('findLeafInNode finds a leaf in a Tabs node', () => {
  const a = makeLeafStub('leaf-a')
  const b = makeLeafStub('leaf-b')
  const tabs: Tabs = { kind: 'tabs', id: 't1', leaves: [a, b], activeIndex: 0 }
  const fw: FloatingWindow = { kind: 'floating', id: 'fw-1', child: tabs }
  assert.equal(findLeafInNode(fw, 'leaf-b'), b)
})

test('findLeafInNode returns null when the leaf id is not present', () => {
  const a = makeLeafStub('leaf-a')
  const tabs: Tabs = { kind: 'tabs', id: 't1', leaves: [a], activeIndex: 0 }
  const fw: FloatingWindow = { kind: 'floating', id: 'fw-1', child: tabs }
  assert.equal(findLeafInNode(fw, 'leaf-missing'), null)
})

test('findLeafInNode recurses through nested Splits', () => {
  // Phase-3 multi-leaf popouts could host a Split inside the FW. The
  // single-leaf shape today uses a plain Tabs, but the walker needs to
  // be future-proof.
  const target = makeLeafStub('leaf-target')
  const inner: Tabs = {
    kind: 'tabs',
    id: 't1',
    leaves: [target],
    activeIndex: 0,
  }
  const split: Split = {
    kind: 'split',
    id: 's1',
    direction: 'horizontal',
    children: [inner],
  }
  const fw: FloatingWindow = { kind: 'floating', id: 'fw-1', child: split }
  assert.equal(findLeafInNode(fw, 'leaf-target'), target)
})
