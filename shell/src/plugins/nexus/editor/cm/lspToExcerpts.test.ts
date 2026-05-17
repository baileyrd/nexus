// BL-141 Phase 3 — unit tests for the LSP → ExcerptRequest
// converters. Pure functions, no IPC, no CM6 state.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  diagnosticLabel,
  diagnosticsToExcerptRequests,
  locationsToExcerptRequests,
  rangeToExcerptLines,
  severityTag,
  uriToRelpath,
  workspaceEditToExcerptRequests,
  type LspLocation,
  type LspWorkspaceEditChanges,
} from './lspToExcerpts.ts'
import type { LspDiagnostic } from './lspIpc.ts'

const FORGE = '/srv/forge'

// ── uriToRelpath ──────────────────────────────────────────────────────────────

test('uriToRelpath strips file:// prefix and forge-root prefix', () => {
  assert.equal(
    uriToRelpath('file:///srv/forge/src/lib.md', FORGE),
    'src/lib.md',
  )
})

test('uriToRelpath returns null for URIs outside the forge', () => {
  assert.equal(uriToRelpath('file:///etc/passwd', FORGE), null)
  assert.equal(uriToRelpath('file:///srv/forge2/other.md', FORGE), null)
})

test('uriToRelpath returns null for non-file:// schemes', () => {
  assert.equal(uriToRelpath('http://example.com/x', FORGE), null)
  assert.equal(uriToRelpath('untitled:Untitled-1', FORGE), null)
})

test('uriToRelpath tolerates a trailing slash on forge root', () => {
  assert.equal(
    uriToRelpath('file:///srv/forge/x.md', '/srv/forge/'),
    'x.md',
  )
})

// ── rangeToExcerptLines ──────────────────────────────────────────────────────

test('rangeToExcerptLines converts 0-based to 1-based + adds context', () => {
  // line=4 → 5 1-based; ±3 context → [2, 8]
  const got = rangeToExcerptLines(
    { start: { line: 4, character: 0 }, end: { line: 4, character: 10 } },
    3,
  )
  assert.deepEqual(got, { line_start: 2, line_end: 8 })
})

