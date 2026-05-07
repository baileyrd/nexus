// BL-080 unit tests for the file-tree per-extension icon mapping.
// Re-exported via `tests/file-icon.test.ts` so the top-level glob
// picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { getFileIcon } from './fileIcon.ts'

test('markdown extensions map to the book glyph', () => {
  assert.equal(getFileIcon('notes/journal.md'), 'book')
  assert.equal(getFileIcon('README.markdown'), 'book')
  // Case-insensitive — the file system is mixed-case on macOS / Windows.
  assert.equal(getFileIcon('LOUD.MD'), 'book')
})

test('source-code extensions map to the fileCode glyph', () => {
  for (const ext of [
    'rs',
    'ts',
    'tsx',
    'js',
    'jsx',
    'mjs',
    'cjs',
    'py',
    'go',
    'rb',
    'java',
    'kt',
    'swift',
    'cpp',
    'cc',
    'c',
    'h',
    'hpp',
    'cs',
  ]) {
    assert.equal(getFileIcon(`src/main.${ext}`), 'fileCode', `expected fileCode for .${ext}`)
  }
})

test('config / data extensions map to the fileJson glyph', () => {
  for (const ext of ['json', 'jsonc', 'json5', 'toml', 'yaml', 'yml']) {
    assert.equal(getFileIcon(`config.${ext}`), 'fileJson', `expected fileJson for .${ext}`)
  }
})

test('unknown / extensionless / hidden-file shapes fall back to doc', () => {
  assert.equal(getFileIcon('LICENSE'), 'doc')
  assert.equal(getFileIcon('Dockerfile'), 'doc')
  assert.equal(getFileIcon('archive.tar.bin.unknown'), 'doc')
  // Leading-dot file with no real extension is `doc`, not a code file
  // — `.gitignore` / `.npmrc` shouldn't grab the code glyph.
  assert.equal(getFileIcon('.gitignore'), 'doc')
})

test('trailing-dot and empty inputs are safe', () => {
  assert.equal(getFileIcon(''), 'doc')
  assert.equal(getFileIcon('weird.'), 'doc')
})

test('query / hash fragments are stripped before extension parsing', () => {
  // The file tree never feeds these in, but the helper is defensive
  // so the same util can be reused from other surfaces.
  assert.equal(getFileIcon('api.json?ts=123'), 'fileJson')
  assert.equal(getFileIcon('lib.ts#frag'), 'fileCode')
})
