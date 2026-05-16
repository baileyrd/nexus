// BL-139 — per-keystroke ghost-text edit prediction.
//
// Sibling to `ghostCompletion.ts` (BL-034) — the older extension uses
// the chat-style `stream_chat (mode=complete)` path with prefix-only
// context, which is fine for prose but weak for code. BL-139 routes
// through the dedicated `com.nexus.ai::predict` handler: the backend
// uses Ollama's FIM endpoint (`/api/generate` with `suffix`) by default
// so a code model can fill the cursor split natively.
//
// Lifecycle:
//
//   keystroke / selection move
//     → invalidate any prior ghost
//     → schedule a debounced fetch (150 ms by default)
//     → ipc_call('com.nexus.ai', 'predict', { prefix, suffix, language, file_path })
//     → render returned completion as a Decoration.widget after the caret
//
//   Tab     → accept (dispatch an insert transaction at the caret)
//   Escape  → dismiss (clear the ghost field)
//   any other keystroke → invalidate via the same StateField predicate
//
// Settings (read lazily so live edits in the Settings panel take effect
// without remounting the editor):
//
//   nexus.editor.editPrediction.enabled    — master toggle (default false; opt-in)
//   nexus.editor.editPrediction.provider   — informational only; the AI
//                                            handler routes based on its own
//                                            configured provider
//   nexus.editor.editPrediction.model      — informational only (same reason)
//   nexus.editor.editPrediction.debounceMs — quiet-period after a keystroke
//                                            (default 150)

import {
  Prec,
  StateEffect,
  StateField,
  type Extension,
} from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  keymap,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'

import { getEditPredictionApi } from './editPredictionApi'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_PREDICT = 'predict'

/** Per-call cap on the size of the prefix slice we send upstream.
 *  ~200 tokens at ~4 chars/token. The BL DoD targets ≤200 token
 *  prefix latency; keeping this bounded means the editor never
 *  ships a 5 MB file to the AI plugin. */
const PREFIX_BYTE_BUDGET = 800
/** Suffix is smaller — most FIM models weight prefix > suffix and
 *  the suffix budget mostly buys disambiguation. */
const SUFFIX_BYTE_BUDGET = 200

interface EditPrediction {
  /** Document position the prediction was generated for. We only
   *  surface the widget when the caret is still here — any move
   *  invalidates it. */
  pos: number
  /** The suggested continuation. Already sanitised by the handler. */
  text: string
  /** Monotonic id matching the request that produced this. Lets the
   *  fetcher ignore late resolutions for stale prefixes. */
  requestId: number
}

const setPrediction = StateEffect.define<EditPrediction | null>()

const predictionField = StateField.define<EditPrediction | null>({
  create: () => null,
  update(value, tr) {
    // Cancel-on-edit + cancel-on-move. Mirrors ghostCompletion's
    // invalidation rule: any user action invalidates the suggestion
    // unless a fresh setPrediction effect arrives in the same
    // transaction (the fetcher pattern).
    if (tr.docChanged || tr.selection) {
      let next: EditPrediction | null = null
      for (const e of tr.effects) {
        if (e.is(setPrediction)) next = e.value
      }
      return next
    }
    let next = value
    for (const e of tr.effects) {
      if (e.is(setPrediction)) next = e.value
    }
    return next
  },
})

class EditPredictionWidget extends WidgetType {
  constructor(readonly text: string) {
    super()
  }
  toDOM(): HTMLElement {
    const span = document.createElement('span')
    span.className = 'cm-nexus-edit-prediction'
    span.style.cssText =
      'opacity:0.45;pointer-events:none;white-space:pre-wrap;color:var(--text-muted);'
    span.textContent = this.text
    return span
  }
  override eq(other: WidgetType): boolean {
    return other instanceof EditPredictionWidget && other.text === this.text
  }
  override ignoreEvent(): boolean {
    return true
  }
}

const predictionDecorations = EditorView.decorations.compute(
  [predictionField],
  (state) => {
    const sug = state.field(predictionField)
    if (!sug) return Decoration.none
    const widget = Decoration.widget({
      widget: new EditPredictionWidget(sug.text),
      side: 1,
    })
    return Decoration.set([widget.range(sug.pos)])
  },
)

export interface EditPredictionSettings {
  enabled: boolean
  debounceMs: number
}

export type SettingsReader = () => EditPredictionSettings

export interface EditPredictionOptions {
  /** Forge-relative path of the buffer this extension is attached
   *  to. Forwarded verbatim to the predict handler — the handler
   *  doesn't use it today but the backend logs it on the activity
   *  trail and a future revision may surface per-file caches. */
  relpath: string
  /** Source language hint (`rust`, `typescript`, `markdown`, …).
   *  Caller derives this from the file extension. */
  language: string
  /** Settings reader. Re-evaluated on every keystroke so a flip in
   *  the Settings panel takes effect immediately without remounting
   *  the editor. */
  readSettings: SettingsReader
}

interface PredictResponse {
  completion?: string
}

/** Tiny debouncer so the fetcher's scheduling rule is unit-testable
 *  without a real EditorView. `schedule(delayMs, fn)` cancels any
 *  pending call and re-arms; `cancel()` drops any pending call. */
export class Debouncer {
  private timer: ReturnType<typeof setTimeout> | null = null

  schedule(delayMs: number, fn: () => void): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      fn()
    }, delayMs)
  }

  cancel(): void {
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
  }
}

