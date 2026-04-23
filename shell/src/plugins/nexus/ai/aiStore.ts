// shell/src/plugins/nexus/ai/aiStore.ts
//
// WI-01 Slice B — multi-turn conversation store with RAG sources.
//
// Slice A held a single Q/A pair (`question`, `streamedAnswer`,
// `finalAnswer`). Slice B replaces those with an ordered `turns`
// array that interleaves user prompts and assistant replies. Each
// `submitQuestion` appends BOTH a user turn and a streaming assistant
// turn (eagerly created so chunks have somewhere to land).
//
// Lifecycle of a single request:
//
//   submitQuestion(req-N) -->
//     turns: [..., {kind:'user'}, {kind:'assistant', status:'streaming'}]
//     status: 'asking'
//   first chunk for req-N -->
//     assistant.streamedText grows
//     status: 'streaming'
//   stream_done for req-N -->
//     assistant.finalText = stream_done.text (overrides chunks)
//     assistant.sources = stream_done.sources ?? []
//     status: 'idle'
//   error -->
//     assistant.status = 'error', assistant.error = err
//     status: 'error'
//   cancel -->
//     assistant.status = 'done', assistant.finalText = streamedText
//       (preserve the partial — keeps the conversation coherent)
//     status: 'idle'
//
// Chunks/done events for unknown request_ids are dropped silently —
// the same staleness guard from Slice A. The lookup is now "find the
// assistant turn whose requestId matches", so concurrent in-flight
// requests would each route to their own turn (the runtime still
// gates single-flight at submit time, but the store is robust to it).
//
// `clearTurns` wipes the conversation but does NOT touch `config` or
// any in-flight stream — those are orthogonal concerns.

import { create } from 'zustand'

/** Snapshot returned by `com.nexus.ai::config`. Mirrors `config_snapshot` in
 *  `crates/nexus-ai/src/core_plugin.rs` (`ConfigView`). */
export interface AiProviderView {
  provider: string
  model: string | null
  base_url: string | null
  has_api_key: boolean
}

export interface AiConfig {
  ai: AiProviderView | null
  embedding: AiProviderView | null
}

/** A single retrieved RAG chunk surfaced beside an assistant turn.
 *  Mirrors `ChunkMatch` in `crates/nexus-ai/src/vectorstore.rs`. */
export interface AiSource {
  /** Forge-relative path the kernel attributes the chunk to. */
  path: string
  /** Optional excerpt from the chunk text the kernel used as context. */
  excerpt?: string
  /** Cosine-similarity score (higher = more relevant) when surfaced. */
  score?: number
  /** Block id from the source file — useful as a render key. */
  blockId?: number
}

export type AiTurn =
  | {
      kind: 'user'
      id: string
      question: string
      askedAt: number
    }
  | {
      kind: 'assistant'
      id: string
      requestId: string
      status: 'streaming' | 'done' | 'error'
      /** Live chunk buffer, accumulated as `stream_chunk` arrives. */
      streamedText: string
      /** Authoritative final body from `stream_done`. Null until done. */
      finalText: string | null
      sources: AiSource[]
      error: Error | null
    }

export type AiStatus = 'idle' | 'asking' | 'streaming' | 'error'

export interface AiState {
  /** Lifecycle phase of the in-flight request (max one at a time). */
  status: AiStatus
  /** Ordered conversation history. Append-only via store actions. */
  turns: AiTurn[]
  /** Composer text, bound to the textarea. Cleared optimistically on send. */
  question: string
  /** Kernel-side `session_id` of the in-flight request. Null when idle. */
  currentRequestId: string | null
  /** Hydrated once on activate from `com.nexus.ai::config`. */
  config: AiConfig | null

  // ── actions ──────────────────────────────────────────────────────────────
  setQuestion: (q: string) => void
  /** Append a user turn + a streaming assistant turn, set asking. */
  startAsk: (requestId: string, question: string) => void
  /** Route a chunk to the matching assistant turn; drop if mismatched. */
  appendChunk: (requestId: string, text: string) => void
  /** Finalize the matching assistant turn: set finalText + sources, idle. */
  finishStream: (requestId: string, finalText: string, sources?: AiSource[]) => void
  /** Mark the in-flight assistant turn as errored. Always wins. */
  setError: (err: Error) => void
  /** Shell-side cancel: preserve any partial streamedText as finalText. */
  cancel: () => void
  setConfig: (c: AiConfig) => void
  /** Wipe the conversation. Leaves config + in-flight stream alone. */
  clearTurns: () => void
  /** Wipe everything except the hydrated config. Used by workspace close. */
  reset: () => void
}

const INITIAL: Omit<
  AiState,
  | 'setQuestion'
  | 'startAsk'
  | 'appendChunk'
  | 'finishStream'
  | 'setError'
  | 'cancel'
  | 'setConfig'
  | 'clearTurns'
  | 'reset'
> = {
  status: 'idle',
  turns: [],
  question: '',
  currentRequestId: null,
  config: null,
}

/** Locate the in-flight assistant turn for a given requestId. Returns
 *  the index, or -1 if no matching streaming turn exists (chunk is
 *  stale — request was cancelled, errored, or never started). */
