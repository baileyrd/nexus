// BL-070: vim keymap unit tests. Drives the `:w`/`:q` ex commands and
// the modal layer through a real `EditorView` so the assertion path
// matches what the CodeMirror host runs at mount time.
//
// Re-exported via `shell/tests/vim-keymap.test.ts` so the top-level
// `pnpm test` glob picks these up.

// `happy-dom` globals (`document`, `window`) are registered via the
// test runner's `--import ./tests/setup/happy-dom.ts` flag before
// this file loads.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { Vim, getCM } from '@replit/codemirror-vim'

import { vimKeymapExt } from './vimKeymap.ts'

function mountWithVim(opts: {
  doc?: string
  relpath?: string
  onSave?: () => void
  onClose?: () => void
}): EditorView {
  const state = EditorState.create({
    doc: opts.doc ?? '',
    extensions: [
      vimKeymapExt({
        relpath: opts.relpath ?? 'notes/foo.md',
        onSave: opts.onSave ?? (() => {}),
        onClose: opts.onClose ?? (() => {}),
      }),
    ],
  })
  // Attach to a detached parent so the view has a host element to
  // mount its status panel into.
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  return new EditorView({ state, parent })
}

test('vim mounts and exposes the CM5 wrapper via getCM', () => {
  const view = mountWithVim({ doc: 'hello world' })
  try {
    const cm = getCM(view)
    assert.ok(cm, 'getCM returns the wrapper for a vim-enabled view')
    assert.equal(cm.cm6, view)
  } finally {
    view.destroy()
  }
})

test(':w routes through onSave for the active view', () => {
  let saved = 0
  const view = mountWithVim({
    onSave: () => {
      saved += 1
    },
  })
  try {
    const cm = getCM(view)
    assert.ok(cm)
    Vim.handleEx(cm, 'w')
    assert.equal(saved, 1, ':w fires onSave exactly once')
  } finally {
    view.destroy()
  }
})

test(':q routes through onClose for the active view', () => {
  let closed = 0
  const view = mountWithVim({
    onClose: () => {
      closed += 1
    },
  })
  try {
    const cm = getCM(view)
    assert.ok(cm)
    Vim.handleEx(cm, 'q')
    assert.equal(closed, 1)
  } finally {
    view.destroy()
  }
})

test(':wq fires save then close in order', () => {
  const order: string[] = []
  const view = mountWithVim({
    onSave: () => order.push('save'),
    onClose: () => order.push('close'),
  })
  try {
    const cm = getCM(view)
    assert.ok(cm)
    Vim.handleEx(cm, 'wq')
    assert.deepEqual(order, ['save', 'close'])
  } finally {
    view.destroy()
  }
})

test('per-view context: two mounted views dispatch to their own callbacks', () => {
  const a: string[] = []
  const b: string[] = []
  const viewA = mountWithVim({
    relpath: 'notes/a.md',
    onSave: () => a.push('save-a'),
  })
  const viewB = mountWithVim({
    relpath: 'notes/b.md',
    onSave: () => b.push('save-b'),
  })
  try {
    Vim.handleEx(getCM(viewA)!, 'w')
    Vim.handleEx(getCM(viewB)!, 'w')
    assert.deepEqual(a, ['save-a'])
    assert.deepEqual(b, ['save-b'])
  } finally {
    viewA.destroy()
    viewB.destroy()
  }
})