/** Trim `s` to `maxBytes` UTF-8 bytes from the END (prefix-style).
 *  Snaps to a UTF-16 code-unit boundary; the resulting string may
 *  be a few bytes under the cap. Empty string when `maxBytes` is 0.
 *  Pure — exercised in unit tests. */
export function tailByBytes(s: string, maxBytes: number): string {
  if (maxBytes <= 0 || s.length === 0) return ''
  // Cheap upper bound: 1 char ≤ 4 bytes in UTF-8. Start from the end
  // and walk forward enough chars to be safe, then trim by byte
  // length precisely.
  const candidate = s.slice(Math.max(0, s.length - maxBytes))
  let bytes = new TextEncoder().encode(candidate).length
  let trimmed = candidate
  while (bytes > maxBytes && trimmed.length > 0) {
    trimmed = trimmed.slice(1)
    bytes = new TextEncoder().encode(trimmed).length
  }
  return trimmed
}

/** Trim `s` to `maxBytes` UTF-8 bytes from the START (suffix-style). */
export function headByBytes(s: string, maxBytes: number): string {
  if (maxBytes <= 0 || s.length === 0) return ''
  const candidate = s.slice(0, maxBytes)
  let bytes = new TextEncoder().encode(candidate).length
  let trimmed = candidate
  while (bytes > maxBytes && trimmed.length > 0) {
    trimmed = trimmed.slice(0, -1)
    bytes = new TextEncoder().encode(trimmed).length
  }
  return trimmed
}

const editPredictionFetcher = (options: EditPredictionOptions) =>
  ViewPlugin.fromClass(
    class implements PluginValue {
      private debouncer = new Debouncer()
      private nextRequestId = 1
      private inFlightRequestId = 0

      constructor(readonly view: EditorView) {}

      update(update: ViewUpdate): void {
        if (update.docChanged || update.selectionSet) {
          this.invalidate()
          if (update.docChanged) {
            this.scheduleFetch()
          }
        }
      }

      destroy(): void {
        this.invalidate()
      }

      private invalidate(): void {
        this.debouncer.cancel()
        // Bumping the in-flight id so any pending resolution lands
        // stale and is dropped.
        this.inFlightRequestId = 0
      }

      private scheduleFetch(): void {
        const settings = options.readSettings()
        if (!settings.enabled) return
        this.debouncer.schedule(settings.debounceMs, () => {
          void this.runFetch()
        })
      }

      private async runFetch(): Promise<void> {
        const api = getEditPredictionApi()
        if (!api) return
        const view = this.view
        const sel = view.state.selection.main
        if (!sel.empty) return
        const pos = sel.head
        const doc = view.state.doc
        const fullText = doc.toString()
        const prefix = tailByBytes(fullText.slice(0, pos), PREFIX_BYTE_BUDGET)
        const suffix = headByBytes(fullText.slice(pos), SUFFIX_BYTE_BUDGET)

        const requestId = this.nextRequestId++
        this.inFlightRequestId = requestId

        let result: PredictResponse | null
        try {
          result = await api.kernel.invoke<PredictResponse>(
            AI_PLUGIN_ID,
            HANDLER_PREDICT,
            {
              prefix,
              suffix,
              language: options.language,
              file_path: options.relpath,
            },
          )
        } catch {
          // Silent — a per-keystroke toast would be unusable.
          return
        }

        // Stale-request guard.
        if (this.inFlightRequestId !== requestId) return
        // Caret-moved guard.
        const currentSel = view.state.selection.main
        if (!currentSel.empty || currentSel.head !== pos) return

        const text = result?.completion ?? ''
        if (!text) return
        view.dispatch({
          effects: setPrediction.of({ pos, text, requestId }),
        })
      }
    },
  )

function acceptPrediction(view: EditorView): boolean {
  const sug = view.state.field(predictionField, false)
  if (!sug) return false
  const sel = view.state.selection.main
  if (!sel.empty || sel.head !== sug.pos) return false
  view.dispatch({
    changes: { from: sug.pos, to: sug.pos, insert: sug.text },
    selection: { anchor: sug.pos + sug.text.length },
    effects: setPrediction.of(null),
  })
  return true
}

function dismissPrediction(view: EditorView): boolean {
  const sug = view.state.field(predictionField, false)
  if (!sug) return false
  view.dispatch({ effects: setPrediction.of(null) })
  return true
}

const editPredictionKeymap = keymap.of([
  { key: 'Tab', run: acceptPrediction },
  { key: 'Escape', run: dismissPrediction },
])

/** Build the BL-139 edit-prediction extension. The extension is
 *  always installed but lies dormant until `readSettings().enabled`
 *  flips true — no background requests, no widgets, no keymap
 *  interception (Tab/Esc fall through when the field is empty). */
export function editPredictionExt(options: EditPredictionOptions): Extension {
  return [
    predictionField,
    predictionDecorations,
    editPredictionFetcher(options),
    Prec.high(editPredictionKeymap),
  ]
}

// Test surface — covers the state-machine + sanitiser without
// needing a real EditorView. Same pattern as ghostCompletion.
export const __test__ = {
  setPrediction,
  predictionField,
  acceptPrediction,
  dismissPrediction,
  tailByBytes,
  headByBytes,
  Debouncer,
  PREFIX_BYTE_BUDGET,
  SUFFIX_BYTE_BUDGET,
}
