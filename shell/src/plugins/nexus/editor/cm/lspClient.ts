// BL-077 — CM6 LSP client extension.
//
// Wires a code-mode editor to `com.nexus.lsp` (BL-076). The
// extension is a single bundle returned by [`lspExtension`]; mount
// it once per code-mode tab. The bundle:
//
//   - `didOpen`s on view mount, `didClose`s on unmount.
//   - Forwards every `docChanged` transaction as `didChange` —
//     no debounce, per BL-077 DoD.
//   - Subscribes to `com.nexus.lsp.textDocument.publishDiagnostics`
//     and projects matching-URI batches into CM6 lint diagnostics.
//   - Provides a CM6 autocompletion source backed by `completions`.
//   - Provides a hover tooltip backed by `hover`.
//   - Maps Cmd/Ctrl-Click to `definition`; the resolved `Location`
//     is forwarded to a caller-supplied `onOpenLocation` handler
//     (the tab/router lives outside CM6).
//   - Adds a `Mod-s` keybinding that fetches `format` `TextEdit[]`
//     and applies them as a single transaction.
//
// Document mode tabs do *not* mount this extension; the choice is
// made by the editor at tab-render time per BL-075's `getEditorMode`.
//
// All LSP responses are cast at the use site — the host is a
// transparent JSON proxy and we don't redeclare the LSP spec types
// in TypeScript form. The shapes below are minimal subsets of the
// LSP fields we actually consume.

