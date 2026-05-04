// shell/src/plugins/nexus/ai/marginSuggest.ts
//
// BL-036 phase 1 — background-pass engine for ambient margin
// suggestions + inline correction (AMB plan pattern 6 / 9 in the
// backlog cross-walk; see docs/PRDs/BACKLOG.md §BL-036).
//
// One pass = one `kernel.invoke('com.nexus.ai', 'stream_chat', …)`
// call against a frozen doc snapshot. The model is asked for a JSON
// list of small, high-confidence improvements; we parse, locate
// each `original` substring inside the snapshot to anchor it, and
// hand the resulting `Suggestion[]` to `useMarginSuggestStore`.
//
// What this layer is NOT:
//   - It does NOT debounce. Phase 2 (the CM extension) decides when
//     to fire — typically idle-debounced on doc changes, but tests
//     and the right-click "rescan" affordance also want to pass
//     on demand without waiting for a timer.
//   - It does NOT mount any UI. The store is the contract; phases
//     2/3 read from it.
//   - It does NOT stream. Models emit small JSON arrays; chunking
//     them into the gutter as they arrive would only flicker the
//     decorations. We `await` the full `result.text` and parse once.
//
// Single-flight: a fresh `requestPass` while another is in flight
// supersedes the prior request (its result will be dropped by the
// store's staleness guard on `currentRequestId`). Callers that want
// debouncing should wrap this with a timer.

import type { PluginAPI } from '../../../types/plugin'
import {
  useMarginSuggestStore,
  type Suggestion,
  type SuggestionKind,
} from './marginSuggestStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
/** Same handler the Cmd+I overlay uses (see `cmdIRuntime.ts`) — we
 *  don't need RAG retrieval for a writing-suggestion pass over a
 *  single doc, so `stream_chat` (no embedding lookup) is the right
 *  shape. */
const HANDLER_STREAM_CHAT = 'stream_chat'

/** Wall-clock budget per pass. Kept short — a margin pass that takes
 *  a full minute would mean the user has typed a paragraph since it
 *  started, so the result is almost certainly stale anyway. */
const MARGIN_PASS_TIMEOUT_MS = 30_000

/** Soft cap on per-pass suggestions. Models occasionally over-
 *  produce; gutter clutter is worse than missing a fix. The cap is
 *  applied AFTER validation so we don't drop high-confidence items
 *  in favour of low-confidence ones. */
const MAX_SUGGESTIONS_PER_PASS = 6

/** Cap on the message text rendered on hover / in the diff card.
 *  Gutter glyph hover is a tooltip; long messages would wrap into
 *  the editor. */
const MAX_MESSAGE_CHARS = 120

/** Correlation prefix lets a future unified router tell margin
 *  passes apart from chat / Cmd+I requests on the shared
 *  `com.nexus.ai.stream_*` subscription. Phase 1 doesn't subscribe
 *  to that bus — we read the result via `invoke()`'s promise — but
 *  using a distinct prefix from day one means the chat router's
 *  staleness guard (which drops chunks for unknown ids) won't ever
 *  collide with our session ids. */
const MARGIN_SESSION_PREFIX = 'margin-'

/** True iff the given session id was minted by this engine. Mirrors
 *  the `isCmdISessionId` shape so the future unified router can
 *  branch cleanly. */
export function isMarginSessionId(id: string): boolean {
  return typeof id === 'string' && id.startsWith(MARGIN_SESSION_PREFIX)
}

const KIND_SET: ReadonlySet<SuggestionKind> = new Set<SuggestionKind>([
  'rephrase',
  'tighten',
  'fact-check',
  'spelling',
  'grammar',
])

/** Engine prompt. Asks the model for a JSON array — no streaming
 *  prose — so the parser is a single `JSON.parse`. The "exact
 *  substring" rule is the load-bearing constraint: we drop any
 *  entry whose `original` we can't locate in the doc, which both
 *  guards against hallucinated rewrites and lets us anchor without
 *  the model emitting line/col offsets (which models are bad at). */
