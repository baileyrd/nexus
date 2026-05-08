// shell/src/plugins/nexus/editor/cm/lspClient.test.ts
//
// BL-077 — unit tests for the pure helpers in `lspClient.ts` plus a
// few smoke checks that the extension actually drives `LspIpc`.
//
// Mirrors `gitGutter.test.ts`'s shape: lift the load-bearing logic
// out of the CM6 plumbing (severity/position converters, completion
// item mapper, edits applier, definition picker) and test it against
// fixtures rather than instantiating an `EditorView`. The extension
// itself is exercised in one happy-path mount that asserts the
// IPC `openFile` / `changeFile` / `closeFile` lifecycle calls fire.

import { describe, it } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { diagnosticCount, forEachDiagnostic } from '@codemirror/lint'

import {
  applyTextEdits,
  lspDiagnosticsToCm,
  lspExtension,
  lspItemToCmCompletion,
  lspPositionToOffset,
  pickFirstLocation,
  severityToCm,
  type LspLocation,
} from './lspClient.ts'
import {
  LspIpc,
  type LspChangeFileArgs,
  type LspDiagnostic,
  type LspKernelHandle,
  type LspOpenFileArgs,
  type PublishDiagnosticsParams,
} from './lspIpc.ts'

// ── severityToCm ─────────────────────────────────────────────────────

describe('severityToCm', () => {
  it('maps every LSP severity to the matching CM6 string', () => {
    assert.equal(severityToCm(1), 'error')
    assert.equal(severityToCm(2), 'warning')
    assert.equal(severityToCm(3), 'info')
    assert.equal(severityToCm(4), 'hint')
  })

  it('defaults to error when severity is omitted (matches LSP 3.17 §Diagnostic)', () => {
    assert.equal(severityToCm(undefined), 'error')
  })
})

// ── lspPositionToOffset ──────────────────────────────────────────────

describe('lspPositionToOffset', () => {
  function docOf(text: string) {
    return EditorState.create({ doc: text }).doc
  }

  it('translates 0-indexed (line, character) → absolute offset', () => {
    const doc = docOf('hello\nworld\n!')
    assert.equal(lspPositionToOffset(doc, { line: 0, character: 0 }), 0)
    assert.equal(lspPositionToOffset(doc, { line: 0, character: 5 }), 5) // end of "hello"
    assert.equal(lspPositionToOffset(doc, { line: 1, character: 0 }), 6) // start of "world"
    assert.equal(lspPositionToOffset(doc, { line: 2, character: 1 }), 13) // after "!"
  })

  it('clamps a column past EOL to the line length', () => {
    const doc = docOf('hi\nworld')
    // line 0 is "hi" (length 2); LSP character 99 → clamp to 2.
    assert.equal(lspPositionToOffset(doc, { line: 0, character: 99 }), 2)
  })

  it('clamps a line past EOF to doc.length', () => {
    const doc = docOf('only one line')
    assert.equal(
      lspPositionToOffset(doc, { line: 99, character: 0 }),
      doc.length,
    )
  })
})

// ── lspDiagnosticsToCm ───────────────────────────────────────────────

describe('lspDiagnosticsToCm', () => {
  function viewOf(text: string): EditorView {
    return new EditorView({ state: EditorState.create({ doc: text }) })
  }

  it('translates each LSP diagnostic to a CM6 lint diagnostic', () => {
    const view = viewOf('let x = 1\nlet y = 2\n')
    const diags: LspDiagnostic[] = [
      {
        range: {
          start: { line: 0, character: 4 },
          end: { line: 0, character: 5 },
        },
        severity: 1,
        message: 'unused',
      },
      {
        range: {
          start: { line: 1, character: 0 },
          end: { line: 1, character: 3 },
        },
        severity: 2,
        source: 'rustc',
        message: 'rebind',
      },
    ]
    const cm = lspDiagnosticsToCm(view, diags)
    assert.equal(cm.length, 2)
    assert.deepEqual(cm[0], {
      from: 4,
      to: 5,
      severity: 'error',
      message: 'unused',
    })
    // `source` prepended in square brackets so the diagnostic chip
    // surfaces it inline.
    assert.equal(cm[1].severity, 'warning')
    assert.equal(cm[1].message, '[rustc] rebind')
    view.destroy()
  })
})

// ── lspItemToCmCompletion ────────────────────────────────────────────

