// BL-039 — AI-DIR auto-link suggestions for CodeMirror 6.
//
// Layout (mirrors ghostCompletion.ts intentionally — same shape so the
// two ghost surfaces feel uniform from the user's side):
//
//   typing → debounce → extract trailing phrase → query
//          `com.nexus.ai::semantic_search` → if top score >= threshold
//          and the phrase isn't already inside a [[…]] / code block /
//          frontmatter, render a `[[basename|phrase]]` ghost widget
//          starting at the phrase's beginning.
//
// Tab accepts (replaces the typed phrase with the wiki-link form).
// Esc dismisses. Both bindings live at `Prec.high` so they don't
// shadow normal Tab/Esc when no link suggestion is active. To avoid
// stomping on BL-034's inline AI ghost, we DEFER (skip the round trip
// entirely) when a `ghostCompletion` suggestion is currently visible
// for the same view.
//
// Settings (read lazily so live edits in settings panel take effect
// without remounting the editor):
//
//   ai.linkSuggest.enabled       — master toggle (default true)
//   ai.linkSuggest.debounceMs    — quiet period after a keystroke (default 600)
//   ai.linkSuggest.minChars      — skip when phrase shorter (default 4)
//   ai.linkSuggest.maxChars      — phrase upper-bound (default 80)
//   ai.linkSuggest.scoreGate     — min top-match score (default 0.55)

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
import { __test__ as ghostInternals } from './ghostCompletion'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_SEMANTIC_SEARCH = 'semantic_search'

/**
 * A live link suggestion. The `phrase` covers `[from, to)` in the
 * document; the `replacement` is the full `[[…]]` form we will paste
 * over `phrase` if the user accepts.
 */
export interface LinkSuggestion {
  /** Inclusive start of the phrase the user typed. */
  from: number
  /** Exclusive end (== caret) of the phrase the user typed. */
  to: number
  /** Phrase text as it appears in the doc — used for the visual
   *  strikethrough / replacement preview. */
  phrase: string
  /** What we will splice in if the user accepts (e.g.
   *  `[[Foo Bar|foo bar]]` or `[[Foo Bar]]`). */
  replacement: string
  /** Monotonic id of the request that produced this. */
  requestId: number
}

const setSuggestion = StateEffect.define<LinkSuggestion | null>()