test('rangeToExcerptLines clamps line_start at 1', () => {
  // line=0 → 1; -3 context would be -2 but we clamp
  const got = rangeToExcerptLines(
    { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
    3,
  )
  assert.equal(got.line_start, 1)
  assert.equal(got.line_end, 4)
})

test('rangeToExcerptLines preserves multi-line ranges', () => {
  const got = rangeToExcerptLines(
    { start: { line: 10, character: 0 }, end: { line: 15, character: 0 } },
    2,
  )
  assert.deepEqual(got, { line_start: 9, line_end: 18 })
})

test('rangeToExcerptLines with zero context is a tight wrap', () => {
  const got = rangeToExcerptLines(
    { start: { line: 4, character: 0 }, end: { line: 4, character: 10 } },
    0,
  )
  assert.deepEqual(got, { line_start: 5, line_end: 5 })
})

// ── locationsToExcerptRequests ────────────────────────────────────────────────

test('locationsToExcerptRequests converts a single in-forge location', () => {
  const locs: LspLocation[] = [
    {
      uri: 'file:///srv/forge/src/lib.md',
      range: {
        start: { line: 9, character: 4 },
        end: { line: 9, character: 12 },
      },
    },
  ]
  const got = locationsToExcerptRequests(locs, {
    forgeRoot: FORGE,
    contextLines: 1,
  })
  assert.equal(got.length, 1)
  assert.equal(got[0].relpath, 'src/lib.md')
  assert.equal(got[0].line_start, 9)
  assert.equal(got[0].line_end, 11)
  assert.equal(got[0].label, 'L10')
})

test('locationsToExcerptRequests preserves response order', () => {
  const locs: LspLocation[] = [
    {
      uri: 'file:///srv/forge/a.md',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
    },
    {
      uri: 'file:///srv/forge/b.md',
      range: { start: { line: 5, character: 0 }, end: { line: 5, character: 1 } },
    },
  ]
  const got = locationsToExcerptRequests(locs, {
    forgeRoot: FORGE,
    contextLines: 0,
  })
  assert.deepEqual(got.map((r) => r.relpath), ['a.md', 'b.md'])
})

test('locationsToExcerptRequests skips URIs outside the forge', () => {
  const locs: LspLocation[] = [
    {
      uri: 'file:///srv/forge/in.md',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
    },
    {
      uri: 'file:///elsewhere/out.md',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
    },
  ]
  const got = locationsToExcerptRequests(locs, { forgeRoot: FORGE })
  assert.equal(got.length, 1)
  assert.equal(got[0].relpath, 'in.md')
})

test('locationsToExcerptRequests builds multi-line label when range spans lines', () => {
  const locs: LspLocation[] = [
    {
      uri: 'file:///srv/forge/x.md',
      range: { start: { line: 9, character: 0 }, end: { line: 12, character: 0 } },
    },
  ]
  const got = locationsToExcerptRequests(locs, {
    forgeRoot: FORGE,
    contextLines: 0,
  })
  assert.equal(got[0].label, 'L10-L13')
})

// ── workspaceEditToExcerptRequests ────────────────────────────────────────────

test('workspaceEditToExcerptRequests flattens per-file edits into excerpts', () => {
  const edit: LspWorkspaceEditChanges = {
    changes: {
      'file:///srv/forge/a.md': [
        {
          range: {
            start: { line: 4, character: 0 },
            end: { line: 4, character: 3 },
          },
          newText: 'fooBar',
        },
        {
          range: {
            start: { line: 9, character: 0 },
            end: { line: 9, character: 3 },
          },
          newText: 'fooBar',
        },
      ],
      'file:///srv/forge/b.md': [
        {
          range: {
            start: { line: 0, character: 0 },
            end: { line: 0, character: 3 },
          },
          newText: 'fooBar',
        },
      ],
    },
  }
  const got = workspaceEditToExcerptRequests(edit, {
    forgeRoot: FORGE,
    contextLines: 0,
  })
  assert.equal(got.length, 3)
  // Per-edit label embeds the new text.
  assert.match(got[0].label ?? '', /→ "fooBar"/)
})

test('workspaceEditToExcerptRequests returns empty for missing changes', () => {
  assert.deepEqual(
    workspaceEditToExcerptRequests({} as LspWorkspaceEditChanges, {
      forgeRoot: FORGE,
    }),
    [],
  )
})

test('workspaceEditToExcerptRequests skips out-of-forge files', () => {
  const edit: LspWorkspaceEditChanges = {
    changes: {
      'file:///elsewhere/x.md': [
        {
          range: {
            start: { line: 0, character: 0 },
            end: { line: 0, character: 1 },
          },
          newText: 'y',
        },
      ],
    },
  }
  const got = workspaceEditToExcerptRequests(edit, { forgeRoot: FORGE })
  assert.equal(got.length, 0)
})

// ── severityTag ───────────────────────────────────────────────────────────────

test('severityTag maps LSP severity codes', () => {
  assert.equal(severityTag(1), 'error')
  assert.equal(severityTag(2), 'warn')
  assert.equal(severityTag(3), 'info')
  assert.equal(severityTag(4), 'hint')
})

test('severityTag defaults to error for missing / unknown severity', () => {
  assert.equal(severityTag(undefined), 'error')
  assert.equal(severityTag(99 as 1), 'error')
})

// ── diagnosticLabel ──────────────────────────────────────────────────────────

test('diagnosticLabel renders single-line range + severity tag + message', () => {
  const d: LspDiagnostic = {
    range: { start: { line: 41, character: 0 }, end: { line: 41, character: 5 } },
    severity: 1,
    message: 'unresolved identifier foo',
  }
  assert.equal(diagnosticLabel(d), 'L42 error: unresolved identifier foo')
})

test('diagnosticLabel uses multi-line head for spanning ranges', () => {
  const d: LspDiagnostic = {
    range: { start: { line: 9, character: 0 }, end: { line: 11, character: 0 } },
    severity: 2,
    message: 'unused import',
  }
  assert.equal(diagnosticLabel(d), 'L10-L12 warn: unused import')
})

test('diagnosticLabel collapses internal whitespace + truncates long messages', () => {
  const long = 'a'.repeat(120)
  const d: LspDiagnostic = {
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
    severity: 3,
    message: `line one\n  line\ttwo  ${long}`,
  }
  const label = diagnosticLabel(d)
  // Head + tag prefix.
  assert.ok(label.startsWith('L1 info: '))
  // No raw newlines / tabs survived.
  assert.ok(!/[\n\t]/.test(label))
  // Truncation marker present.
  assert.ok(label.endsWith('…'))
})

test('diagnosticLabel omits the colon when the message is empty', () => {
  const d: LspDiagnostic = {
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    severity: 4,
    message: '',
  }
  assert.equal(diagnosticLabel(d), 'L1 hint')
})

// ── diagnosticsToExcerptRequests ─────────────────────────────────────────────

test('diagnosticsToExcerptRequests fans diagnostics out per file', () => {
  const map: Record<string, LspDiagnostic[]> = {
    'file:///srv/forge/a.md': [
      {
        range: { start: { line: 4, character: 0 }, end: { line: 4, character: 3 } },
        severity: 1,
        message: 'first',
      },
      {
        range: { start: { line: 9, character: 0 }, end: { line: 9, character: 3 } },
        severity: 2,
        message: 'second',
      },
    ],
    'file:///srv/forge/b.md': [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        severity: 3,
        message: 'third',
      },
    ],
  }
  const got = diagnosticsToExcerptRequests(map, {
    forgeRoot: FORGE,
    contextLines: 0,
  })
  assert.equal(got.length, 3)
  assert.deepEqual(got.map((r) => r.relpath), ['a.md', 'a.md', 'b.md'])
  assert.equal(got[0].label, 'L5 error: first')
  assert.equal(got[1].label, 'L10 warn: second')
  assert.equal(got[2].label, 'L1 info: third')
})

