// shell/src/plugins/nexus/ai/aiStore.ts
//
// WI-01 Slice B — multi-turn conversation store with RAG sources.
// WI-01 Slice C — session management (save/load/list/delete/rename).
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

/** BL-038 — numbered, line-aware citation surfaced beside an assistant
 *  turn. Mirrors `Citation` in `crates/nexus-ai/src/rag.rs`. The shell
 *  prefers `turn.citations` when populated; otherwise falls back to
 *  `turn.sources` so older backends still render chips. */
export interface AiCitation {
  /** 1-based index. Matches `[N]` markers in the answer text when the
   *  model emitted them; otherwise source order (descending by score). */
  index: number
  /** Forge-relative path of the source file. */
  path: string
  /** Block id from the source file. */
  blockId: number
  /** 1-based start line. Null when storage couldn't resolve the block. */
  startLine: number | null
  /** 1-based end line. Null when `startLine` is. */
  endLine: number | null
  /** Truncated chunk text (≤ 200 chars on the kernel side). */
  excerpt: string
  /** Cosine similarity score. */
  score: number
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
      /** BL-038 — numbered citations parallel to `sources`. Empty when
       *  the kernel didn't ship them (older backend) or RAG returned
       *  nothing. */
      citations: AiCitation[]
      error: Error | null
    }

export type AiStatus = 'idle' | 'asking' | 'streaming' | 'error'

/** Lightweight session metadata returned by `com.nexus.ai::session_list`.
 *  Mirrors the kernel's payload (`crates/nexus-ai/src/core_plugin.rs`
 *  `handle_session_list`) — note the kernel returns `{ id, title?,
 *  updated_at?, bytes }` only. There's no `created_at` or `turn_count`
 *  on the wire, so we synthesize neither here. */
export interface AiSessionMeta {
  /** Stable, validated session id. Used as the kernel filename stem. */
  id: string
  /** Display title — auto-derived from the first user turn, or
   *  user-renamed via inline edit. May be empty/null on disk. */
  title: string
  /** ISO timestamp from the most recent save; null if the kernel
   *  didn't surface one (defensive — older sessions may lack it). */
  updatedAt: string | null
  /** Encoded byte size on disk. Surfaced for ops display only. */
  bytes: number
}

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

  /** Saved sessions enumerated via `session_list`. Refreshed after save
   *  / delete; not on every turn change (legacy was chatty — see
   *  `docs/wi01-chatpanel-reference.md` §5). */
  sessions: AiSessionMeta[]
  /** Id of the currently-loaded session. `null` means a fresh, unsaved
   *  conversation; the next `saveCurrentSession` call mints an id. */
  activeSessionId: string | null
  /** True while `session_list` is in flight. The picker shows a
   *  skeleton/spinner during initial hydration. */
  sessionsLoading: boolean

  // ── actions ──────────────────────────────────────────────────────────────
  setQuestion: (q: string) => void
  /** Append a user turn + a streaming assistant turn, set asking. */
  startAsk: (requestId: string, question: string) => void
  /** Route a chunk to the matching assistant turn; drop if mismatched. */
  appendChunk: (requestId: string, text: string) => void
  /** Finalize the matching assistant turn: set finalText + sources +
   *  citations, idle. BL-038 added the optional `citations` argument
   *  for numbered, line-aware sources. */
  finishStream: (
    requestId: string,
    finalText: string,
    sources?: AiSource[],
    citations?: AiCitation[],
  ) => void
  /** Mark the in-flight assistant turn as errored. Always wins. */
  setError: (err: Error) => void
  /** Shell-side cancel: preserve any partial streamedText as finalText. */
  cancel: () => void
  setConfig: (c: AiConfig) => void
  /** Wipe the conversation. Leaves config + in-flight stream alone. */
  clearTurns: () => void
  /** Wipe everything except the hydrated config. Used by workspace close. */
  reset: () => void

  // ── session-management actions (Slice C) ────────────────────────────────
  /** Replace the in-memory session list. Fed by aiRuntime after a
   *  successful `session_list` round-trip. */
  setSessions: (sessions: AiSessionMeta[]) => void
  /** Toggle the loading flag — used by the runtime around `session_list`. */
  setSessionsLoading: (loading: boolean) => void
  /** Set the active session id without otherwise mutating turns.
   *  Used by `loadSession` after the kernel returns and `newSession`
   *  to reset to "unsaved". */
  setActiveSessionId: (id: string | null) => void
  /** Replace `turns` wholesale with a hydrated session payload. The
   *  runtime uses this when the user picks a session from the list.
   *  Status/in-flight state is left alone — the caller is expected to
   *  cancel/auto-save first (see `aiRuntime.loadSession`). */
  hydrateTurns: (turns: AiTurn[]) => void
  /** Local-only "new chat": clear turns + active id. No kernel call.
   *  Auto-save lives in the runtime; the next save mints a fresh id.
   *  Does NOT cancel an in-flight stream — the runtime handles that
   *  before calling this so the partial can be auto-saved first. */
  newSession: () => void
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
  | 'setSessions'
  | 'setSessionsLoading'
  | 'setActiveSessionId'
  | 'hydrateTurns'
  | 'newSession'
> = {
  status: 'idle',
  turns: [],
  question: '',
  currentRequestId: null,
  config: null,
  sessions: [],
  activeSessionId: null,
  sessionsLoading: false,
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
      citations: [],
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

  finishStream: (requestId, finalText, sources, citations) => {
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
      citations: citations ?? target.citations,
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

  // ── session-management actions (Slice C) ────────────────────────────────

  setSessions: (sessions) => set({ sessions }),

  setSessionsLoading: (loading) => set({ sessionsLoading: loading }),

  setActiveSessionId: (id) => set({ activeSessionId: id }),

  hydrateTurns: (turns) =>
    // Wholesale replace. We deliberately leave `status` /
    // `currentRequestId` alone: the runtime is expected to cancel any
    // in-flight stream BEFORE calling hydrate (otherwise loaded turns
    // would get clobbered by a stale chunk landing afterward).
    set({ turns }),

  newSession: () =>
    // Clear conversation + drop the active id so the next save mints
    // a fresh one. Doesn't touch the in-flight stream — the runtime
    // wraps this with `cancelInFlight()` and an auto-save of any
    // unflushed turns when invoked from "New chat".
    set({ turns: [], activeSessionId: null }),
}))
