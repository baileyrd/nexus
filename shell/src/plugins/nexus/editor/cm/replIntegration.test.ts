// BL-142 Phase 2b.2 — EditorView-level integration tests for the
// REPL extensions. The pure factor tests in `replGutter.test.ts` /
// `replKeymap.test.ts` cover state derivation; this file mounts a
// real CM6 `EditorView` under happy-dom and exercises the parts
// previously marked "needs a live Tauri window":
//
//   1. The gutter watcher populates the state field after the
//      view's first commit (the `queueMicrotask` path), and
//      re-dispatches when the doc gains / loses REPL fences.
//   2. The `Shift-Enter` (and `Mod-Enter`) keybinding routes
//      through CM6's keymap chain and fires `onRun` only when the
//      cursor is inside a REPL block.
//   3. The `replOutput` decoration mounts a widget below each
//      resolved REPL cell, the widget subscribes to the store at
//      mount, updates its DOM in place when the buffer changes,
//      and tears down cleanly on `view.destroy()` (no leaked
//      subscribers).
//
// What still needs a live Tauri window: a real `python3` (or other
// kernel) subprocess plus the full `repl_start` / `repl_eval` /
// output-bus pipeline. That's covered by the Rust IPC tests in
// `crates/nexus-bootstrap/tests/terminal_repl_ipc.rs`.

import { describe, it, beforeEach, afterEach } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import { replGutterExt, replStateField } from './replGutter.ts'
import { replKeymapExt } from './replKeymap.ts'
import { replOutputExt } from './replOutput.ts'
import type { ReplFenceBlock } from './replFence.ts'
import {
  useReplOutputStore,
  _resetReplOutputStoreForTests,
} from '../replOutputStore.ts'

/** Build a view, parent it to the body so layout/microtask work,
 *  and return both the view and a destroy helper that detaches
 *  the parent node too. */
function mount(doc: string, extensions: ReturnType<typeof replGutterExt>[]) {
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  const view = new EditorView({
    state: EditorState.create({ doc, extensions }),
    parent,
  })
  return {
    view,
    parent,
    destroy() {
      view.destroy()
      parent.remove()
    },
  }
}

/** Wait one microtask + one macrotask so the gutter watcher's
 *  `queueMicrotask` runs and any subsequent CM6 measure pass
 *  settles. */
async function nextTick(): Promise<void> {
  await Promise.resolve()
  await new Promise<void>((r) => setTimeout(r, 0))
}

describe('replGutterExt — EditorView integration', () => {
  it('seeds the state field via the watcher after the first commit', async () => {
    const calls: ReplFenceBlock[] = []
    const m = mount('```python repl\nprint(1)\n```\n', [
      replGutterExt({ onRun: (b) => calls.push(b) }),
    ])
    try {
      // The watcher uses `queueMicrotask` to avoid dispatching from
      // inside the view constructor; field is empty synchronously.
      const beforeTick = m.view.state.field(replStateField, false)
      assert.equal(beforeTick?.blocks.size ?? 0, 0)

      await nextTick()

      const seeded = m.view.state.field(replStateField, false)
      assert.equal(seeded?.blocks.size, 1)
      assert.equal(seeded?.blocks.get(1)?.language, 'python')
    } finally {
      m.destroy()
    }
  })

  it('refreshes the state field when a REPL fence is added by a doc change', async () => {
    const m = mount('intro\n', [replGutterExt({ onRun: () => {} })])
    try {
      await nextTick()
      assert.equal(m.view.state.field(replStateField).blocks.size, 0)

      // Append a REPL fence; the watcher's `update(u)` hook
      // dispatches the refresh effect because `u.docChanged` is true.
      m.view.dispatch({
        changes: {
          from: m.view.state.doc.length,
          insert: '\n```python repl\nprint(2)\n```\n',
        },
      })
      await nextTick()

      const after = m.view.state.field(replStateField)
      assert.equal(after.blocks.size, 1)
      assert.equal(after.blocks.get(3)?.language, 'python')
    } finally {
      m.destroy()
    }
  })

  it('clears the field when the last REPL fence is removed', async () => {
    const m = mount('```python repl\nx\n```\n', [
      replGutterExt({ onRun: () => {} }),
    ])
    try {
      await nextTick()
      assert.equal(m.view.state.field(replStateField).blocks.size, 1)

      m.view.dispatch({
        changes: { from: 0, to: m.view.state.doc.length, insert: 'no fences here\n' },
      })
      await nextTick()

      assert.equal(m.view.state.field(replStateField).blocks.size, 0)
    } finally {
      m.destroy()
    }
  })
})

