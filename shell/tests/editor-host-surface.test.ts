/**
 * R10 / #193 — the editor host surface seam inverts the host→editor-plugin
 * dependency. These tests exercise the seam directly (registration,
 * delegation, absent-surface fallbacks) without importing the editor
 * plugin, proving the host no longer hard-depends on it.
 */

import { test, afterEach } from 'node:test'
import assert from 'node:assert/strict'
import type { ActiveEditor, FencedRenderer } from '../src/types/plugin'
import {
  registerEditorHostSurface,
  getEditorHostSurface,
  hasEditorHostSurface,
  __resetEditorHostSurfaceForTests,
} from '../src/host/EditorHostSurface'

afterEach(() => {
  __resetEditorHostSurfaceForTests()
})

test('no surface registered by default', () => {
  assert.equal(hasEditorHostSurface(), false)
  assert.equal(getEditorHostSurface(), null)
})

test('register exposes the surface and disposer clears it', () => {
  const noopRenderer: FencedRenderer = () => document.createElement('div')
  const surface = {
    getActiveEditor: (): ActiveEditor | null => ({ relpath: 'a.md', revision: 3 }),
    subscribeActiveEditor: () => () => {},
    registerFencedCodeRenderer: (_lang: string, _r: FencedRenderer) => () => {},
  }
  void noopRenderer
  const dispose = registerEditorHostSurface(surface)
  assert.equal(hasEditorHostSurface(), true)
  assert.deepEqual(getEditorHostSurface()?.getActiveEditor(), { relpath: 'a.md', revision: 3 })
  dispose()
  assert.equal(hasEditorHostSurface(), false)
  assert.equal(getEditorHostSurface(), null)
})

test('disposer is idempotent and only clears its own registration', () => {
  const first = {
    getActiveEditor: (): ActiveEditor | null => null,
    subscribeActiveEditor: () => () => {},
    registerFencedCodeRenderer: () => () => {},
  }
  const second = {
    getActiveEditor: (): ActiveEditor | null => ({ relpath: 'b.md', revision: 1 }),
    subscribeActiveEditor: () => () => {},
    registerFencedCodeRenderer: () => () => {},
  }
  const disposeFirst = registerEditorHostSurface(first)
  // Replacing the surface (e.g. a re-activation) supersedes the first.
  registerEditorHostSurface(second)
  // The first disposer must NOT clear the second's registration.
  disposeFirst()
  assert.equal(hasEditorHostSurface(), true)
  assert.deepEqual(getEditorHostSurface()?.getActiveEditor(), { relpath: 'b.md', revision: 1 })
})

test('subscribeActiveEditor delegates through the registered surface', () => {
  let captured: ((a: ActiveEditor | null) => void) | null = null
  let unsubbed = false
  registerEditorHostSurface({
    getActiveEditor: () => null,
    subscribeActiveEditor: (handler) => {
      captured = handler
      return () => {
        unsubbed = true
      }
    },
    registerFencedCodeRenderer: () => () => {},
  })
  const seen: Array<ActiveEditor | null> = []
  const unsub = getEditorHostSurface()!.subscribeActiveEditor((a) => seen.push(a))
  assert.equal(typeof captured, 'function')
  captured!({ relpath: 'c.md', revision: 7 })
  assert.deepEqual(seen, [{ relpath: 'c.md', revision: 7 }])
  unsub()
  assert.equal(unsubbed, true)
})