function findStreamingAssistantIdx(turns: AiTurn[], requestId: string): number {
  for (let i = turns.length - 1; i >= 0; i -= 1) {
    const t = turns[i]
    if (t.kind === 'assistant' && t.requestId === requestId && t.status === 'streaming') {
      return i
    }
  }
  return -1
}

/** Newest assistant turn with status === 'streaming', regardless of
 *  requestId. Used by cancel/setError when we don't carry the id. */
function findCurrentStreamingIdx(turns: AiTurn[]): number {
  for (let i = turns.length - 1; i >= 0; i -= 1) {
    const t = turns[i]
    if (t.kind === 'assistant' && t.status === 'streaming') return i
  }
  return -1
}

/** Cheap unique id for a turn. Doesn't need to be cryptographic — it's
 *  only a React render key. */
let turnSeq = 0
function newTurnId(prefix: string): string {
  turnSeq += 1
  return `${prefix}-${Date.now().toString(36)}-${turnSeq.toString(36)}`
}

export const useAiStore = create<AiState>((set, get) => ({
  ...INITIAL,

  setQuestion: (q) => set({ question: q }),

  startAsk: (requestId, question) => {
    const userTurn: AiTurn = {
      kind: 'user',
      id: newTurnId('user'),
      question,
      askedAt: Date.now(),
    }
    const assistantTurn: AiTurn = {
      kind: 'assistant',
      id: newTurnId('asst'),
      requestId,
      status: 'streaming',
      streamedText: '',
      finalText: null,
      sources: [],
      error: null,
    }
    set((s) => ({
      status: 'asking',
      currentRequestId: requestId,
      // Optimistic clear — legacy ChatPanel.tsx:472. Composer empties
      // immediately so the user can type their next question without
      // waiting for the round-trip.
      question: '',
      turns: [...s.turns, userTurn, assistantTurn],
    }))
  },

  appendChunk: (requestId, text) => {
    const state = get()
    const idx = findStreamingAssistantIdx(state.turns, requestId)
    if (idx === -1) return // stale / unknown request — drop

    const turns = state.turns.slice()
    const target = turns[idx]
    if (target.kind !== 'assistant') return // type guard
    turns[idx] = {
      ...target,
      streamedText: target.streamedText + text,
    }
    set({
      status: state.currentRequestId === requestId ? 'streaming' : state.status,
      turns,
    })
  },

  finishStream: (requestId, finalText, sources) => {
    const state = get()
    const idx = findStreamingAssistantIdx(state.turns, requestId)
    if (idx === -1) return // stale / unknown request — drop

    const turns = state.turns.slice()
    const target = turns[idx]
    if (target.kind !== 'assistant') return // type guard
    turns[idx] = {
      ...target,
      status: 'done',
      finalText,
      sources: sources ?? target.sources,
      streamedText: '', // cleared so render falls through to finalText
    }
    set({
      status: state.currentRequestId === requestId ? 'idle' : state.status,
      currentRequestId: state.currentRequestId === requestId ? null : state.currentRequestId,
      turns,
    })
  },

  setError: (err) => {
    const state = get()
    const idx = findCurrentStreamingIdx(state.turns)
    if (idx === -1) {
      // No in-flight assistant turn (rare — error fired with nothing
      // streaming). Just flip global status; the UI shows a banner.
      set({ status: 'error', currentRequestId: null })
      return
    }
    const turns = state.turns.slice()
    const target = turns[idx]
    if (target.kind !== 'assistant') return
    turns[idx] = {
      ...target,
      status: 'error',
      error: err,
      // Preserve whatever partial text we got so the user can see
      // where the kernel cut out.
      finalText: target.streamedText.length > 0 ? target.streamedText : target.finalText,
      streamedText: '',
    }
    set({
      status: 'error',
      currentRequestId: null,
      turns,
    })
  },

  cancel: () => {
    const state = get()
    const idx = findCurrentStreamingIdx(state.turns)
    if (idx === -1) {
      set({ status: 'idle', currentRequestId: null })
      return
    }
    const turns = state.turns.slice()
    const target = turns[idx]
    if (target.kind !== 'assistant') return
    turns[idx] = {
      ...target,
      status: 'done',
      // Preserve the partial. If no text was ever streamed, finalText
      // stays null and the view can render an "[cancelled]" affordance.
      finalText: target.streamedText.length > 0 ? target.streamedText : target.finalText,
      streamedText: '',
    }
    set({
      status: 'idle',
      currentRequestId: null,
      turns,
    })
  },

  setConfig: (c) => set({ config: c }),

  clearTurns: () =>
    // Deliberate: don't clear `config`, don't touch `currentRequestId`
    // or `status`. An in-flight stream stays in flight; its assistant
    // turn was just removed from `turns`, so chunks/done events will
    // bounce off the missing-turn guard in `appendChunk` /
    // `finishStream`. That's the right behaviour — clearing chat
    // shouldn't clobber the stream contract.
    set({ turns: [] }),

  reset: () =>
    set((s) => ({
      ...INITIAL,
      // Keep the hydrated config — it's plugin-lifetime state, not
      // request-lifetime state.
      config: s.config,
    })),
}))