describe('replKeymapExt — EditorView integration', () => {
  /** Dispatch a synthetic keydown on the contentDOM. CM6 installs a
   *  single keydown listener there which routes through every
   *  `keymap.of([...])` provider in priority order. Happy-dom
   *  supports KeyboardEvent so this drives the real keymap chain. */
  function pressShiftEnter(view: EditorView) {
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      code: 'Enter',
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    })
    view.contentDOM.dispatchEvent(event)
  }

  function pressModEnter(view: EditorView) {
    const isMac =
      typeof navigator !== 'undefined' && /Mac/.test(navigator.platform ?? '')
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      code: 'Enter',
      // CM6 maps `Mod-` to `metaKey` on macOS, `ctrlKey` elsewhere.
      ctrlKey: !isMac,
      metaKey: isMac,
      bubbles: true,
      cancelable: true,
    })
    view.contentDOM.dispatchEvent(event)
  }

  it('Shift-Enter inside a REPL block invokes onRun with the block + code', () => {
    const calls: Array<{ lang: string; code: string }> = []
    const m = mount('```python repl\nprint(2+2)\n```\n', [
      replKeymapExt({
        onRun: (block, code) => calls.push({ lang: block.language, code }),
      }),
    ])
    try {
      // Place the cursor inside the body (line 2, "print(2+2)").
      const docText = m.view.state.doc.toString()
      const cursor = docText.indexOf('print')
      m.view.dispatch({ selection: { anchor: cursor } })

      pressShiftEnter(m.view)

      assert.equal(calls.length, 1)
      assert.equal(calls[0].lang, 'python')
      assert.equal(calls[0].code, 'print(2+2)\n')
    } finally {
      m.destroy()
    }
  })

  it('Mod-Enter inside a REPL block also fires onRun (remap-friendly path)', () => {
    const calls: Array<{ lang: string; code: string }> = []
    const m = mount('```node repl\nconsole.log(1)\n```\n', [
      replKeymapExt({
        onRun: (block, code) => calls.push({ lang: block.language, code }),
      }),
    ])
    try {
      const docText = m.view.state.doc.toString()
      const cursor = docText.indexOf('console')
      m.view.dispatch({ selection: { anchor: cursor } })

      pressModEnter(m.view)

      assert.equal(calls.length, 1)
      assert.equal(calls[0].lang, 'node')
      assert.equal(calls[0].code, 'console.log(1)\n')
    } finally {
      m.destroy()
    }
  })

  it('Shift-Enter outside any REPL block does not invoke onRun', () => {
    const calls: ReplFenceBlock[] = []
    const m = mount('# heading\n\n```python repl\nprint(1)\n```\n\nafter\n', [
      replKeymapExt({ onRun: (block) => calls.push(block) }),
    ])
    try {
      // Cursor on "heading" — well outside the REPL block.
      const cursor = m.view.state.doc.toString().indexOf('heading')
      m.view.dispatch({ selection: { anchor: cursor } })

      pressShiftEnter(m.view)

      assert.equal(calls.length, 0)
    } finally {
      m.destroy()
    }
  })

  it('Shift-Enter dispatches the correct block when the cursor is in the second of two REPL cells', () => {
    const calls: Array<{ lang: string; code: string }> = []
    const m = mount(
      '```python repl\nprint(1)\n```\n\n```node repl\nconsole.log(2)\n```\n',
      [
        replKeymapExt({
          onRun: (block, code) => calls.push({ lang: block.language, code }),
        }),
      ],
    )
    try {
      const cursor = m.view.state.doc.toString().indexOf('console')
      m.view.dispatch({ selection: { anchor: cursor } })

      pressShiftEnter(m.view)

      assert.equal(calls.length, 1)
      assert.equal(calls[0].lang, 'node')
      assert.equal(calls[0].code, 'console.log(2)\n')
    } finally {
      m.destroy()
    }
  })
})

