// BL-034 — inline ghost-text AI completion for CodeMirror 6.
//
// Layout:
//
//   typing → debounce → fetch suggestion via `com.nexus.ai::stream_chat`
//          (mode=complete, tools=none, trim=true) → render as a
//          `Decoration.widget` after the caret.
//
// Tab accepts the suggestion (inserting it as plain text). Escape
// dismisses. Both bindings live at `Prec.high` so they don't shadow
// the editor's normal Tab/Esc handling when no suggestion is active.
//
// Single-flight + cancel-on-edit: one in-flight request per editor
// view. Any further keystroke before stream_done aborts the stale
// request — we tag the request with a monotonic id and ignore late
// resolutions. The ipc_call itself can't be cancelled mid-stream
// from this side, so we just discard the result if `requestId !==
// state.field(suggestionField).requestId` when it lands.
//
// Settings (read lazily so live edits in the AI settings panel take
// effect for the next request without remounting the editor):
//
//   ai.ghost.enabled       — master toggle (default true)
//   ai.ghost.debounceMs    — quiet-period after a keystroke (default 350)
//   ai.ghost.minChars      — skip when the prefix is shorter (default 8)
//   ai.ghost.contextChars  — prefix size sent to the engine (default 2000)
//   ai.ghost.maxTokens     — engine generation cap (default 64)

