// shell/src/plugins/nexus/ai/marginSuggestStore.ts
//
// BL-036 phase 1 — transient state for the AMB margin-suggestions
// engine. Holds the most recent pass's structured suggestions plus
// the lifecycle status driving the gutter glyph + squiggle UI that
// land in phases 2/3.
//
// Lifecycle of a single pass:
//
//   beginPass(reqId, docPath, gen) → status 'pending', clears prior
//                                    pass's `suggestions` (so the UI
//                                    doesn't decorate against a doc
//                                    snapshot we no longer hold).
//   setSuggestions(reqId, list)    → status 'done', `suggestions = list`.
//   setError(reqId, err)           → status 'error', `lastError = err`.
//   dismiss(id) / accept(id)       → drops one suggestion in place
//                                    without changing status.
//   clear()                        → drops everything; used when the
//                                    active doc closes.
//
// Stale-result guard: every mutation that takes a `requestId` must
// match `currentRequestId`. A late stream from a superseded pass is
// silently dropped — same staleness rule the chat store uses (BL-016
// `aiStore.appendChunk`).

import { create } from 'zustand'

/** The five suggestion classes shipped in v1. Spelling/grammar are
 *  rendered as wavy underlines (phase 3); the other three render as
 *  margin glyphs that expand into accept/dismiss diff cards (phase
 *  2). The split exists at the type level so the CM extensions in
 *  phase 2/3 can fan out cleanly. */
export type SuggestionKind =
  | 'rephrase'
  | 'tighten'
  | 'fact-check'
  | 'spelling'
  | 'grammar'

export interface Suggestion {
  /** Stable per-pass id. Format: `${requestId}-${index}`. Used as
   *  the React key in the gutter and as the dismissal handle. */
  id: string
  kind: SuggestionKind
  /** Inclusive char offset in the analyzed doc snapshot. The CM
   *  extension maps these into Decoration ranges using the doc
   *  changes since `generatedFor`. */
  rangeFrom: number
  /** Exclusive char offset; `[rangeFrom, rangeTo)` is the original
   *  span. */
  rangeTo: number
  /** Original text in [rangeFrom, rangeTo). Kept verbatim so the
   *  decoration mapper can verify the underlying text still matches
   *  before applying — the user may have edited the span between
   *  the pass and the click. */
  original: string
  /** Engine-proposed replacement. `null` means "annotation only" —
   *  fact-check leaves the text alone and just surfaces the
   *  message; the diff card renders without an accept button. */
  replacement: string | null
  /** One-line reason shown on the gutter glyph hover and in the
   *  diff card. Capped to ~120 chars by the parser. */
  message: string
  /** 1-based line of `rangeFrom` in the analyzed snapshot. Lets
   *  the gutter sort glyphs without re-walking the doc. */
  line: number
  /** Doc-version counter the engine assigned at `beginPass`. The
   *  CM extension uses this to ignore decorations whose anchored
   *  text has drifted. */
  generatedFor: number
}

export type MarginSuggestStatus =
  | 'idle'
  | 'pending'
  | 'done'
  | 'error'

export interface MarginSuggestState {
  status: MarginSuggestStatus
  /** Suggestions from the most recent successful pass. Cleared at
   *  `beginPass` so a long-running pass doesn't leave stale glyphs
   *  decorating a freshly-edited doc. */
  suggestions: Suggestion[]
  /** Forge-relative path the pass is/was scoped to. Mismatched
   *  paths are dropped at the runtime level; the store keeps it so
   *  the CM extension can sanity-check before mounting. */
  currentDocPath: string | null
  /** Monotonically-increasing per-pass counter. The runtime owns
   *  the increment; the store just records the value the engine
   *  is currently passing through. */
  currentGeneration: number
  /** Correlation id for the in-flight pass; null when idle. */
  currentRequestId: string | null
  /** Sticky error from the last failed pass. Cleared on next
   *  `beginPass`. */
  lastError: Error | null

  beginPass(requestId: string, docPath: string, generation: number): void
  setSuggestions(requestId: string, list: Suggestion[]): void
  setError(requestId: string, err: Error): void
  dismiss(id: string): void
  accept(id: string): void
  clear(): void
}

const INITIAL: Pick<
  MarginSuggestState,
  | 'status'
  | 'suggestions'
  | 'currentDocPath'
  | 'currentGeneration'
  | 'currentRequestId'
  | 'lastError'
> = {
  status: 'idle',
  suggestions: [],
  currentDocPath: null,
  currentGeneration: 0,
  currentRequestId: null,
  lastError: null,
}

export const useMarginSuggestStore = create<MarginSuggestState>((set, get) => ({
  ...INITIAL,

  beginPass: (requestId, docPath, generation) =>
    set({
      status: 'pending',
      suggestions: [],
      currentDocPath: docPath,
      currentGeneration: generation,
      currentRequestId: requestId,
      lastError: null,
    }),

  setSuggestions: (requestId, list) => {
    const state = get()
    if (state.currentRequestId !== requestId) return // stale pass
    set({
      status: 'done',
      suggestions: list,
      currentRequestId: null,
    })
  },

  setError: (requestId, err) => {
    const state = get()
    if (state.currentRequestId !== requestId) return // stale pass
    set({
      status: 'error',
      lastError: err,
      currentRequestId: null,
    })
  },

  dismiss: (id) =>
    set((s) => ({
      suggestions: s.suggestions.filter((x) => x.id !== id),
    })),

  // Phase 1: accept is identical to dismiss at the store level — the
  // diff application happens against the editor view in phase 2.
  // Splitting them now lets the runtime/UI dispatch through distinct
  // verbs from day one.
  accept: (id) =>
    set((s) => ({
      suggestions: s.suggestions.filter((x) => x.id !== id),
    })),

  clear: () => set({ ...INITIAL }),
}))
