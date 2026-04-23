// shell/src/plugins/nexus/ai/aiStore.ts
//
// WI-01 Slice A — minimal-but-real chat store.
//
// Single in-flight Q&A. The store tracks one request lifecycle:
//
//   idle  --startAsk(reqId)-->  asking
//   asking --first chunk-->     streaming
//   streaming --finishStream--> idle  (finalAnswer set)
//   any --setError-->           error
//   any --cancel-->             idle
//
// Chunks arriving with a request_id that does not match
// `currentRequestId` are dropped silently. This is how we tolerate:
//   - chunks for a previously-cancelled request still arriving from
//     the kernel (no kernel-side cancel API yet — see aiRuntime.ts).
//   - races between a new submit and the tail of a prior stream.
//
// `stream_done.text` is authoritative; it overwrites the streamed
// chunk buffer (legacy ChatPanel.tsx:335). Render `finalAnswer ??
// streamedAnswer` so the user sees live tokens, then the final wins.
//
// Slices B / C will grow this into a turn list + sessions; for Slice
// A we deliberately keep it to one Q + one A so the streaming
// contract is the only thing under test.

import { create } from 'zustand'

// `aiRuntime` will pass either a plain `Error` (timeouts, missing
// kernel handle) or a `KernelIpcError` (typed wrapper around the
// `IpcErrorEnvelope` from @nexus/extension-api) — both extend `Error`.
// We type-erase to `Error` here so the store doesn't have to import
// the host-side wrapper class (WI-23 import-hygiene guardrail). The
// view can `if (err.name === 'KernelIpcError') ...` if it ever needs
// to branch on `kind`.

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

export type AiStatus = 'idle' | 'asking' | 'streaming' | 'error'

export interface AiState {
  /** Lifecycle phase of the single in-flight request. */
  status: AiStatus
  /** Kernel-side `session_id` of the in-flight request. Null when idle.
   *  Incoming events whose `session_id` doesn't match are dropped. */
  currentRequestId: string | null
  /** Composer text, bound to the textarea. Cleared optimistically on send. */
  question: string
  /** The most recent submitted question — used for retry. */
  lastQuestion: string
  /** Live-streaming chunk buffer. Overwritten by `finalAnswer` on done. */
  streamedAnswer: string
  /** Authoritative final response text. Set on `stream_done`. */
  finalAnswer: string | null
  /** Last error from kernel.invoke or watchdog timeout. */
  error: Error | null
  /** Hydrated once on activate from `com.nexus.ai::config`. */
  config: AiConfig | null

  // ── actions ──────────────────────────────────────────────────────────────
  setQuestion: (q: string) => void
  /** Begin a new request; clears prior answer + error. */
  startAsk: (requestId: string, question: string) => void
  /** Append a chunk if requestId matches; otherwise drop. */
  appendChunk: (requestId: string, text: string) => void
  /** Finalize: set finalAnswer, clear streamed buffer, idle. */
  finishStream: (requestId: string, finalText: string) => void
  /** Set error + idle. Always wins, regardless of phase. */
  setError: (err: Error) => void
  /** Cancel the in-flight request from the shell side. Drops any
   *  remaining chunks (kernel may keep producing — we ignore them). */
  cancel: () => void
  setConfig: (c: AiConfig) => void
  /** Wipe everything except the hydrated config. */
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
  | 'reset'
> = {
  status: 'idle',
  currentRequestId: null,
  question: '',
  lastQuestion: '',
  streamedAnswer: '',
  finalAnswer: null,
  error: null,
  config: null,
}

export const useAiStore = create<AiState>((set, get) => ({
  ...INITIAL,

  setQuestion: (q) => set({ question: q }),

  startAsk: (requestId, question) =>
    set({
      status: 'asking',
      currentRequestId: requestId,
      // Optimistic clear — legacy ChatPanel.tsx:472. Composer empties
      // immediately so the user can type their next question without
      // waiting for the round-trip.
      question: '',
      lastQuestion: question,
      streamedAnswer: '',
      finalAnswer: null,
      error: null,
    }),

  appendChunk: (requestId, text) => {
    const state = get()
    if (state.currentRequestId !== requestId) {
      // Stale chunk — request was cancelled or superseded. Drop it.
      return
    }
    set({
      status: 'streaming',
      streamedAnswer: state.streamedAnswer + text,
    })
  },

  finishStream: (requestId, finalText) => {
    const state = get()
    if (state.currentRequestId !== requestId) {
      // Done event for a stale request — ignore.
      return
    }
    set({
      status: 'idle',
      currentRequestId: null,
      streamedAnswer: '',
      finalAnswer: finalText,
    })
  },

  setError: (err) =>
    set({
      status: 'error',
      currentRequestId: null,
      error: err,
    }),

  cancel: () =>
    set({
      status: 'idle',
      currentRequestId: null,
      streamedAnswer: '',
      // Preserve any partial finalAnswer (none yet, but for symmetry).
    }),

  setConfig: (c) => set({ config: c }),

  reset: () =>
    set((s) => ({
      ...INITIAL,
      // Keep the hydrated config — it's plugin-lifetime state, not
      // request-lifetime state.
      config: s.config,
    })),
}))
