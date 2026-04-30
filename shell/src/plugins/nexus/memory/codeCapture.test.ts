// Pure-logic tests for the BL-046 code-aware capture helpers.
// Re-exported via `shell/tests/code-capture.test.ts` so the
// default `pnpm test` glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  buildCodeSnippetSection,
  detectCodeLanguage,
} from './codeCapture.ts'
import { buildSnippet } from './captureStore.ts'

// ── detectCodeLanguage ──────────────────────────────────────────────────────

test('detectCodeLanguage: maps common extensions to fence info-strings', () => {
  assert.equal(detectCodeLanguage('crates/foo/src/lib.rs'), 'rust')
  assert.equal(detectCodeLanguage('shell/src/plugin.ts'), 'typescript')
  assert.equal(detectCodeLanguage('app/main.tsx'), 'tsx')
  assert.equal(detectCodeLanguage('scripts/run.py'), 'python')
  assert.equal(detectCodeLanguage('go-svc/main.go'), 'go')
  assert.equal(detectCodeLanguage('config/values.yaml'), 'yaml')
  assert.equal(detectCodeLanguage('infra/main.tf'), null) // not in the table
})

test('detectCodeLanguage: extension match is case-insensitive', () => {
  assert.equal(detectCodeLanguage('FOO.RS'), 'rust')
  assert.equal(detectCodeLanguage('Component.Tsx'), 'tsx')
})

test('detectCodeLanguage: strips IDE-style query/fragment suffixes', () => {
  assert.equal(detectCodeLanguage('a/b/file.ts?123'), 'typescript')
  assert.equal(detectCodeLanguage('a/b/file.ts#L42'), 'typescript')
})

test('detectCodeLanguage: returns null for unknown / extensionless paths', () => {
  assert.equal(detectCodeLanguage(undefined), null)
  assert.equal(detectCodeLanguage(null), null)
  assert.equal(detectCodeLanguage(''), null)
  assert.equal(detectCodeLanguage('Makefile'), null)
  assert.equal(detectCodeLanguage('.gitignore'), null) // dot is at index 0
  assert.equal(detectCodeLanguage('weird.unknownext'), null)
})

// ── buildCodeSnippetSection ─────────────────────────────────────────────────

test('buildCodeSnippetSection: emits File/Lines headers, fence, and #code tag', () => {
  const lines = buildCodeSnippetSection('let x = 1;\nlet y = 2;\n', {
    file: 'src/main.rs',
    language: 'rust',
    lineRange: { start: 12, end: 13 },
  })
  // Expected order: File line, Lines line, blank, fence open,
  // body, fence close, blank, #code tag.
  assert.deepEqual(lines, [
    'File: src/main.rs',
    'Lines: L12-L13',
    '',
    '```rust',
    'let x = 1;\nlet y = 2;',
    '```',
    '',
    '#code/rust',
  ])
})

test('buildCodeSnippetSection: omits Lines line when range is absent', () => {
  const lines = buildCodeSnippetSection('print(1)', {
    file: 'a.py',
    language: 'python',
  })
  assert.equal(lines.includes('Lines: L1-L1'), false)
  assert.ok(lines.includes('File: a.py'))
})

test('buildCodeSnippetSection: escapes triple-backtick body with a 4-tick fence', () => {
  const body = 'before\n```\nfenced inside\n```\nafter'
  const lines = buildCodeSnippetSection(body, {
    file: 'a.md',
    language: 'markdown',
  })
  assert.equal(lines.find((l) => l.startsWith('```')), '````markdown')
  assert.equal(lines[lines.length - 3], '````')
})

// ── buildSnippet integration ────────────────────────────────────────────────

test('buildSnippet without code metadata produces the BL-043 plain form', () => {
  const out = buildSnippet('hello world', {
    app: 'Browser',
    capturedAt: '2026-04-30T10:15:00Z',
  })
  // Trailing newline + newline-joined; `.includes` is enough to
  // pin the contract without taking a strict-equal dependency on
  // every blank line.
  assert.match(out, /^## Captured at 2026-04-30T10:15:00Z/)
  assert.match(out, /Source: Browser/)
  assert.match(out, /hello world\n$/)
  assert.equal(out.includes('```'), false)
  assert.equal(out.includes('#code/'), false)
})

test('buildSnippet with code metadata produces a fenced block + #code tag', () => {
  const out = buildSnippet('let x = 1', {
    app: 'VS Code',
    capturedAt: '2026-04-30T10:15:00Z',
    code: {
      file: 'src/main.rs',
      language: 'rust',
      lineRange: { start: 5, end: 5 },
    },
  })
  assert.match(out, /Source: VS Code/)
  assert.match(out, /File: src\/main\.rs/)
  assert.match(out, /Lines: L5-L5/)
  assert.match(out, /```rust\nlet x = 1\n```/)
  assert.match(out, /#code\/rust/)
})

test('buildSnippet with code metadata strips trailing blank lines from the fence body', () => {
  const out = buildSnippet('let x = 1\n\n\n', {
    app: 'editor',
    capturedAt: 'now',
    code: { file: 'a.rs', language: 'rust' },
  })
  // The fence body should have exactly one occurrence of `let x = 1`
  // followed directly by a closing fence — no extra blank lines.
  assert.match(out, /```rust\nlet x = 1\n```/)
})
