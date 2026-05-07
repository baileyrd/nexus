// shell/src/plugins/nexus/editor/codeMode.test.ts
//
// BL-075 — pure-function tests for the dual-mode router. Each test
// pins a single behavioural invariant; the matrix is deliberately
// flat so a future contributor can grok the full contract from one
// scroll.

import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import {
  DEFAULT_CODE_EXTENSIONS,
  getEditorMode,
  getExtension,
  pickLanguageExtension,
} from './codeMode.ts'

describe('getExtension', () => {
  it('returns the lowercased final segment after the last dot', () => {
    assert.equal(getExtension('main.rs'), 'rs')
    assert.equal(getExtension('App.TSX'), 'tsx')
    assert.equal(getExtension('a.b.c.json'), 'json')
  })

  it('returns empty string for names with no extension', () => {
    assert.equal(getExtension('LICENSE'), '')
    assert.equal(getExtension('Makefile'), '')
    assert.equal(getExtension(''), '')
  })

  it('handles trailing-dot names by returning empty string', () => {
    // `name.` has a dot at the end; nothing after it counts as an
    // extension, so this is structurally a "no extension" case.
    assert.equal(getExtension('weird.'), '')
  })

  it('trims whitespace before extracting', () => {
    assert.equal(getExtension('  main.rs  '), 'rs')
  })
})

describe('getEditorMode', () => {
  it('routes markdown to document mode regardless of override list', () => {
    assert.equal(getEditorMode('notes.md'), 'document')
    assert.equal(getEditorMode('notes.markdown'), 'document')
    assert.equal(getEditorMode('notes.mdx'), 'document')
    // Even if a misconfigured override included markdown, it must
    // still route to document — the editor's block-tree pipeline
    // depends on the assumption.
    assert.equal(
      getEditorMode('notes.md', ['md', 'rs']),
      'document',
    )
  })

  it('routes every default code extension to code mode', () => {
    for (const ext of DEFAULT_CODE_EXTENSIONS) {
      assert.equal(
        getEditorMode(`file.${ext}`),
        'code',
        `expected ${ext} → code`,
      )
    }
  })

  it('routes unknown extensions to document mode', () => {
    // Plain-text files keep their pre-BL-075 behaviour: opening
    // `LICENSE` shouldn't suddenly fall into a code-mode CM6 with no
    // language.
    assert.equal(getEditorMode('LICENSE'), 'document')
    assert.equal(getEditorMode('CHANGELOG'), 'document')
    assert.equal(getEditorMode('script.sh'), 'document')
  })

  it('respects a caller-supplied override list', () => {
    // User added `.sh` to the override; now it counts as code.
    assert.equal(getEditorMode('script.sh', ['sh']), 'code')
    // And dropping `rs` excludes Rust from the code set.
    assert.equal(getEditorMode('main.rs', ['py']), 'document')
  })
})

describe('pickLanguageExtension', () => {
  it('returns a non-null extension for every default-routed name', () => {
    for (const ext of DEFAULT_CODE_EXTENSIONS) {
      const result = pickLanguageExtension(`file.${ext}`)
      assert.ok(result !== null, `expected extension for ${ext}`)
    }
  })

  it('returns null for unmapped extensions', () => {
    assert.equal(pickLanguageExtension('script.sh'), null)
    assert.equal(pickLanguageExtension('notes.md'), null)
    assert.equal(pickLanguageExtension('LICENSE'), null)
  })

  it('handles tsx and jsx through the javascript package', () => {
    // Smoke test: `pickLanguageExtension` doesn't throw when asked
    // for the JSX/TSX flavour. The returned extension's internals
    // are CM6's concern; we only assert "we got something back".
    assert.ok(pickLanguageExtension('App.tsx') !== null)
    assert.ok(pickLanguageExtension('App.jsx') !== null)
  })
})