import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from '@codemirror/autocomplete'
import { linter, setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { Prec } from '@codemirror/state'
import {
  EditorView,
  ViewPlugin,
  hoverTooltip,
  keymap,
  type Tooltip,
} from '@codemirror/view'

import type {
  LspDiagnostic,
  LspIpc,
  LspPosition,
  LspRange,
  PublishDiagnosticsParams,
} from './lspIpc.ts'

/** Shape of an LSP `Location` — minimal subset we forward. */
export interface LspLocation {
  uri: string
  range: LspRange
}

/** LSP `CompletionItem` subset — we cast the IPC reply at use site. */
interface LspCompletionItem {
  label: string
  kind?: number
  detail?: string
  documentation?: string | { value: string }
  insertText?: string
  filterText?: string
  sortText?: string
  textEdit?: { range: LspRange; newText: string }
}

/** LSP `CompletionList` shape (or just `CompletionItem[]`). */
interface LspCompletionList {
  isIncomplete: boolean
  items: LspCompletionItem[]
}

/** LSP `Hover` subset. */
interface LspHover {
  contents:
    | string
    | { kind?: 'plaintext' | 'markdown'; value: string }
    | Array<string | { language?: string; value: string }>
  range?: LspRange
}

/** LSP `TextEdit` (used by `format` and rename apply). */
export interface LspTextEdit {
  range: LspRange
  newText: string
}

/**
 * Convert an LSP `severity` to a CM6 lint severity. The LSP enum is
 * 1=Error, 2=Warning, 3=Info, 4=Hint; CM6 has 'error', 'warning',
 * 'info', 'hint'.
 */
export function severityToCm(
  severity: 1 | 2 | 3 | 4 | undefined,
): Diagnostic['severity'] {
  switch (severity) {
    case 1:
      return 'error'
    case 2:
      return 'warning'
    case 3:
      return 'info'
    case 4:
      return 'hint'
    default:
      // Spec says default is Error per https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#diagnostic
      return 'error'
  }
}

/**
 * Convert a 0-indexed LSP position to a CM6 absolute offset. Returns
 * the document end if the position is past the buffer (a stale
 * server-pushed payload after a delete).
 */
export function lspPositionToOffset(
  doc: { lineAt(line: number): { from: number; to: number; length: number } | null; lines: number; length: number },
  pos: LspPosition,
): number {
  if (pos.line >= doc.lines) return doc.length
  // CM6 lines are 1-indexed; LSP lines are 0-indexed.
  const lineInfo = (doc as unknown as {
    line(n: number): { from: number; length: number }
  }).line(pos.line + 1)
  // Clamp the column so a stale offset past EOL doesn't throw.
  const character = Math.min(pos.character, lineInfo.length)
  return lineInfo.from + character
}

/** Convert LSP diagnostics for the *current* doc into CM6 form. */
export function lspDiagnosticsToCm(
  view: EditorView,
  diags: LspDiagnostic[],
): Diagnostic[] {
  const doc = view.state.doc
  return diags.map((d) => ({
    from: lspPositionToOffset(doc, d.range.start),
    to: lspPositionToOffset(doc, d.range.end),
    severity: severityToCm(d.severity),
    message: d.source ? `[${d.source}] ${d.message}` : d.message,
  }))
}

/** LSP CompletionItemKind → human label for the CM6 chip. */
const COMPLETION_KIND_LABELS: Record<number, Completion['type']> = {
  1: 'text',
  2: 'method',
  3: 'function',
  4: 'function', // constructor
  5: 'property',
  6: 'variable',
  7: 'class',
  8: 'interface',
  9: 'namespace',
  10: 'property',
  11: 'type',
  12: 'constant',
  13: 'enum',
  14: 'keyword',
  15: 'text', // snippet
  16: 'constant', // color
  17: 'text', // file
  18: 'text', // reference
  19: 'namespace', // folder
  20: 'enum',
  21: 'constant',
  22: 'class', // struct
  23: 'keyword', // event
  24: 'function', // operator
  25: 'type',
}

/** Best-effort extract of human text from an LSP `Hover.contents`. */
function hoverText(hover: LspHover | null): string | null {
  if (!hover) return null
  const c = hover.contents
  if (typeof c === 'string') return c
  if (Array.isArray(c)) {
    return c
      .map((item) => (typeof item === 'string' ? item : item.value))
      .filter((s) => s && s.trim().length > 0)
      .join('\n\n')
  }
  return c.value
}

/**
 * Convert an LSP `CompletionItem` to a CM6 `Completion`. The CM6
 * source caller is responsible for picking the `from`/`to` slice the
 * insertion replaces; this just wraps label / kind / docs.
 */
export function lspItemToCmCompletion(item: LspCompletionItem): Completion {
  const insert = item.insertText ?? item.textEdit?.newText ?? item.label
  const completion: Completion = {
    label: item.label,
    apply: insert,
  }
  const kindLabel = item.kind != null ? COMPLETION_KIND_LABELS[item.kind] : undefined
  if (kindLabel != null) completion.type = kindLabel
  if (item.detail != null) completion.detail = item.detail
  const docText =
    typeof item.documentation === 'string'
      ? item.documentation
      : item.documentation?.value
  if (docText != null) completion.info = docText
  return completion
}

/** Options for [`lspExtension`]. */
export interface LspExtensionOptions {
  /** Forge-relative path of the open file. */
  relpath: string
  /** Initial document version — usually `1`. */
  initialVersion?: number
  /** Optional language hint for `didOpen`; the host will infer if absent. */
  languageId?: string
  /** IPC adapter — usually `new LspIpc(runtime.kernel)`. */
  ipc: LspIpc
  /**
   * Caller for go-to-definition. Fired with the resolved location;
   * the editor / router decides how to open the file.
   */
  onOpenLocation?: (loc: LspLocation) => void
  /** Sink for IPC errors. Defaults to `console.warn`. */
  onError?: (where: string, err: unknown) => void
}

/** State held inside the view plugin. */
interface PluginState {
  version: number
  uri: string
  diagnosticsUnsub: (() => void) | null
}

function inferUri(relpath: string): string {
  if (relpath.startsWith('file://')) return relpath
  // The host normalises relpath → absolute via the forge root, but
  // the URI we report to the server is what we send in `didOpen`;
  // the host accepts a bare absolute path or a `file://` URI. We
  // pass through verbatim; the host wraps as needed.
  return relpath
}

/**
 * BL-077 — root LSP client extension. Returns a CM6 `Extension`
 * compatible with [`CodeMirrorHost.buildExtensions`].
 */
export function lspExtension(opts: LspExtensionOptions) {
  const onError =
    opts.onError ?? ((where: string, err: unknown) => {
      console.warn(`[lsp] ${where}:`, err)
    })

  // ── ViewPlugin: lifecycle, didChange, diagnostics subscription ──
  const lifecycle = ViewPlugin.fromClass(
    class {
      private readonly view: EditorView
      private readonly state: PluginState
      private destroyed = false

      constructor(view: EditorView) {
        this.view = view
        this.state = {
          version: opts.initialVersion ?? 1,
          uri: inferUri(opts.relpath),
          diagnosticsUnsub: null,
        }
        // didOpen — fire & forget; no-op when the host has no server
        // routed for this extension (returns null).
        void opts.ipc
          .openFile({
            path: opts.relpath,
            content: view.state.doc.toString(),
            language_id: opts.languageId,
            version: this.state.version,
          })
          .catch((err) => onError('openFile', err))

        // Diagnostics subscription — every event with a matching URI
        // gets projected into the lint state.
        void opts.ipc
          .onDiagnostics((params: PublishDiagnosticsParams) => {
            if (this.destroyed) return
            // Server uses absolute file URIs; we accept either the
            // raw relpath or the file:// form the host wraps it in.
            if (
              params.uri !== this.state.uri &&
              !params.uri.endsWith(opts.relpath)
            ) {
              return
            }
            const diagnostics = lspDiagnosticsToCm(this.view, params.diagnostics)
            this.view.dispatch(setDiagnostics(this.view.state, diagnostics))
          })
          .then((unsub) => {
            if (this.destroyed) {
              unsub()
            } else {
              this.state.diagnosticsUnsub = unsub
            }
          })
          .catch((err) => onError('onDiagnostics', err))
      }

      update(update: import('@codemirror/view').ViewUpdate) {
        if (!update.docChanged) return
        // No debounce in code mode (BL-077 DoD).
        this.state.version += 1
        const text = update.state.doc.toString()
        void opts.ipc
          .changeFile({
            path: opts.relpath,
            content: text,
            version: this.state.version,
          })
          .catch((err) => onError('changeFile', err))
      }

      destroy() {
        this.destroyed = true
        this.state.diagnosticsUnsub?.()
        void opts.ipc
          .closeFile(opts.relpath)
          .catch((err) => onError('closeFile', err))
      }
    },
  )

  // ── Autocomplete source ──
  const completion = autocompletion({
    override: [
      async (ctx: CompletionContext): Promise<CompletionResult | null> => {
        // Match the typical identifier prefix; on explicit invocation
        // (Ctrl-Space) trigger even at non-word boundary.
        const word = ctx.matchBefore(/[A-Za-z_$][\w$]*/)
        if (!ctx.explicit && (!word || word.from === word.to)) return null
        const pos = ctx.state.doc.lineAt(ctx.pos)
        const line = pos.number - 1
        const character = ctx.pos - pos.from
        let raw: unknown
        try {
          raw = await opts.ipc.completions({
            path: opts.relpath,
            line,
            character,
          })
        } catch (err) {
          onError('completions', err)
          return null
        }
        if (!raw) return null
        // The reply is either `CompletionItem[]` or a `CompletionList`.
        const items: LspCompletionItem[] = Array.isArray(raw)
          ? (raw as LspCompletionItem[])
          : (raw as LspCompletionList).items ?? []
        if (items.length === 0) return null
        return {
          from: word ? word.from : ctx.pos,
          options: items.map(lspItemToCmCompletion),
        }
      },
    ],
  })

  // ── Hover tooltip ──
  const hover = hoverTooltip(async (view, pos): Promise<Tooltip | null> => {
    const lineInfo = view.state.doc.lineAt(pos)
    const line = lineInfo.number - 1
    const character = pos - lineInfo.from
    let raw: unknown
    try {
      raw = await opts.ipc.hover({
        path: opts.relpath,
        line,
        character,
      })
    } catch (err) {
      onError('hover', err)
      return null
    }
    const text = hoverText(raw as LspHover | null)
    if (text == null || text.trim().length === 0) return null
    const hoverObj = raw as LspHover | null
    let from = pos
    let to = pos
    if (hoverObj?.range != null) {
      from = lspPositionToOffset(view.state.doc, hoverObj.range.start)
      to = lspPositionToOffset(view.state.doc, hoverObj.range.end)
    }
    return {
      pos: from,
      end: to,
      above: true,
      create() {
        const dom = document.createElement('div')
        dom.className = 'cm-tooltip-lsp-hover'
        dom.textContent = text
        return { dom }
      },
    }
  })

  // ── Cmd/Ctrl-Click → go-to-definition ──
  const goToDef = EditorView.domEventHandlers({
    mousedown(event, view) {
      const isModClick = event.metaKey || event.ctrlKey
      if (!isModClick || event.button !== 0) return false
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
      if (pos == null) return false
      const lineInfo = view.state.doc.lineAt(pos)
      const line = lineInfo.number - 1
      const character = pos - lineInfo.from
      void opts.ipc
        .definition({
          path: opts.relpath,
          line,
          character,
        })
        .then((raw) => {
          const loc = pickFirstLocation(raw)
          if (loc != null) opts.onOpenLocation?.(loc)
        })
        .catch((err) => onError('definition', err))
      // Prevent text selection while we navigate.
      event.preventDefault()
      return true
    },
  })

  // ── Format-on-save (Mod-s) ──
  const formatKey = keymap.of([
    {
      key: 'Mod-s',
      run: (view) => {
        void opts.ipc
          .format(opts.relpath)
          .then((raw) => {
            const edits = raw as LspTextEdit[] | null
            if (!edits || edits.length === 0) return
            applyTextEdits(view, edits)
          })
          .catch((err) => onError('format', err))
        // Return false so the *outer* save handler still fires —
        // formatting is additive, not a replacement for the save
        // pipeline. Code mode's save lives in EditorView.tsx's
        // command palette wiring.
        return false
      },
    },
  ])

  // `setDiagnostics` only takes effect when the lint state field is
  // installed. `linter(() => [])` is the canonical "I'll push
  // diagnostics manually" pull-source — it registers the state
  // field without ever auto-running.
  const lintState = linter(() => [])

  return [
    lifecycle,
    completion,
    hover,
    goToDef,
    lintState,
    Prec.high(formatKey),
  ]
}

/**
 * The LSP `definition` reply is `Location | Location[] | null`. Pick
 * the first match if any (multi-target results are rare and the
 * caller usually wants the first hit).
 */
export function pickFirstLocation(raw: unknown): LspLocation | null {
  if (!raw) return null
  if (Array.isArray(raw)) return (raw[0] as LspLocation | undefined) ?? null
  return raw as LspLocation
}

/**
 * Apply LSP `TextEdit[]` to `view` in a single transaction. Edits
 * are sorted *bottom-up* (per LSP spec recommendation) so earlier
 * edits don't invalidate later positions.
 */
export function applyTextEdits(view: EditorView, edits: LspTextEdit[]): void {
  if (edits.length === 0) return
  const doc = view.state.doc
  const changes = edits
    .map((e) => ({
      from: lspPositionToOffset(doc, e.range.start),
      to: lspPositionToOffset(doc, e.range.end),
      insert: e.newText,
    }))
    .sort((a, b) => b.from - a.from)
  view.dispatch({ changes })
}