test('diagnosticsToExcerptRequests drops out-of-forge URIs and non-array entries', () => {
  const map: Record<string, LspDiagnostic[]> = {
    'file:///srv/forge/keep.md': [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        message: 'kept',
      },
    ],
    'file:///elsewhere/drop.md': [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        message: 'dropped',
      },
    ],
    'untitled:nope': [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        message: 'also dropped',
      },
    ],
  }
  const got = diagnosticsToExcerptRequests(map, { forgeRoot: FORGE })
  assert.equal(got.length, 1)
  assert.equal(got[0].relpath, 'keep.md')
})

test('diagnosticsToExcerptRequests returns empty for empty input', () => {
  assert.deepEqual(
    diagnosticsToExcerptRequests({}, { forgeRoot: FORGE }),
    [],
  )
})

test('diagnosticsToExcerptRequests applies contextLines expansion', () => {
  const map: Record<string, LspDiagnostic[]> = {
    'file:///srv/forge/x.md': [
      {
        range: { start: { line: 9, character: 0 }, end: { line: 9, character: 5 } },
        message: 'x',
      },
    ],
  }
  const got = diagnosticsToExcerptRequests(map, {
    forgeRoot: FORGE,
    contextLines: 2,
  })
  assert.equal(got[0].line_start, 8)
  assert.equal(got[0].line_end, 12)
})

test('diagnosticsToExcerptRequests skips diagnostics with no range', () => {
  const map: Record<string, LspDiagnostic[]> = {
    'file:///srv/forge/x.md': [
      // Defensive: a malformed payload missing `range` shouldn't crash.
      { message: 'no range here' } as unknown as LspDiagnostic,
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        message: 'has range',
      },
    ],
  }
  const got = diagnosticsToExcerptRequests(map, { forgeRoot: FORGE })
  assert.equal(got.length, 1)
  assert.equal(got[0].label, 'L1 error: has range')
})
