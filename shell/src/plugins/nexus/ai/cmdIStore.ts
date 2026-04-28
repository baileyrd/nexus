// shell/src/plugins/nexus/ai/cmdIStore.ts
//
// BL-032 — transient state for the Cmd+I overlay.
//
// Mirrors the pattern in `commandPaletteStore.ts`: only UI-state lives
// here; the conversation/streaming machinery is owned by `aiStore` and
// reached via the runtime.
//
// One-shot lifecycle per activation:
//
//   open()                      → visible: true, prompt cleared, chips
//                                 placeholder (rehydrated by runtime).
//   setPrompt(...)              → composer text.
//   beginSubmit(requestId)      → status: 'submitting', clears response.
//   appendResponseChunk(...)    → status: 'streaming', growing body.
//   finishResponse(text)        → status: 'done', final body.
//   setError(err)               → status: 'error'.
//   close()                     → visible: false, snapshot retained so
//                                 the next open starts in 'idle' but
//                                 doesn't flash the previous answer.
//                                 Also wipes the in-flight requestId
//                                 so a tail chunk can't reopen the
//                                 closed overlay.

import { create } from 'zustand'
import type { ContextChip } from './contextContributors'

export type CmdIStatus =
  | 'idle'
  | 'collecting'
  | 'submitting'
  | 'streaming'
  | 'done'
  | 'error'

export interface CmdIState {
  visible: boolean
  /** Free-form prompt text bound to the overlay's input. */
  prompt: string
  /** Chips assembled from registered contributors at activation. */
  chips: ContextChip[]
  /** Lifecycle of the in-flight (or last-completed) request. */
  status: CmdIStatus
  /** Streaming body — appended to as `stream_chunk` arrives. */
  responseText: string
  /** Sticky error from the last submission. Cleared on next submit. */
  error: Error | null
  /** Correlation id for the active stream. Cleared on done/error/close. */
  currentRequestId: string | null

  open(): void
  close(): void
  setPrompt(p: string): void
  setChips(chips: ContextChip[]): void

  beginSubmit(requestId: string): void
  appendResponseChunk(requestId: string, chunk: string): void
  finishResponse(requestId: string, finalText: string): void
  setError(err: Error): void
}

const INITIAL: Pick<
  CmdIState,
  | 'visible'
  | 'prompt'
  | 'chips'
  | 'status'
  | 'responseText'
  | 'error'
  | 'currentRequestId'
> = {
  visible: false,
  prompt: '',
  chips: [],
  status: 'idle',
  responseText: '',
  error: null,
  currentRequestId: null,
}

export const useCmdIStore = create<CmdIState>((set, get) => ({
  ...INITIAL,

  open: () =>
    // Clear everything on each activation so chips/response from a
    // prior summon don't bleed into a new one. Status is 'collecting'
    // until the runtime hydrates the chips.
    set({
      visible: true,
      prompt: '',
      chips: [],
      status: 'collecting',
      responseText: '',
      error: null,
      currentRequestId: null,
    }),

  close: () =>
    // Drop currentRequestId so a tail chunk can't append into a closed
    // overlay (the matching guard in appendResponseChunk would reject
    // it anyway, but we'd still flicker the streaming status).
    set({
      visible: false,
      currentRequestId: null,
    }),

  setPrompt: (p) => set({ prompt: p }),
  setChips: (chips) =>
    set((s) => ({
      chips,
      // Once chips are in, the user can type — flip out of 'collecting'
      // unless we've already moved on to a stream.
      status: s.status === 'collecting' ? 'idle' : s.status,
    })),

  beginSubmit: (requestId) =>
    set({
      status: 'submitting',
      currentRequestId: requestId,
      responseText: '',
      error: null,
    }),

  appendResponseChunk: (requestId, chunk) => {
    const state = get()
    if (state.currentRequestId !== requestId) return // stale
    set({
      status: 'streaming',
      responseText: state.responseText + chunk,
    })
  },

  finishResponse: (requestId, finalText) => {
    const state = get()
    if (state.currentRequestId !== requestId) return // stale
    set({
      status: 'done',
      // `stream_done.text` is authoritative — replace any chunk-built
      // body so we don't double up if chunks + done both fire.
      responseText: finalText.length > 0 ? finalText : state.responseText,
      currentRequestId: null,
    })
  },

  setError: (err) =>
    set({
      status: 'error',
      error: err,
      currentRequestId: null,
    }),
}))
