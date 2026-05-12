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
// Tab cycles through the top-N (≤3) candidates returned by the
// ranker, wrapping 0→1→2→0; Enter accepts the currently-visible
// candidate; Esc dismisses. With a single candidate Tab is a visual
// no-op. All bindings live at `Prec.high` so they don't shadow
// normal Tab/Enter/Esc when no link suggestion is mounted. To avoid
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

/**
 * Cycle-aware suggestion state. The ranker returns up to three
 * candidates with the same `from/to/phrase/requestId` and differing
 * `replacement`s; `index` tracks which one the ghost is currently
 * showing. `index` advances on Tab and resets to 0 on every fresh
 * fetch.
 */
export interface SuggestionState {
  candidates: LinkSuggestion[]
  index: number
}

const setSuggestion = StateEffect.define<SuggestionState | null>()
const cycleSuggestion = StateEffect.define<void>()

const suggestionField = StateField.define<SuggestionState | null>({
  create: () => null,
  update(value, tr) {
    // Cycle effects don't carry a doc/selection mutation but we still
    // want to honour them when a fresh setSuggestion is in the same
    // transaction (the latter wins). Apply set first so cycle moves
    // the index of the post-set state.
    let next = value
    let didSet = false
    for (const e of tr.effects) {
      if (e.is(setSuggestion)) {
        next = e.value
        didSet = true
      }
    }

    // A doc edit or caret move with no setSuggestion piggy-backed
    // invalidates the suggestion — the user typed/clicked, so the
    // cycle resets along with the candidate list.
    if (!didSet && (tr.docChanged || tr.selection)) {
      return null
    }

    for (const e of tr.effects) {
      if (e.is(cycleSuggestion) && next && next.candidates.length > 0) {
        next = {
          candidates: next.candidates,
          index: (next.index + 1) % next.candidates.length,
        }
      }
    }
    return next
  },
})

/** Read the visible candidate (or `null` when none is mounted). */
function activeCandidate(state: SuggestionState | null): LinkSuggestion | null {
  if (!state || state.candidates.length === 0) return null
  return state.candidates[state.index] ?? null
}

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
      'opacity:0.55;pointer-events:none;white-space:pre-wrap;color:var(--accent-muted);'
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
  const active = activeCandidate(sug)
  if (!sug || !active) return Decoration.none
  // Render only the trailing portion the user hasn't typed yet —
  // i.e. everything in `replacement` that follows `phrase`. If the
  // replacement happens to start with `[[` and the phrase doesn't,
  // we surface that too by prepending it as a separate widget at
  // `from`. Implementation choice: render the WHOLE replacement as a
  // single widget at `to` (after the caret). The user's typed phrase
  // stays committed; the ghost reads as the link form they would get.
  // When more than one candidate is queued, append a small `(1/3)`
  // counter so the user knows Tab will cycle.
  const counter =
    sug.candidates.length > 1
      ? ` (${sug.index + 1}/${sug.candidates.length})`
      : ''
  const widget = Decoration.widget({
    widget: new LinkGhostWidget(' → ' + active.replacement + counter),
    side: 1,
  })
  return Decoration.set([widget.range(active.to)])
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
  while (fenceRe.exec(beforeCaret) !== null) {
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

      const matches = (result?.matches ?? []).filter(
        (m) => m && typeof m.file_path === 'string' && m.score >= settings.scoreGate,
      )
      if (matches.length === 0) return

      // Cap at three — the cycle is only useful when there are
      // genuinely distinct alternatives, and a longer queue makes
      // Tab feel unbounded. Drop duplicate `file_path` entries so
      // the same target doesn't appear twice when the ranker
      // returns multiple chunks from one file.
      const seenPaths = new Set<string>()
      const candidates: LinkSuggestion[] = []
      for (const m of matches) {
        if (seenPaths.has(m.file_path)) continue
        seenPaths.add(m.file_path)
        candidates.push({
          from: extracted.from,
          to: caret,
          phrase: extracted.phrase,
          replacement: buildReplacement(m.file_path, extracted.phrase),
          requestId,
        })
        if (candidates.length === 3) break
      }
      view.dispatch({
        effects: setSuggestion.of({ candidates, index: 0 }),
      })
    }
  },
)

function acceptSuggestion(view: EditorView): boolean {
  const state = view.state.field(suggestionField)
  const active = activeCandidate(state)
  if (!active) return false
  const sel = view.state.selection.main
  if (!sel.empty || sel.head !== active.to) return false
  view.dispatch({
    changes: { from: active.from, to: active.to, insert: active.replacement },
    selection: { anchor: active.from + active.replacement.length },
    effects: setSuggestion.of(null),
  })
  return true
}

function cycleNextSuggestion(view: EditorView): boolean {
  const state = view.state.field(suggestionField)
  if (!state || state.candidates.length === 0) return false
  // With a single candidate there's nothing to cycle to — let Tab
  // fall through to the default tab handler so editing still feels
  // natural in that case.
  if (state.candidates.length < 2) return false
  view.dispatch({ effects: cycleSuggestion.of() })
  return true
}

function dismissSuggestion(view: EditorView): boolean {
  const sug = view.state.field(suggestionField)
  if (!sug) return false
  view.dispatch({ effects: setSuggestion.of(null) })
  return true
}

const linkSuggestKeymap = keymap.of([
  // Tab cycles when there is more than one candidate; Enter accepts
  // the visible one. With a single candidate Tab falls through (the
  // user can still accept via Enter).
  { key: 'Tab', run: cycleNextSuggestion },
  { key: 'Enter', run: acceptSuggestion },
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
  cycleSuggestion,
  suggestionField,
  acceptSuggestion,
  cycleNextSuggestion,
  dismissSuggestion,
  activeCandidate,
  extractPhrase,
  isInSkipZone,
  basenameNoExt,
  buildReplacement,
}

export type { Transaction }