import {
  Prec,
  StateEffect,
  StateField,
  type Extension,
  type Transaction,
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

import { getGhostApi } from '../../ai/ghostApi'
import { configStore } from '../../../../stores/configStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_STREAM_CHAT = 'stream_chat'

interface GhostSuggestion {
  /** Document position the suggestion was generated for. We only
   *  surface the widget when the caret is still here — any move
   *  invalidates it. */
  pos: number
  /** The suggested continuation. Already trimmed by the engine; we
   *  do not rewrite it. */
  text: string
  /** Monotonic id matching the request that produced this. Lets the
   *  request handler ignore late resolutions for stale prefixes. */
  requestId: number
}

const setSuggestion = StateEffect.define<GhostSuggestion | null>()

const suggestionField = StateField.define<GhostSuggestion | null>({
  create: () => null,
  update(value, tr) {
    // Clear on any doc change or selection move. Cancel-on-edit
    // semantics — the user has moved past whatever the suggestion
    // was generated for.
    if (tr.docChanged || tr.selection) {
      let next: GhostSuggestion | null = null
      for (const e of tr.effects) {
        if (e.is(setSuggestion)) next = e.value
      }
      return next
    }
    let next = value
    for (const e of tr.effects) {
      if (e.is(setSuggestion)) next = e.value
    }
    return next
  },
})

class GhostWidget extends WidgetType {
  constructor(readonly text: string) {
    super()
  }
  // Ghost text reuses the editor's mono font but with a muted tone so
  // it doesn't read as committed content. `pointer-events: none` so
  // clicks land on the underlying line, not on the widget.
  toDOM(): HTMLElement {
    const span = document.createElement('span')
    span.className = 'cm-nexus-ghost'
    span.style.cssText =
      'opacity:0.45;pointer-events:none;white-space:pre-wrap;color:var(--fg-muted, #9ca3af);'
    span.textContent = this.text
    return span
  }
  override eq(other: WidgetType): boolean {
    return other instanceof GhostWidget && other.text === this.text
  }
  override ignoreEvent(): boolean {
    return true
  }
}

// Decorations are rebuilt from the field each transaction. Cheap —
// at most one widget per editor view.
const ghostDecorations = EditorView.decorations.compute([suggestionField], (state) => {
  const sug = state.field(suggestionField)
  if (!sug) return Decoration.none
  // Anchor the widget side=1 so it sits AFTER any character the user
  // has just typed (otherwise insert-at-caret reorders it).
  const widget = Decoration.widget({ widget: new GhostWidget(sug.text), side: 1 })
  return Decoration.set([widget.range(sug.pos)])
})

interface GhostSettings {
  enabled: boolean
  debounceMs: number
  minChars: number
  contextChars: number
  maxTokens: number
}

function readSettings(): GhostSettings {
  return {
    enabled: configStore.get<boolean>('ai.ghost.enabled', true),
    debounceMs: configStore.get<number>('ai.ghost.debounceMs', 350),
    minChars: configStore.get<number>('ai.ghost.minChars', 8),
    contextChars: configStore.get<number>('ai.ghost.contextChars', 2000),
    maxTokens: configStore.get<number>('ai.ghost.maxTokens', 64),
  }
}

interface ChatResponse {
  text?: string
}

// ViewPlugin owns the debounce + fetch lifecycle. State lives on the
// instance (timer + counter), not the field, because both are
// imperative + per-view.
const ghostFetcher = ViewPlugin.fromClass(
  class implements PluginValue {
    private timer: ReturnType<typeof setTimeout> | null = null
    private nextRequestId = 1
    private inFlightRequestId = 0

    constructor(readonly view: EditorView) {}

    update(update: ViewUpdate): void {
      // Cancel-on-edit: any user action invalidates the in-flight
      // request and the existing suggestion. Selection-only moves
      // also count — moving the caret past the suggestion makes it
      // meaningless.
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
      if (this.timer) {
        clearTimeout(this.timer)
        this.timer = null
      }
      // Bumping the in-flight id so any pending resolution lands
      // stale and is dropped.
      this.inFlightRequestId = 0
    }

    private scheduleFetch(): void {
      const settings = readSettings()
      if (!settings.enabled) return
      if (this.timer) clearTimeout(this.timer)
      this.timer = setTimeout(() => {
        this.timer = null
        void this.runFetch(settings)
      }, settings.debounceMs)
    }

    private async runFetch(settings: GhostSettings): Promise<void> {
      const api = getGhostApi()
      if (!api) return
      const view = this.view
      const sel = view.state.selection.main
      if (!sel.empty) return // a range selection — nothing to extend
      const pos = sel.head
      const doc = view.state.doc
      const start = Math.max(0, pos - settings.contextChars)
      const prefix = doc.sliceString(start, pos)
      if (prefix.length < settings.minChars) return
      // Heuristic skip: avoid firing while the user is mid-word so we
      // don't suggest before they've expressed intent. A trailing
      // space / newline / punctuation is the common "ready" cue.
      if (!/[\s\p{P}]$/u.test(prefix)) return

      const requestId = this.nextRequestId++
      this.inFlightRequestId = requestId
      const sessionId = `ghost-${requestId}-${Date.now()}`

      let result: ChatResponse | null
      try {
        result = await api.kernel.invoke<ChatResponse>(
          AI_PLUGIN_ID,
          HANDLER_STREAM_CHAT,
          {
            messages: [{ role: 'user', content: prefix }],
            session_id: sessionId,
            mode: 'complete',
            tools: 'none',
            trim: true,
            max_tokens: settings.maxTokens,
            stop: ['\n\n'],
          },
        )
      } catch {
        // API errors are silent for ghost — we don't want a misconfig
        // toast on every keystroke. The chat surface still surfaces
        // these via the existing settings panel.
        return
      }

      // Stale-request guard — anything that bumped invalidate() since
      // we issued the request rules out this resolution.
      if (this.inFlightRequestId !== requestId) return
      // Caret-moved guard — the user typed during the network round
      // trip, so the suggested continuation no longer follows their
      // current prefix.
      const currentSel = view.state.selection.main
      if (!currentSel.empty || currentSel.head !== pos) return

      const text = (result?.text ?? '').replace(/^[ \t]+/, '')
      if (!text) return
      view.dispatch({
        effects: setSuggestion.of({ pos, text, requestId }),
      })
    }
  },
)

/** Accept the current suggestion at the caret. Called by the
 *  high-precedence Tab binding; `false` falls through so Tab still
 *  inserts a tab when there's nothing to accept. */
function acceptSuggestion(view: EditorView): boolean {
  const sug = view.state.field(suggestionField)
  if (!sug) return false
  const sel = view.state.selection.main
  if (!sel.empty || sel.head !== sug.pos) return false
  view.dispatch({
    changes: { from: sug.pos, to: sug.pos, insert: sug.text },
    selection: { anchor: sug.pos + sug.text.length },
    effects: setSuggestion.of(null),
  })
  return true
}

/** Dismiss the current suggestion. False-fallthrough behaviour so
 *  Esc still bubbles to the search panel / overlay close handlers
 *  when there's no ghost in flight. */
function dismissSuggestion(view: EditorView): boolean {
  const sug = view.state.field(suggestionField)
  if (!sug) return false
  view.dispatch({ effects: setSuggestion.of(null) })
  return true
}

const ghostKeymap = keymap.of([
  { key: 'Tab', run: acceptSuggestion },
  { key: 'Escape', run: dismissSuggestion },
])

/** Build the BL-034 ghost-completion extension. Caller composes this
 *  alongside the rest of the source-mode CodeMirror extensions. */
export function ghostCompletionExt(): Extension {
  return [
    suggestionField,
    ghostDecorations,
    ghostFetcher,
    // High precedence so our Tab/Esc beat anything in the baseline /
    // search keymap when a suggestion is live. The handlers return
    // `false` when no suggestion exists, which lets the editor's
    // normal Tab/Esc resume.
    Prec.high(ghostKeymap),
  ]
}

// Re-export for tests that need to drive the field directly.
export const __test__ = { setSuggestion, suggestionField, acceptSuggestion, dismissSuggestion }

// Ensure the dynamic side-effect StateEffect constructor isn't
// considered unused if the field's update path is the only consumer.
export type { Transaction }
