// C68 (#421) — regression coverage for the "Copy as rich text" clipboard
// helper: the multi-MIME write, the plain-text derivation, and both
// fallback paths (no ClipboardItem support; a rejecting write()).
//
// happy-dom's `navigator` is a getter-only accessor
// (`Object.getOwnPropertyDescriptor(globalThis, 'navigator')` has no
// setter), so a plain `globalThis.navigator = {...}` assignment silently
// no-ops — stubs must go through `Object.defineProperty` instead.
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { copyRichTextToClipboard, htmlToPlainText } from './richTextClipboard'

function stubNavigator(value: unknown): () => void {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator')
  Object.defineProperty(globalThis, 'navigator', { value, configurable: true, writable: true })
  return () => {
    if (original) Object.defineProperty(globalThis, 'navigator', original)
  }
}

test('htmlToPlainText strips tags and keeps the text content', () => {
  const text = htmlToPlainText('<p><b>Hello</b> <em>world</em></p>')
  assert.equal(text, 'Hello world')
})

test('htmlToPlainText returns an empty string for empty HTML', () => {
  assert.equal(htmlToPlainText(''), '')
})

test('copyRichTextToClipboard writes a text/html + text/plain ClipboardItem when supported', async () => {
  const written: ClipboardItems = []
  const restore = stubNavigator({
    clipboard: {
      write: (items: ClipboardItems) => {
        written.push(...items)
        return Promise.resolve()
      },
    },
  })
  try {
    await copyRichTextToClipboard('<p>Hello <b>world</b></p>')
    assert.equal(written.length, 1)
    const item = written[0]
    assert.ok(item.types.includes('text/html'))
    assert.ok(item.types.includes('text/plain'))
    const htmlBlob = await item.getType('text/html')
    const plainBlob = await item.getType('text/plain')
    assert.equal(await htmlBlob.text(), '<p>Hello <b>world</b></p>')
    assert.equal(await plainBlob.text(), 'Hello world')
  } finally {
    restore()
  }
})

test('copyRichTextToClipboard falls back to writeText when ClipboardItem is unavailable', async () => {
  let plainWritten: string | undefined
  const originalClipboardItem = (globalThis as { ClipboardItem?: unknown }).ClipboardItem
  const restore = stubNavigator({
    clipboard: {
      writeText: (text: string) => {
        plainWritten = text
        return Promise.resolve()
      },
    },
  })
  ;(globalThis as { ClipboardItem?: unknown }).ClipboardItem = undefined
  try {
    await copyRichTextToClipboard('<p>Hello <b>world</b></p>')
    assert.equal(plainWritten, 'Hello world')
  } finally {
    restore()
    ;(globalThis as { ClipboardItem?: unknown }).ClipboardItem = originalClipboardItem
  }
})

test('copyRichTextToClipboard falls back to writeText when the multi-MIME write rejects', async () => {
  let plainWritten: string | undefined
  const restore = stubNavigator({
    clipboard: {
      write: () => Promise.reject(new Error('NotAllowedError')),
      writeText: (text: string) => {
        plainWritten = text
        return Promise.resolve()
      },
    },
  })
  try {
    await copyRichTextToClipboard('<p>Hello <b>world</b></p>')
    assert.equal(plainWritten, 'Hello world')
  } finally {
    restore()
  }
})

test('copyRichTextToClipboard throws when navigator.clipboard is unavailable', async () => {
  const restore = stubNavigator({})
  try {
    await assert.rejects(() => copyRichTextToClipboard('<p>hi</p>'))
  } finally {
    restore()
  }
})