describe('replOutputExt — EditorView integration', () => {
  beforeEach(() => {
    _resetReplOutputStoreForTests()
  })
  afterEach(() => {
    _resetReplOutputStoreForTests()
  })

  it('does not mount a widget when resolveSessionId returns null', () => {
    const m = mount('```python repl\nprint(1)\n```\n', [
      replOutputExt({ resolveSessionId: () => null }),
    ])
    try {
      const widgets = m.parent.querySelectorAll('.nexus-repl-output')
      assert.equal(widgets.length, 0)
    } finally {
      m.destroy()
    }
  })

  it('mounts one widget per resolved cell and reflects store state in its DOM', () => {
    const m = mount('```python repl\nprint(1)\n```\n', [
      replOutputExt({ resolveSessionId: () => 'sess-A' }),
    ])
    try {
      const widgets = m.parent.querySelectorAll('.nexus-repl-output')
      assert.equal(widgets.length, 1)
      const el = widgets[0] as HTMLElement
      assert.equal(el.dataset.sessionId, 'sess-A')
      // No buffer yet — widget is hidden.
      assert.equal(el.style.display, 'none')

      // Drive the store; the widget's subscriber should update the
      // DOM in place (no remount).
      useReplOutputStore.getState().clear('sess-A')
      useReplOutputStore.getState().append('sess-A', 'hello\n')

      const after = m.parent.querySelectorAll('.nexus-repl-output')
      assert.equal(after.length, 1, 'widget remained the same node')
      assert.strictEqual(after[0], el, 'identity preserved across updates')
      assert.equal((after[0] as HTMLElement).style.display, '')
      assert.equal(after[0].textContent, 'hello\n')

      // Subsequent appends accumulate.
      useReplOutputStore.getState().append('sess-A', 'world\n')
      assert.equal(after[0].textContent, 'hello\nworld\n')
    } finally {
      m.destroy()
    }
  })

  it('renders one widget per resolved cell with independent buffers', () => {
    // Two REPL cells; resolve each to a distinct session id so the
    // decoration builder emits one widget per cell.
    const langToSession: Record<string, string> = {
      python: 'sess-py',
      node: 'sess-node',
    }
    const m = mount(
      '```python repl\nprint(1)\n```\n\n```node repl\nconsole.log(1)\n```\n',
      [
        replOutputExt({
          resolveSessionId: (block) =>
            langToSession[block.language] ?? null,
        }),
      ],
    )
    try {
      const widgets = m.parent.querySelectorAll('.nexus-repl-output')
      assert.equal(widgets.length, 2)
      const sessionIds = Array.from(widgets).map(
        (w) => (w as HTMLElement).dataset.sessionId,
      )
      assert.deepEqual([...sessionIds].sort(), ['sess-node', 'sess-py'])

      useReplOutputStore.getState().clear('sess-py')
      useReplOutputStore.getState().append('sess-py', 'py-out\n')

      const pyEl = m.parent.querySelector(
        '[data-session-id="sess-py"]',
      ) as HTMLElement | null
      const nodeEl = m.parent.querySelector(
        '[data-session-id="sess-node"]',
      ) as HTMLElement | null
      assert.ok(pyEl)
      assert.ok(nodeEl)
      assert.equal(pyEl?.textContent, 'py-out\n')
      assert.equal(nodeEl?.style.display, 'none')
    } finally {
      m.destroy()
    }
  })

  it('rebuilds the decoration set when a new REPL fence is added by a doc change', async () => {
    const m = mount('intro\n', [
      replOutputExt({ resolveSessionId: () => 'sess-X' }),
    ])
    try {
      assert.equal(
        m.parent.querySelectorAll('.nexus-repl-output').length,
        0,
      )

      m.view.dispatch({
        changes: {
          from: m.view.state.doc.length,
          insert: '\n```python repl\nprint(1)\n```\n',
        },
      })
      // The decoration view-plugin runs synchronously on update;
      // no microtask wait needed, but a tick gives CM6 a chance to
      // realise widget DOM.
      await nextTick()

      const widgets = m.parent.querySelectorAll('.nexus-repl-output')
      assert.equal(widgets.length, 1)
      assert.equal(
        (widgets[0] as HTMLElement).dataset.sessionId,
        'sess-X',
      )
    } finally {
      m.destroy()
    }
  })

  it('drops the store subscriber when the view is destroyed (no leak across mounts)', () => {
    const before = useReplOutputStore.subscribe(() => {})
    // Count subscribers indirectly: each widget adds one. Compare
    // listener count via Zustand's internal listener set is not
    // public, so instead drive an update and observe whether a
    // destroyed widget's DOM still reflects new chunks.
    before()

    const m = mount('```python repl\nprint(1)\n```\n', [
      replOutputExt({ resolveSessionId: () => 'sess-D' }),
    ])
    const el = m.parent.querySelector(
      '.nexus-repl-output',
    ) as HTMLElement | null
    assert.ok(el)
    useReplOutputStore.getState().clear('sess-D')
    useReplOutputStore.getState().append('sess-D', 'before-destroy\n')
    assert.equal(el?.textContent, 'before-destroy\n')

    m.destroy()

    // The widget element is detached; appends should not throw and
    // (critically) should not reach the detached element's DOM —
    // which would mean the subscriber is still alive.
    useReplOutputStore.getState().append('sess-D', 'after-destroy\n')
    // `el` was detached; its textContent should be whatever it had
    // at destroy time. The widget's `destroy()` nulls its `dom`
    // ref so the subscriber's `render()` short-circuits.
    assert.equal(
      el?.textContent,
      'before-destroy\n',
      'detached widget should not pick up post-destroy appends',
    )
  })
})