const suggestionField = StateField.define<LinkSuggestion | null>({
  create: () => null,
  update(value, tr) {
    // Any doc edit or selection move invalidates the suggestion —
    // unless the same transaction carries a fresh setSuggestion
    // (the fetcher dispatch path).
    if (tr.docChanged || tr.selection) {
      let next: LinkSuggestion | null = null
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

class LinkGhostWidget extends WidgetType {
  constructor(readonly text: string) {
    super()
  }
  toDOM(): HTMLElement {
    const span = document.createElement('span')
    span.className = 'cm-nexus-link-ghost'
    // Slightly bluer than the AI ghost so the two surfaces are
    // visually distinguishable when both fire in succession.
    span.style.cssText =
      'opacity:0.55;pointer-events:none;white-space:pre-wrap;color:var(--accent-muted, #6b8aff);'
    span.textContent = this.text
    return span
  }
  override eq(other: WidgetType): boolean {
    return other instanceof LinkGhostWidget && other.text === this.text
  }
  override ignoreEvent(): boolean {
    return true
  }
}

// Widget-after-caret: the phrase the user typed stays in place; we
// append the rest of the wiki-link form (`]]` and any basename
// prefix) so the caret reads the suggestion as natural overflow.
//
// Chosen over a strikethrough+replacement preview because the latter
// fights with selection rendering and frequently mis-paints when the
// user is actively typing.
const linkDecorations = EditorView.decorations.compute([suggestionField], (state) => {
  const sug = state.field(suggestionField)
  if (!sug) return Decoration.none
  // Render only the trailing portion the user hasn't typed yet —
  // i.e. everything in `replacement` that follows `phrase`. If the
  // replacement happens to start with `[[` and the phrase doesn't,
  // we surface that too by prepending it as a separate widget at
  // `from`. Implementation choice: render the WHOLE replacement as a
  // single widget at `to` (after the caret). The user's typed phrase
  // stays committed; the ghost reads as the link form they would get.
  const widget = Decoration.widget({
    widget: new LinkGhostWidget(' → ' + sug.replacement),
    side: 1,
  })
  return Decoration.set([widget.range(sug.to)])
})

interface LinkSuggestSettings {
  enabled: boolean
  debounceMs: number
  minChars: number
  maxChars: number
  scoreGate: number
}

function readSettings(): LinkSuggestSettings {
  return {
    enabled: configStore.get<boolean>('ai.linkSuggest.enabled', true),
    debounceMs: configStore.get<number>('ai.linkSuggest.debounceMs', 600),
    minChars: configStore.get<number>('ai.linkSuggest.minChars', 4),
    maxChars: configStore.get<number>('ai.linkSuggest.maxChars', 80),
    scoreGate: configStore.get<number>('ai.linkSuggest.scoreGate', 0.55),
  }
}

interface ChunkMatch {
  file_path: string
  block_id?: number
  chunk_text?: string
  score: number
}
interface SemanticResponse {
  matches?: ChunkMatch[]
}

/**
 * Strip directory prefix and trailing `.md` from a forge-relative
 * path. `notes/Foo Bar.md` → `Foo Bar`. Defensive against the path
 * being a basename already, or having no extension.
 */
export function basenameNoExt(relpath: string): string {
  const slash = relpath.lastIndexOf('/')
  const base = slash === -1 ? relpath : relpath.slice(slash + 1)
  return base.endsWith('.md') ? base.slice(0, -3) : base
}

/**
 * Build the wiki-link form for a top match. If the typed phrase is
 * exactly the basename (case-insensitive), use the bare `[[basename]]`
 * — the alias would just be noise.
 */
export function buildReplacement(filePath: string, phrase: string): string {
  const base = basenameNoExt(filePath)
  if (base.toLowerCase() === phrase.toLowerCase()) return `[[${base}]]`
  return `[[${base}|${phrase}]]`
}

/**
 * Pull the trailing "phrase" out of `text` ending at offset `caret`.
 *
 * Boundaries (in priority order): start-of-line (`\n`), sentence
 * boundary (`. `, `? `, `! `), or `caret - maxChars`. Trailing
 * whitespace at the caret terminates the phrase (we don't suggest
 * mid-trailing-space). Returns `null` if the resulting phrase is
 * shorter than `minChars` or contains no letters.
 */
export function extractPhrase(
  text: string,
  caret: number,
  minChars: number,
  maxChars: number,
): { phrase: string; from: number } | null {
  if (caret <= 0) return null
  // Phrase ends at the caret. We do NOT trim a trailing space — if
  // the user just hit space, the phrase is whatever preceded it.
  const end = caret
  // Don't trigger when caret sits in the middle of a word — wait for
  // a word boundary (space / punctuation / line end). The fetcher
  // already gates on the debounce; this is the static check.
  if (end < text.length && /\w/.test(text[end] ?? '')) return null

  const start = Math.max(0, end - maxChars)
  const window = text.slice(start, end)
  // Walk backwards looking for the nearest hard boundary. Sentence
  // terminators require a following space to avoid splitting on
  // abbreviations mid-word ("e.g." style); newline always wins.
  let boundary = -1
  for (let i = window.length - 1; i > 0; i--) {
    const ch = window[i - 1]
    if (ch === '\n') {
      boundary = i
      break
    }
    if (
      (ch === '.' || ch === '?' || ch === '!') &&
      i < window.length &&
      window[i] === ' '
    ) {
      // skip the space after the terminator
      boundary = i + 1
      break
    }
  }
  const absStart = boundary === -1 ? start : start + boundary
  let phrase = text.slice(absStart, end)
  // Trim leading whitespace — common when the boundary lands just
  // before an indent / bullet marker.
  const leading = phrase.match(/^[\s>*+\-#]+/)
  const phraseStart = absStart + (leading ? leading[0].length : 0)
  phrase = text.slice(phraseStart, end)
  // Trim a trailing single space so "foo bar " is treated as the
  // phrase "foo bar" — the user has clearly finished the word.
  let trimmedEnd = end
  while (trimmedEnd > phraseStart && text[trimmedEnd - 1] === ' ') {
    trimmedEnd--
  }
  phrase = text.slice(phraseStart, trimmedEnd)
  if (phrase.length < minChars) return null
  if (!/[A-Za-z]/.test(phrase)) return null
  return { phrase, from: phraseStart }
}

/**
 * Skip-zone detection. Returns true when `caret` sits inside an
 * existing `[[…]]` wiki-link, a fenced code block, or YAML
 * frontmatter (the leading `---` …  `---` block at file head).
 */
export function isInSkipZone(text: string, caret: number): boolean {
  // Frontmatter — only valid when the file STARTS with `---\n`.
  if (text.startsWith('---\n') || text.startsWith('---\r\n')) {
    const close = text.indexOf('\n---', 4)
    // Closing fence must be at the start of its own line — check the
    // byte preceding it is a newline (it is, by construction of the
    // search) and the byte after `---` is either EOL or EOF.
    if (close !== -1) {
      const after = close + 4
      const afterCh = text[after]
      if (afterCh === undefined || afterCh === '\n' || afterCh === '\r') {
        if (caret <= after) return true
      }
    } else if (caret <= text.length) {
      // No closing fence yet — the whole tail is frontmatter.
      return true
    }
  }

  // Existing wiki-link — the most recent `[[` before caret with no
  // intervening `]]` means we're inside one.
  const beforeCaret = text.slice(0, caret)
  const lastOpen = beforeCaret.lastIndexOf('[[')
  if (lastOpen !== -1) {
    const between = text.slice(lastOpen, caret)
    if (!between.includes(']]')) return true
  }

  // Fenced code block — count backtick-fence lines (` ``` `) before
  // the caret. Odd count means we're inside a fence. We only match
  // fences at the start of a line to avoid inline-code false positives.
  const fenceRe = /^```/gm
  let count = 0
  let m: RegExpExecArray | null
  while ((m = fenceRe.exec(beforeCaret)) !== null) {
    count++
  }
  if (count % 2 === 1) return true

  return false
}

const ghostFetcher = ViewPlugin.fromClass(
  class implements PluginValue {
    private timer: ReturnType<typeof setTimeout> | null = null
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
      if (this.timer) {
        clearTimeout(this.timer)
        this.timer = null
      }
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

    private async runFetch(settings: LinkSuggestSettings): Promise<void> {
      const api = getGhostApi()
      if (!api) return
      const view = this.view
      // Defer when BL-034's inline AI ghost is currently visible —
      // two ghosts with conflicting Tab semantics would confuse the
      // user. The AI ghost is short-lived; the next keystroke after
      // its dismissal lets us run.
      if (view.state.field(ghostInternals.suggestionField, false)) return

      const sel = view.state.selection.main
      if (!sel.empty) return
      const caret = sel.head
      const docStr = view.state.doc.toString()

      if (isInSkipZone(docStr, caret)) return
      const extracted = extractPhrase(
        docStr,
        caret,
        settings.minChars,
        settings.maxChars,
      )
      if (!extracted) return

      const requestId = this.nextRequestId++
      this.inFlightRequestId = requestId

      let result: SemanticResponse | null
      try {
        result = await api.kernel.invoke<SemanticResponse>(
          AI_PLUGIN_ID,
          HANDLER_SEMANTIC_SEARCH,
          { query: extracted.phrase, limit: 3 },
        )
      } catch {
        return
      }

      if (this.inFlightRequestId !== requestId) return
      const currentSel = view.state.selection.main
      if (!currentSel.empty || currentSel.head !== caret) return

      const top = result?.matches?.[0]
      if (!top || top.score < settings.scoreGate) return

      const replacement = buildReplacement(top.file_path, extracted.phrase)
      view.dispatch({
        effects: setSuggestion.of({
          from: extracted.from,
          to: caret,
          phrase: extracted.phrase,
          replacement,
          requestId,
        }),
      })
    }
  },
)

function acceptSuggestion(view: EditorView): boolean {
  const sug = view.state.field(suggestionField)
  if (!sug) return false
  const sel = view.state.selection.main
  if (!sel.empty || sel.head !== sug.to) return false
  view.dispatch({
    changes: { from: sug.from, to: sug.to, insert: sug.replacement },
    selection: { anchor: sug.from + sug.replacement.length },
    effects: setSuggestion.of(null),
  })
  return true
}

function dismissSuggestion(view: EditorView): boolean {
  const sug = view.state.field(suggestionField)
  if (!sug) return false
  view.dispatch({ effects: setSuggestion.of(null) })
  return true
}

const linkSuggestKeymap = keymap.of([
  { key: 'Tab', run: acceptSuggestion },
  { key: 'Escape', run: dismissSuggestion },
])

/** Build the BL-039 link-suggestion extension. Composed alongside
 *  `ghostCompletionExt` in the editor source-mode extension list. */
export function linkSuggestExt(): Extension {
  return [
    suggestionField,
    linkDecorations,
    ghostFetcher,
    Prec.high(linkSuggestKeymap),
  ]
}

export const __test__ = {
  setSuggestion,
  suggestionField,
  acceptSuggestion,
  dismissSuggestion,
  extractPhrase,
  isInSkipZone,
  basenameNoExt,
  buildReplacement,
}

export type { Transaction }
