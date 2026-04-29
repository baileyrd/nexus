// Runtime smoke test for the live-preview extension.
//
// Pure-builder tests (livePreviewDecorations.test.ts) cover decoration
// shape but cannot catch CM6 mount-time constraints — see commit 29e637c
// where block decorations from a ViewPlugin throw `RangeError: Block
// decorations may not be specified via plugins`. This file mounts an
// `EditorView` for real and asserts no throw on construction or
// selection changes.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { markdown } from '@codemirror/lang-markdown'
import { livePreviewExt } from './livePreview'

test('livePreview: mounts an EditorView with table/hr/heading widgets without throwing', () => {
  const doc = [
    '# Heading',
    '',
    '---',
    '',
    '| a | b |',
    '| - | - |',
    '| 1 | 2 |',
    '',
    'paragraph',
  ].join('\n')

  const parent = document.createElement('div')
  document.body.appendChild(parent)

  let view: EditorView | undefined
  try {
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown(), livePreviewExt()],
      }),
      parent,
    })

    // Move the selection across the block-decoration regions to trigger
    // a recompute — historically the second call is what blew up.
    view.dispatch({ selection: { anchor: doc.indexOf('---') } })
    view.dispatch({ selection: { anchor: doc.indexOf('| 1') } })
    view.dispatch({ selection: { anchor: doc.indexOf('paragraph') } })

    assert.ok(view.dom, 'EditorView should have mounted a DOM node')
  } finally {
    view?.destroy()
    parent.remove()
  }
})
