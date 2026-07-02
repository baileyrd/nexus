// C1 (#354) — unit tests for the image/attachment pipeline helpers.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/editor-attachments.test.ts` re-export
// shim).

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  attachmentDirForMode,
  attachmentMarkdown,
  bytesToDataUrl,
  clearForgeImageCache,
  isExternalSrc,
  isImagePath,
  loadForgeImage,
  normalizeRelpath,
  pastedImageName,
  resolveImageCandidates,
  sanitizeAttachmentName,
  writeAttachment,
} from './attachments.ts'
import type { KernelAPI } from '../../../types/plugin.ts'

// ── path + src helpers ────────────────────────────────────────────────

test('normalizeRelpath collapses . and .. segments', () => {
  assert.equal(normalizeRelpath('a/b/../c'), 'a/c')
  assert.equal(normalizeRelpath('./x/./y'), 'x/y')
  assert.equal(normalizeRelpath('a//b'), 'a/b')
})

test('normalizeRelpath rejects escapes above the forge root', () => {
  assert.equal(normalizeRelpath('../x'), null)
  assert.equal(normalizeRelpath('a/../../x'), null)
})

test('isExternalSrc leaves absolute / data / fragment srcs alone', () => {
  assert.equal(isExternalSrc('https://example.com/a.png'), true)
  assert.equal(isExternalSrc('data:image/png;base64,AAAA'), true)
  assert.equal(isExternalSrc('//cdn.example.com/a.png'), true)
  assert.equal(isExternalSrc('#fragment'), true)
  assert.equal(isExternalSrc('img.png'), false)
  assert.equal(isExternalSrc('sub/dir/img.png'), false)
})

test('resolveImageCandidates probes note-dir then forge root', () => {
  assert.deepEqual(resolveImageCandidates('notes/daily/today.md', 'img.png'), [
    'notes/daily/img.png',
    'img.png',
  ])
})

test('resolveImageCandidates decodes URI escapes and strips queries', () => {
  assert.deepEqual(
    resolveImageCandidates('notes/a.md', 'My%20Shot.png?v=2'),
    ['notes/My Shot.png', 'My Shot.png'],
  )
})

test('resolveImageCandidates resolves ../ against the note dir', () => {
  // The root-relative probe would escape the forge → only one candidate.
  assert.deepEqual(
    resolveImageCandidates('notes/daily/today.md', '../assets/x.png'),
    ['notes/assets/x.png'],
  )
})

test('resolveImageCandidates returns nothing for external srcs', () => {
  assert.deepEqual(
    resolveImageCandidates('a.md', 'https://example.com/x.png'),
    [],
  )
})

test('isImagePath keys off the extension, case-insensitively', () => {
  assert.equal(isImagePath('shot.PNG'), true)
  assert.equal(isImagePath('doc.pdf'), false)
})

test('bytesToDataUrl base64-encodes with the given MIME', () => {
  assert.equal(
    bytesToDataUrl(new Uint8Array([72, 105]), 'text/plain'),
    'data:text/plain;base64,SGk=',
  )
})

// ── attachment placement ──────────────────────────────────────────────

test('attachmentDirForMode: root → forge root', () => {
  assert.equal(attachmentDirForMode('root', 'notes/a.md', 'attachments'), '')
})

test('attachmentDirForMode: same → the note dir (root note → root)', () => {
  assert.equal(
    attachmentDirForMode('same', 'notes/daily/a.md', 'attachments'),
    'notes/daily',
  )
  assert.equal(attachmentDirForMode('same', 'a.md', 'attachments'), '')
})

test('attachmentDirForMode: specific → configured folder, cleaned', () => {
  assert.equal(attachmentDirForMode('specific', 'a.md', '/media/'), 'media')
  assert.equal(attachmentDirForMode('specific', 'a.md', '   '), 'attachments')
})

test('sanitizeAttachmentName strips separators and control chars', () => {
  assert.equal(sanitizeAttachmentName('../../evil.png'), '..-..-evil.png')
  assert.equal(sanitizeAttachmentName('a\u0000b.png'), 'ab.png')
  assert.equal(sanitizeAttachmentName('  '), 'file')
  assert.equal(sanitizeAttachmentName('nice name.png'), 'nice name.png')
})

test('pastedImageName is timestamped with the MIME extension', () => {
  const name = pastedImageName('image/png', new Date(2026, 6, 2, 9, 5, 7))
  assert.equal(name, 'pasted-image-20260702-090507.png')
})

test('attachmentMarkdown emits image embeds for images, links otherwise', () => {
  assert.equal(
    attachmentMarkdown('attachments/My Shot.png'),
    '![](attachments/My%20Shot.png)',
  )
  assert.equal(
    attachmentMarkdown('docs/report.pdf'),
    '[report.pdf](docs/report.pdf)',
  )
})

// ── kernel-backed helpers (fake kernel) ───────────────────────────────

interface Call {
  plugin: string
  cmd: string
  args: Record<string, unknown>
}

function fakeKernel(
  files: Map<string, number[]>,
  calls: Call[] = [],
): KernelAPI {
  return {
    invoke: async <T>(
      plugin: string,
      cmd: string,
      args?: unknown,
    ): Promise<T> => {
      const a = (args ?? {}) as Record<string, unknown>
      calls.push({ plugin, cmd, args: a })
      const path = String(a.path ?? '')
      if (cmd === 'file_exists') return { exists: files.has(path) } as T
      if (cmd === 'read_file') return { bytes: files.get(path) ?? null } as T
      if (cmd === 'write_file') {
        files.set(path, Array.from(a.bytes as number[]))
        return { path } as T
      }
      throw new Error(`unexpected IPC: ${plugin}::${cmd}`)
    },
    on: () => () => {},
  } as unknown as KernelAPI
}

test('writeAttachment dedups colliding names via file_exists', async () => {
  const files = new Map<string, number[]>([
    ['media/shot.png', [1]],
    ['media/shot-1.png', [2]],
  ])
  const relpath = await writeAttachment(
    fakeKernel(files),
    'media',
    'shot.png',
    new Uint8Array([9]),
  )
  assert.equal(relpath, 'media/shot-2.png')
  assert.deepEqual(files.get('media/shot-2.png'), [9])
})

test('writeAttachment writes to the forge root when dir is empty', async () => {
  const files = new Map<string, number[]>()
  const relpath = await writeAttachment(
    fakeKernel(files),
    '',
    'shot.png',
    new Uint8Array([7]),
  )
  assert.equal(relpath, 'shot.png')
})

test('loadForgeImage returns the first candidate that exists, cached', async () => {
  clearForgeImageCache()
  const files = new Map<string, number[]>([['notes/img.png', [72, 105]]])
  const calls: Call[] = []
  const kernel = fakeKernel(files, calls)
  const url = await loadForgeImage(kernel, ['notes/img.png', 'img.png'])
  assert.equal(url, 'data:image/png;base64,SGk=')
  // Second load resolves from the module cache — no extra IPC.
  const before = calls.length
  const again = await loadForgeImage(kernel, ['notes/img.png'])
  assert.equal(again, url)
  assert.equal(calls.length, before)
  clearForgeImageCache()
})

test('loadForgeImage returns null when no candidate resolves', async () => {
  clearForgeImageCache()
  const url = await loadForgeImage(fakeKernel(new Map()), ['missing.png'])
  assert.equal(url, null)
  clearForgeImageCache()
})