describe('lspItemToCmCompletion', () => {
  it('uses insertText when present, falling back to label', () => {
    const c1 = lspItemToCmCompletion({
      label: 'foo',
      insertText: 'foo()',
    })
    assert.equal(c1.label, 'foo')
    assert.equal(c1.apply, 'foo()')

    const c2 = lspItemToCmCompletion({ label: 'bar' })
    assert.equal(c2.apply, 'bar')
  })

  it('maps LSP CompletionItemKind to CM6 type chips', () => {
    // Method = 2
    assert.equal(lspItemToCmCompletion({ label: 'x', kind: 2 }).type, 'method')
    // Variable = 6
    assert.equal(
      lspItemToCmCompletion({ label: 'x', kind: 6 }).type,
      'variable',
    )
    // Keyword = 14
    assert.equal(
      lspItemToCmCompletion({ label: 'x', kind: 14 }).type,
      'keyword',
    )
    // Unknown kind → undefined (no type chip)
    assert.equal(
      lspItemToCmCompletion({ label: 'x', kind: 999 }).type,
      undefined,
    )
  })

  it('extracts documentation from both string and {value} forms', () => {
    assert.equal(
      lspItemToCmCompletion({ label: 'x', documentation: 'plain' }).info,
      'plain',
    )
    assert.equal(
      lspItemToCmCompletion({
        label: 'x',
        documentation: { kind: 'markdown', value: '**bold**' },
      } as Parameters<typeof lspItemToCmCompletion>[0]).info,
      '**bold**',
    )
  })
})

// ── pickFirstLocation ────────────────────────────────────────────────

describe('pickFirstLocation', () => {
  const loc: LspLocation = {
    uri: 'file:///x.rs',
    range: {
      start: { line: 1, character: 0 },
      end: { line: 1, character: 5 },
    },
  }

  it('returns null for null / empty replies', () => {
    assert.equal(pickFirstLocation(null), null)
    assert.equal(pickFirstLocation([]), null)
  })

  it('returns the bare location when reply is a single object', () => {
    assert.deepEqual(pickFirstLocation(loc), loc)
  })

  it('returns the first when reply is an array', () => {
    const second: LspLocation = {
      uri: 'file:///y.rs',
      range: {
        start: { line: 0, character: 0 },
        end: { line: 0, character: 0 },
      },
    }
    assert.deepEqual(pickFirstLocation([loc, second]), loc)
  })
})

// ── applyTextEdits ───────────────────────────────────────────────────

describe('applyTextEdits', () => {
  it('applies edits in bottom-up order so positions don\'t shift', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: 'AAA BBB CCC' }),
    })
    // Replace AAA with X and CCC with Z. If edits ran top-down naively
    // CCC's range would be wrong after AAA → X.
    applyTextEdits(view, [
      {
        range: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 3 },
        },
        newText: 'X',
      },
      {
        range: {
          start: { line: 0, character: 8 },
          end: { line: 0, character: 11 },
        },
        newText: 'Z',
      },
    ])
    assert.equal(view.state.doc.toString(), 'X BBB Z')
    view.destroy()
  })

  it('is a no-op for an empty edit list', () => {
    const view = new EditorView({
      state: EditorState.create({ doc: 'unchanged' }),
    })
    applyTextEdits(view, [])
    assert.equal(view.state.doc.toString(), 'unchanged')
    view.destroy()
  })
})

// ── lspExtension lifecycle smoke test ────────────────────────────────

