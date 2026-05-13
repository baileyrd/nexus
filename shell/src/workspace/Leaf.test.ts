// Unit tests for LeafImpl. Uses node:test to avoid extra devDeps.
// Run with: node --experimental-strip-types --test src/workspace/Leaf.test.ts
//
// Imports of node built-ins are string-indirected so tsc type-checks without
// @types/node installed — same pattern as ViewRegistry.test.ts.

import type { Leaf, View, WorkspaceParent } from './types.ts'
import { viewRegistry } from './ViewRegistry.ts'
import { LeafImpl } from './Leaf.ts'
import { ViewBase } from './View.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

// --- fixtures ------------------------------------------------------------

interface Calls {
  setState: number
  onOpen: number
  onClose: number
  lastEl: HTMLElement | null
  lastState: unknown
}

function makeRecordingViewClass(type: string, calls: Calls) {
  class RecordingView extends ViewBase {
    readonly viewType = type
    private _state: unknown = {}
    override getState(): unknown {
      return this._state
    }
    override setState(state: unknown, _eState?: unknown): void {
      calls.setState++
      this._state = state
    }
    override onOpen(el: HTMLElement): void {
      calls.onOpen++
      calls.lastEl = el
    }
    override onClose(): void {
      calls.onClose++
    }
  }
  return RecordingView
}

function freshCalls(): Calls {
  return { setState: 0, onOpen: 0, onClose: 0, lastEl: null, lastState: null }
}

function makeFakeEl(): HTMLElement {
  const el = {
    children: [] as unknown[],
    replaceChildren() {
      this.children = []
    },
  }
  return el as unknown as HTMLElement
}

const fakeParent = { kind: 'tabs', id: 'p', leaves: [], activeIndex: 0 } as unknown as WorkspaceParent

// --- tests ---------------------------------------------------------------

test('setViewState without containerEl stashes onOpen until attachContainer', async () => {
  const calls = freshCalls()
  const dispose = viewRegistry.register('foo', (l: Leaf) => new (makeRecordingViewClass('foo', calls))(l))
  try {
    const leaf = new LeafImpl(fakeParent)
    await leaf.setViewState({ type: 'foo' })
    assert.ok(leaf.view)
    assert.equal(leaf.view!.viewType, 'foo')
    assert.equal(calls.onOpen, 0, 'onOpen must not fire without containerEl')

    const fakeEl = makeFakeEl()
    await leaf.attachContainer(fakeEl)
    assert.equal(calls.onOpen, 1, 'onOpen fires once on attach')
    assert.equal(calls.lastEl, fakeEl)
  } finally {
    dispose()
  }
})

test('second setViewState calls first view onClose before second onOpen', async () => {
  const fooCalls = freshCalls()
  const barCalls = freshCalls()
  const dFoo = viewRegistry.register('foo2', (l: Leaf) => new (makeRecordingViewClass('foo2', fooCalls))(l))
  const dBar = viewRegistry.register('bar2', (l: Leaf) => new (makeRecordingViewClass('bar2', barCalls))(l))
  try {
    const leaf = new LeafImpl(fakeParent)
    const el = makeFakeEl()
    await leaf.attachContainer(el)
    await leaf.setViewState({ type: 'foo2' })
    assert.equal(fooCalls.onOpen, 1)
    await leaf.setViewState({ type: 'bar2' })
    assert.equal(fooCalls.onClose, 1, 'foo.onClose fires once')
    assert.equal(barCalls.onOpen, 1, 'bar.onOpen fires once')
    assert.equal(leaf.view!.viewType, 'bar2')
  } finally {
    dFoo()
    dBar()
  }
})

test('unknown view type falls back to empty without throwing', async () => {
  const leaf = new LeafImpl(fakeParent)
  await leaf.setViewState({ type: 'definitely-not-registered-xyz' })
  assert.ok(leaf.view)
  assert.equal(leaf.view!.viewType, 'empty')
})

test('getViewState round-trips type and state', async () => {
  const calls = freshCalls()
  const dispose = viewRegistry.register('rt', (l: Leaf) => new (makeRecordingViewClass('rt', calls))(l))
  try {
    const leaf = new LeafImpl(fakeParent)
    await leaf.setViewState({ type: 'rt', state: { path: 'x.md' } })
    const vs = leaf.getViewState()
    assert.equal(vs.type, 'rt')
    assert.deepEqual(vs.state, { path: 'x.md' })
    assert.equal(vs.pinned, undefined)
    assert.equal(vs.group, undefined)
  } finally {
    dispose()
  }
})

test('setViewState with active:true emits active-leaf-change', async () => {
  const calls = freshCalls()
  const events: Array<{ name: string; payload: unknown }> = []
  const emit = (name: string, payload?: unknown) => {
    events.push({ name, payload })
  }
  const dispose = viewRegistry.register('act', (l: Leaf) => new (makeRecordingViewClass('act', calls))(l))
  try {
    const leaf = new LeafImpl(fakeParent, emit)
    await leaf.setViewState({ type: 'act', active: true })
    const names = events.map(e => e.name)
    assert.ok(names.includes('view-changed'), 'view-changed should be emitted')
    assert.ok(names.includes('active-leaf-change'), 'active-leaf-change should be emitted')
  } finally {
    dispose()
  }
})

test('detach calls onClose and nulls view; later attachContainer is inert', async () => {
  const calls = freshCalls()
  const dispose = viewRegistry.register('det', (l: Leaf) => new (makeRecordingViewClass('det', calls))(l))
  try {
    const leaf = new LeafImpl(fakeParent)
    const el = makeFakeEl()
    await leaf.attachContainer(el)
    await leaf.setViewState({ type: 'det' })
    assert.equal(calls.onOpen, 1)
    await leaf.detach()
    assert.equal(calls.onClose, 1)
    assert.equal(leaf.view, null)

    // subsequent attach must not re-open — view is gone
    const el2 = makeFakeEl()
    await leaf.attachContainer(el2)
    assert.equal(calls.onOpen, 1, 'onOpen must not fire again after detach')
  } finally {
    dispose()
  }
})

// Skipped 2026-05-13 alongside the test-runner audit (BL-110 follow-up
// spike): the assertion expects re-attach after `attachContainer(null)`
// to be a no-op for `onOpen`, but commit 9a541afa
// ("re-home view DOM on sidebar collapse/reopen") intentionally
// changed LeafImpl to re-fire `onOpen` so the view rebinds to the new
// container element after a sidebar collapse/expand. Either the spec
// or the impl needs to win — the workspace owner should pick. Kept as
// documentation of the original intent rather than deleted.
test('containerEl=null is a transient unmount — onClose does NOT fire; re-attach does not re-open', { skip: 'spec/impl drift since 9a541afa — see comment above' }, async () => {
  const calls = freshCalls()
  const dispose = viewRegistry.register('rem', (l: Leaf) => new (makeRecordingViewClass('rem', calls))(l))
  try {
    const leaf = new LeafImpl(fakeParent)
    const el = makeFakeEl()
    await leaf.attachContainer(el)
    await leaf.setViewState({ type: 'rem' })
    assert.equal(calls.onOpen, 1)
    assert.equal(calls.onClose, 0)

    // transient unmount
    await leaf.attachContainer(null)
    assert.equal(calls.onClose, 0, 'onClose must not fire on transient unmount')

    // re-mount — already opened, must not re-invoke onOpen
    const el2 = makeFakeEl()
    await leaf.attachContainer(el2)
    assert.equal(calls.onOpen, 1, 'onOpen must not repeat on re-mount without a new setViewState')
  } finally {
    dispose()
  }
})
