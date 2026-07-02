// C1 (#354) — unit tests for the paste/drop attachment importer's
// pure helpers. The full CM event path is exercised manually / via
// e2e; these pin the file-classification decisions.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/editor-attachment-paste.test.ts`
// re-export shim).

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { attachmentNameFor, filesFromDataTransfer } from './attachmentPaste.ts'

test('filesFromDataTransfer: null / file-less payloads → []', () => {
  assert.deepEqual(filesFromDataTransfer(null), [])
  assert.deepEqual(
    filesFromDataTransfer({ files: [] } as unknown as DataTransfer),
    [],
  )
})

test('filesFromDataTransfer returns the files as an array', () => {
  const f = new File([new Uint8Array([1])], 'a.png', { type: 'image/png' })
  const files = filesFromDataTransfer({ files: [f] } as unknown as DataTransfer)
  assert.equal(files.length, 1)
  assert.equal(files[0]!.name, 'a.png')
})

test('attachmentNameFor: generic clipboard image names get timestamped', () => {
  const f = new File([new Uint8Array([1])], 'image.png', { type: 'image/png' })
  const name = attachmentNameFor(f, new Date(2026, 6, 2, 9, 5, 7))
  assert.equal(name, 'pasted-image-20260702-090507.png')
})

test('attachmentNameFor: real file names are kept', () => {
  const f = new File([new Uint8Array([1])], 'diagram-final.png', {
    type: 'image/png',
  })
  assert.equal(
    attachmentNameFor(f, new Date(2026, 6, 2, 9, 5, 7)),
    'diagram-final.png',
  )
})

test('attachmentNameFor: unnamed non-image blobs still get a name', () => {
  const f = new File([new Uint8Array([1])], '', { type: 'application/pdf' })
  const name = attachmentNameFor(f, new Date(2026, 6, 2, 9, 5, 7))
  // Unknown MIME falls back to the pasted-image naming with png ext —
  // the sanitizer downstream guarantees a usable file name either way.
  assert.match(name, /^pasted-image-20260702-090507\./)
})