describe('lspExtension', () => {
  function makeFakeKernel(): {
    handle: LspKernelHandle
    calls: Array<{ command: string; args: unknown }>
    pushDiagnostics: (params: PublishDiagnosticsParams) => boolean
  } {
    const calls: Array<{ command: string; args: unknown }> = []
    let diagnosticsHandler:
      | ((topic: string, payload: PublishDiagnosticsParams) => void)
      | null = null
    const handle: LspKernelHandle = {
      async invoke<T = unknown>(
        _pluginId: string,
        commandId: string,
        args?: unknown,
      ): Promise<T> {
        calls.push({ command: commandId, args })
        if (commandId === 'open_file') {
          // Simulate a server matched the path so the lifecycle
          // path is exercised.
          return {
            uri: (args as LspOpenFileArgs).path,
            server: 'fake',
          } as unknown as T
        }
        return null as unknown as T
      },
      async on<T = unknown>(
        _topicPrefix: string,
        handler: (topic: string, payload: T) => void,
      ): Promise<() => void> {
        diagnosticsHandler = handler as unknown as (
          topic: string,
          payload: PublishDiagnosticsParams,
        ) => void
        return () => {
          diagnosticsHandler = null
        }
      },
    }
    return {
      handle,
      calls,
      pushDiagnostics: (params: PublishDiagnosticsParams): boolean => {
        if (!diagnosticsHandler) return false
        diagnosticsHandler(
          'com.nexus.lsp.textDocument.publishDiagnostics',
          params,
        )
        return true
      },
    }
  }

  it('fires open_file on mount, change_file on edit, close_file on destroy', async () => {
    const { handle, calls } = makeFakeKernel()
    const ipc = new LspIpc(handle)
    const view = new EditorView({
      state: EditorState.create({
        doc: 'fn main() {}',
        extensions: [
          lspExtension({ relpath: 'src/lib.rs', ipc }),
        ],
      }),
    })

    // Let the openFile microtask settle.
    await new Promise((r) => setTimeout(r, 0))
    const open = calls.find((c) => c.command === 'open_file')
    assert.ok(open, 'open_file was called')
    assert.equal((open.args as LspOpenFileArgs).path, 'src/lib.rs')
    assert.equal((open.args as LspOpenFileArgs).content, 'fn main() {}')

    // Edit the doc — must fire change_file with version 2 (initial 1
    // bumps to 2 on first change).
    view.dispatch({
      changes: { from: 0, to: 0, insert: '// hi\n' },
    })
    await new Promise((r) => setTimeout(r, 0))
    const changes = calls.filter((c) => c.command === 'change_file')
    assert.equal(changes.length, 1)
    assert.equal((changes[0].args as LspChangeFileArgs).version, 2)
    assert.equal(
      (changes[0].args as LspChangeFileArgs).content,
      '// hi\nfn main() {}',
    )

    // Destroy → close_file
    view.destroy()
    await new Promise((r) => setTimeout(r, 0))
    const close = calls.find((c) => c.command === 'close_file')
    assert.ok(close, 'close_file was called')
  })

  it('projects matching-URI publishDiagnostics events into the lint state', async () => {
    const { handle, pushDiagnostics } = makeFakeKernel()
    const ipc = new LspIpc(handle)
    const parent = document.createElement('div')
    document.body.appendChild(parent)
    const view = new EditorView({
      state: EditorState.create({
        doc: 'let x = 1\n',
        extensions: [lspExtension({ relpath: 'src/lib.rs', ipc })],
      }),
      parent,
    })
    // Let the subscribe handshake settle so the handler is wired.
    // Need TWO microtask drains because the openFile and onDiagnostics
    // chains are independent — a single setTimeout(0) can leave the
    // diagnostics .then() unresolved.
    await new Promise((r) => setTimeout(r, 0))
    await new Promise((r) => setTimeout(r, 0))

    const fired = pushDiagnostics({
      uri: 'src/lib.rs',
      diagnostics: [
        {
          range: {
            start: { line: 0, character: 4 },
            end: { line: 0, character: 5 },
          },
          severity: 2,
          message: 'unused',
        },
      ],
    })
    assert.equal(fired, true, 'diagnosticsHandler must be wired by mount')
    // Allow the dispatch's reconfigure (which adds lintExtensions
    // when missing) to settle.
    await new Promise((r) => setTimeout(r, 0))
    // setDiagnostics dispatches synchronously inside the handler.
    // We just verify the dispatch reached the view by looking at
    // the lint state's pretty-print — the lint state field isn't
    // public, but `forEachDiagnostic` from `@codemirror/lint`
    // exposes it. Pull dynamically to avoid a static import that
    // bloats the test surface.
    const collected: string[] = []
    forEachDiagnostic(view.state, (d) => {
      collected.push(`${d.severity}:${d.message}`)
    })
    assert.deepEqual(collected, ['warning:unused'])
    view.destroy()
  })

  it('ignores diagnostics for other URIs', async () => {
    const { handle, pushDiagnostics } = makeFakeKernel()
    const ipc = new LspIpc(handle)
    const view = new EditorView({
      state: EditorState.create({
        doc: 'x',
        extensions: [lspExtension({ relpath: 'src/lib.rs', ipc })],
      }),
    })
    await new Promise((r) => setTimeout(r, 0))
    pushDiagnostics({
      uri: 'src/other.rs',
      diagnostics: [
        {
          range: {
            start: { line: 0, character: 0 },
            end: { line: 0, character: 1 },
          },
          severity: 1,
          message: 'should be ignored',
        },
      ],
    })
    const collected: string[] = []
    forEachDiagnostic(view.state, (d) => collected.push(d.message))
    assert.deepEqual(collected, [])
    view.destroy()
  })
})

