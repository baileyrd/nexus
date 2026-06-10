/**
 * V16 — the workspace host surface seam inverts the host→workspace-plugin
 * dependency (same treatment as #193's EditorHostSurface). These tests
 * exercise the seam directly (registration, delegation, absent-surface
 * fallbacks, late-registration re-binding) without importing the workspace
 * plugin, proving the host no longer hard-depends on it.
 */

import { test, afterEach } from 'node:test'
import assert from 'node:assert/strict'
import {
  registerWorkspaceHostSurface,
  getWorkspaceHostSurface,
  hasWorkspaceHostSurface,
  getWorkspaceRootPath,
  subscribeWorkspaceRootPath,
  __resetWorkspaceHostSurfaceForTests,
  type WorkspaceHostSurface,
} from '../src/host/WorkspaceHostSurface'

afterEach(() => {
  __resetWorkspaceHostSurfaceForTests()
})

/** Minimal surface over a mutable root path, with a manual fan-out. */
function makeSurface(initial: string | null = null): {
  surface: WorkspaceHostSurface
  setRoot(path: string | null): void
} {
  let root = initial
  const handlers = new Set<(rootPath: string | null) => void>()
  return {
    surface: {
      getRootPath: () => root,
      subscribeRootPath: (handler) => {
        handlers.add(handler)
        return () => handlers.delete(handler)
      },
    },
    setRoot(path) {
      if (path === root) return
      root = path
      for (const h of handlers) h(path)
    },
  }
}

test('no surface registered by default; reads fall back to null', () => {
  assert.equal(hasWorkspaceHostSurface(), false)
  assert.equal(getWorkspaceHostSurface(), null)
  assert.equal(getWorkspaceRootPath(), null)
})

test('register exposes the surface and disposer clears it', () => {
  const { surface } = makeSurface('/forge/a')
  const dispose = registerWorkspaceHostSurface(surface)
  assert.equal(hasWorkspaceHostSurface(), true)
  assert.equal(getWorkspaceRootPath(), '/forge/a')
  dispose()
  assert.equal(hasWorkspaceHostSurface(), false)
  assert.equal(getWorkspaceRootPath(), null)
})

test('disposer is idempotent and only clears its own registration', () => {
  const first = makeSurface(null)
  const second = makeSurface('/forge/b')
  const disposeFirst = registerWorkspaceHostSurface(first.surface)
  // Replacing the surface (e.g. a re-activation) supersedes the first.
  registerWorkspaceHostSurface(second.surface)
  // The first disposer must NOT clear the second's registration.
  disposeFirst()
  assert.equal(hasWorkspaceHostSurface(), true)
  assert.equal(getWorkspaceRootPath(), '/forge/b')
})

test('subscribeWorkspaceRootPath delegates through the registered surface', () => {
  const { surface, setRoot } = makeSurface(null)
  registerWorkspaceHostSurface(surface)
  let fired = 0
  const unsub = subscribeWorkspaceRootPath(() => {
    fired += 1
  })
  setRoot('/forge/c')
  assert.equal(fired, 1)
  assert.equal(getWorkspaceRootPath(), '/forge/c')
  unsub()
  setRoot('/forge/d')
  assert.equal(fired, 1, 'unsubscribed handler must not fire')
})

test('late registration re-binds an existing subscription (chrome mounts before the plugin activates)', () => {
  // Subscribe BEFORE any surface exists — the normal boot ordering for
  // ForgeSelector / RightPanelFooter.
  let fired = 0
  const unsub = subscribeWorkspaceRootPath(() => {
    fired += 1
  })
  assert.equal(getWorkspaceRootPath(), null)

  const { surface, setRoot } = makeSurface(null)
  registerWorkspaceHostSurface(surface)
  assert.equal(fired, 1, 'registration itself notifies so consumers re-read the snapshot')

  setRoot('/forge/e')
  assert.equal(fired, 2, 'value changes flow through the late-bound surface')
  assert.equal(getWorkspaceRootPath(), '/forge/e')
  unsub()
})

test('re-registration swaps the value subscription to the new surface', () => {
  const first = makeSurface('/forge/old')
  registerWorkspaceHostSurface(first.surface)
  let fired = 0
  const unsub = subscribeWorkspaceRootPath(() => {
    fired += 1
  })

  const second = makeSurface('/forge/new')
  registerWorkspaceHostSurface(second.surface)
  assert.equal(fired, 1, 'replacement notifies')
  assert.equal(getWorkspaceRootPath(), '/forge/new')

  first.setRoot('/forge/stale')
  assert.equal(fired, 1, 'old surface no longer feeds the subscription')
  second.setRoot('/forge/newer')
  assert.equal(fired, 2)
  assert.equal(getWorkspaceRootPath(), '/forge/newer')
  unsub()
})