export function buildSuggestionPrompt(docText: string): string {
  return [
    'You are a writing assistant. Read the document below and identify up to',
    `${MAX_SUGGESTIONS_PER_PASS} small, high-confidence improvements. Output ONLY a JSON`,
    'array, no prose, no markdown fences. Each item:',
    '{',
    '  "kind": "rephrase" | "tighten" | "fact-check" | "spelling" | "grammar",',
    '  "original": "<exact substring from the document>",',
    '  "replacement": "<rewrite, or empty string for fact-check>",',
    '  "message": "<one-line reason, ≤80 chars>"',
    '}',
    'Rules:',
    '- "original" MUST be an exact substring of the document.',
    '- Skip if you have ANY uncertainty.',
    '- Prefer "tighten" over "rephrase" when shortening works.',
    '- Use "fact-check" only when a factual claim looks dubious; replacement="".',
    '- Use "spelling"/"grammar" only for clear errors, not stylistic choices.',
    '- Do not duplicate suggestions for the same span.',
    '',
    'DOCUMENT:',
    docText,
    '',
    'JSON:',
  ].join('\n')
}

/** Mint a fresh pass session id. Format keeps `requestId` collision-
 *  resistant across concurrent windows / forge-roots. */
function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${MARGIN_SESSION_PREFIX}${crypto.randomUUID()}`
  }
  return `${MARGIN_SESSION_PREFIX}${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

/** Locate `needle` in `haystack` starting at `fromIndex`. Returns
 *  the offset, or -1 if absent. Wraps `String.prototype.indexOf` so
 *  the parser can swap in fuzzy matching later without touching the
 *  call site. */
function findFrom(haystack: string, needle: string, fromIndex: number): number {
  if (needle.length === 0) return -1
  return haystack.indexOf(needle, fromIndex)
}

/** 1-based line of `offset` in `text`. Cheap newline scan; fine
 *  for the per-pass cap of 6 suggestions. */
function lineOfOffset(text: string, offset: number): number {
  if (offset <= 0) return 1
  let line = 1
  const end = Math.min(offset, text.length)
  for (let i = 0; i < end; i += 1) {
    if (text.charCodeAt(i) === 10 /* \n */) line += 1
  }
  return line
}

/** Strip a leading ```json fence + trailing ``` if the model
 *  emitted one despite the "no markdown" instruction. Tolerates
 *  the `json` tag being absent or capitalised. */
function stripFence(text: string): string {
  const trimmed = text.trim()
  if (!trimmed.startsWith('```')) return trimmed
  const firstNl = trimmed.indexOf('\n')
  if (firstNl === -1) return trimmed
  const afterFence = trimmed.slice(firstNl + 1)
  const closing = afterFence.lastIndexOf('```')
  if (closing === -1) return afterFence.trim()
  return afterFence.slice(0, closing).trim()
}

interface RawSuggestion {
  kind?: unknown
  original?: unknown
  replacement?: unknown
  message?: unknown
}

/** Parse + validate the model's JSON response against the doc
 *  snapshot. Filters out:
 *
 *   - non-object entries
 *   - unknown `kind` values
 *   - entries whose `original` isn't a substring of the doc
 *     (hallucinated rewrites)
 *   - duplicates (same `original` + `kind` pair)
 *
 *  The remaining entries are anchored by walking the doc left-to-
 *  right: each suggestion takes the FIRST occurrence of its
 *  `original` after the prior suggestion's end. This both gives
 *  document-order glyphs in the gutter and prevents two
 *  suggestions from racing for the same span when the model emits
 *  the same `original` substring twice.
 *
 *  Returns at most `MAX_SUGGESTIONS_PER_PASS` items; over-produced
 *  passes are truncated. The truncation is post-validation so a
 *  noisy model can't push valid items off the end with junk.
 *
 *  Exposed for tests — production callers go through `requestPass`. */
export function parseSuggestionsResponse(
  text: string,
  docText: string,
  generation: number,
  requestId: string,
): Suggestion[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(stripFence(text))
  } catch {
    return []
  }
  if (!Array.isArray(parsed)) return []

  const seen = new Set<string>()
  const out: Suggestion[] = []
  let cursor = 0

  for (const raw of parsed) {
    if (!raw || typeof raw !== 'object') continue
    const r = raw as RawSuggestion

    if (typeof r.kind !== 'string' || !KIND_SET.has(r.kind as SuggestionKind)) continue
    if (typeof r.original !== 'string' || r.original.length === 0) continue
    const kind = r.kind as SuggestionKind
    const original = r.original
    const replacement =
      typeof r.replacement === 'string' && r.replacement.length > 0
        ? r.replacement
        : null
    const message =
      typeof r.message === 'string'
        ? r.message.slice(0, MAX_MESSAGE_CHARS)
        : ''

    // Dedupe by `kind|original` — the model occasionally emits the
    // same suggestion twice with slightly different messages.
    const dedupeKey = `${kind}|${original}`
    if (seen.has(dedupeKey)) continue

    // Anchor: first occurrence at or after `cursor`. If the model
    // emitted a later suggestion ahead of an earlier one, scan
    // from the start of the doc as a fallback so we don't drop a
    // valid suggestion just because of out-of-order emission.
    let from = findFrom(docText, original, cursor)
    if (from === -1) from = findFrom(docText, original, 0)
    if (from === -1) continue // hallucinated — drop

    const to = from + original.length
    seen.add(dedupeKey)
    out.push({
      id: `${requestId}-${out.length}`,
      kind,
      rangeFrom: from,
      rangeTo: to,
      original,
      replacement,
      message,
      line: lineOfOffset(docText, from),
      generatedFor: generation,
    })
    cursor = to

    if (out.length >= MAX_SUGGESTIONS_PER_PASS) break
  }

  return out
}

