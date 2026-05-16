// BL-142 Phase 2b.1 — unit tests for the display-time ANSI stripper.
//
// ESC is built via `String.fromCharCode(0x1b)` rather than a literal
// `''` so the test file stays purely printable — Edit tools
// and code-review tools both handle it cleanly.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { stripAnsi } from './stripAnsi.ts'

const ESC = String.fromCharCode(0x1b)
const BEL = String.fromCharCode(0x07)

test('stripAnsi passes plain text through unchanged', () => {
  assert.equal(stripAnsi('hello world\n'), 'hello world\n')
})

test('stripAnsi removes a CSI SGR colour sequence', () => {
  // ESC[31m hello ESC[0m
  const colored = `${ESC}[31mhello${ESC}[0m`
  assert.equal(stripAnsi(colored), 'hello')
})

test('stripAnsi removes a CSI cursor-move sequence', () => {
  // ESC[2J ESC[H — clear + home (common Python REPL reset)
  const reset = `${ESC}[2J${ESC}[Hready`
  assert.equal(stripAnsi(reset), 'ready')
})

test('stripAnsi removes an OSC window-title sequence terminated by BEL', () => {
  // ESC] 0 ; title BEL
  const osc = `${ESC}]0;repl session${BEL}after`
  assert.equal(stripAnsi(osc), 'after')
})

test('stripAnsi removes an OSC sequence terminated by ESC \\', () => {
  const osc = `${ESC}]0;title${ESC}\\after`
  assert.equal(stripAnsi(osc), 'after')
})

test('stripAnsi drops two-byte char-set escapes', () => {
  // ESC ( B selects ASCII; ESC ) 0 selects DEC line drawing
  const seq = `${ESC}(B${ESC})0visible`
  assert.equal(stripAnsi(seq), 'visible')
})

test('stripAnsi passes through unknown ESC sequences verbatim', () => {
  // ESC ? — not a recognized form. Leaving it visible is safer
  // than guessing.
  const seq = `${ESC}?ok`
  assert.equal(stripAnsi(seq), `${ESC}?ok`)
})

test('stripAnsi drops a trailing lone ESC', () => {
  assert.equal(stripAnsi(`good${ESC}`), 'good')
})

test('stripAnsi preserves newlines, tabs, and other control chars', () => {
  assert.equal(stripAnsi('line 1\nline 2\tcol\n'), 'line 1\nline 2\tcol\n')
})

test('stripAnsi handles a realistic python NameError', () => {
  // Approximation of `python3 -i` error rendering with color.
  const py = `${ESC}[31mTraceback (most recent call last):\n  File "<stdin>", line 1, in <module>\nNameError: name 'x' is not defined${ESC}[0m\n>>> `
  assert.equal(
    stripAnsi(py),
    'Traceback (most recent call last):\n  File "<stdin>", line 1, in <module>\nNameError: name \'x\' is not defined\n>>> ',
  )
})