interface StreamChatResult {
  session_id?: string
  text?: string
}

/** Module-scoped generation counter. Bumped per `requestPass` so
 *  the CM extension can compare against `Suggestion.generatedFor`
 *  to detect drift. Reset by `_resetForTests`. */
let generationCounter = 0

/**
 * Run one suggestion pass over `docText`, scoped to `docPath`.
 *
 * Single-flight: callers don't need to gate concurrent invocations.
 * If a pass is already in flight, this one supersedes it — the
 * store's staleness guard drops the older result when it eventually
 * resolves. Callers that want debouncing should wrap with a timer.
 *
 * Resolves to the parsed suggestion list (post-validation), or `[]`
 * on transport / parse error. Errors are also written to
 * `useMarginSuggestStore.lastError` so the UI can surface them; the
 * promise itself doesn't reject because the engine is fire-and-
 * forget for the typical "background pass on idle" call site.
 */
export async function requestPass(
  api: PluginAPI,
  docPath: string,
  docText: string,
): Promise<Suggestion[]> {
  generationCounter += 1
  const generation = generationCounter
  const requestId = newRequestId()

  useMarginSuggestStore.getState().beginPass(requestId, docPath, generation)

  const prompt = buildSuggestionPrompt(docText)

  let result: StreamChatResult
  try {
    result = await api.kernel.invoke<StreamChatResult>(
      AI_PLUGIN_ID,
      HANDLER_STREAM_CHAT,
      {
        messages: [{ role: 'user', content: prompt }],
        session_id: requestId,
      },
      MARGIN_PASS_TIMEOUT_MS,
    )
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err))
    useMarginSuggestStore.getState().setError(requestId, error)
    return []
  }

  const text = typeof result?.text === 'string' ? result.text : ''
  const suggestions = parseSuggestionsResponse(
    text,
    docText,
    generation,
    requestId,
  )
  useMarginSuggestStore.getState().setSuggestions(requestId, suggestions)
  return suggestions
}

/** Test-only — wipe the generation counter so tests can assert on
 *  exact generation values. Production code never needs this. */
export function _resetForTests(): void {
  generationCounter = 0
}
