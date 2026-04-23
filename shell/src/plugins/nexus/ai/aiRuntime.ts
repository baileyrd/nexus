// shell/src/plugins/nexus/ai/aiRuntime.ts
//
// WI-01 Slice A — kernel-bridge plumbing for the chat plugin.
// Held out of React so the focus command (and future activate-time
// hydration) work independently of the view's mount lifecycle.
//
// Contract with the AI core plugin (`crates/nexus-ai/src/core_plugin.rs`):
//
//   - `com.nexus.ai::config`      → returns AiConfig snapshot (sync)
//   - `com.nexus.ai::stream_ask`  → RAG retrieve + streaming chat
//       args:    { messages: [{role,content}], session_id, limit? }
//       returns: { session_id, text, sources }   (final consolidated)
//
// Events published by the kernel (forwarded to JS via api.kernel.on):
//
//   - com.nexus.ai.stream_start  { session_id, sources }
//   - com.nexus.ai.stream_chunk  { session_id, chunk, index }
//   - com.nexus.ai.stream_done   { session_id, text, sources }
//
// Note: `session_id` is what the kernel calls our request_id.
// We treat them as the same correlation key; the store uses
// `currentRequestId` semantically.
//
// Cancel-on-shell semantics (Slice A): there's no kernel-side abort
// API yet. `cancelInFlight()` clears `currentRequestId` so subsequent
// chunks bounce off the mismatch check in the store. The kernel may
// keep producing chunks + a final done — they're dropped on arrival.
// A future kernel handler (`stream_cancel { session_id }`?) can layer
// real cancellation on top without changing this shell contract.

import type { KernelAPI, PluginAPI } from '../../../types/plugin'
import { useAiStore, type AiConfig } from './aiStore'

// `api.kernel.invoke` rejects with a `KernelIpcError` (extends Error)
// or a raw value if the bridge ever returns a non-envelope shape. We
// don't import the class — WI-23 forbids plugins reaching into
// shell/src/host. We treat `Error` as the common supertype and let
// the view duck-type on `err.name === 'KernelIpcError'` if it ever
// needs to branch on `kind`.

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_CONFIG = 'config'
const HANDLER_STREAM_ASK = 'stream_ask'

const TOPIC_PREFIX = 'com.nexus.ai.stream_'
const TOPIC_CHUNK = 'com.nexus.ai.stream_chunk'
const TOPIC_DONE = 'com.nexus.ai.stream_done'
// stream_start exists on the wire but Slice A has no use for it (no
// pre-allocated assistant turn to flip into "streaming"). Slice B
// will need it for the source-chip pre-render.

/** Top-k RAG sources fetched per question. Match nexus-ai's default. */
const DEFAULT_LIMIT = 5

/** Wall-clock budget for a single Q&A. Closes legacy ChatPanel.tsx
 *  reference §6 "no client-side timeout, no abort button" gap. */
const REQUEST_TIMEOUT_MS = 60_000

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI): void {
  kernel = api
}

// ── Focus plumbing (preserved from the prior skeleton) ────────────────────
// Held module-side so the `nexus.ai.focus` command can poke the textarea
// even when ChatView isn't currently mounted (the focuser registers on
// mount and unregisters on unmount; pendingFocus drains on next mount).

type Focuser = () => void
let focuser: Focuser | null = null
let pendingFocus = false

export function registerFocuser(fn: Focuser | null): void {
  focuser = fn
  if (fn && pendingFocus) {
    pendingFocus = false
    fn()
  }
}

export function requestFocus(): void {
  if (focuser) {
    focuser()
  } else {
    pendingFocus = true
  }
}

// ── Watchdog ──────────────────────────────────────────────────────────────
//
// One in-flight request at a time. When `submitQuestion` starts, we
// arm a 60s timer keyed off the request id; on stream_done / error /
// cancel we clear it. If it fires, we synthesize an error in the
// store and clear `currentRequestId` so any late kernel events are
// dropped by the store's request_id check.

let watchdogTimer: ReturnType<typeof setTimeout> | null = null
let watchdogRequestId: string | null = null

function armWatchdog(requestId: string): void {
  clearWatchdog()
  watchdogRequestId = requestId
  watchdogTimer = setTimeout(() => {
    const state = useAiStore.getState()
    if (state.currentRequestId !== requestId) return
    state.setError(
      new Error(
        `AI request timed out after ${REQUEST_TIMEOUT_MS / 1000}s with no stream_done`,
      ),
    )
    watchdogTimer = null
    watchdogRequestId = null
  }, REQUEST_TIMEOUT_MS)
}

function clearWatchdog(): void {
  if (watchdogTimer) clearTimeout(watchdogTimer)
  watchdogTimer = null
  watchdogRequestId = null
}

// ── Stream subscription ───────────────────────────────────────────────────

interface StreamChunkEvent {
  session_id?: string
  chunk?: string
  index?: number
}

interface StreamDoneEvent {
  session_id?: string
  text?: string
}

/**
 * Subscribe to all `com.nexus.ai.stream_*` topics under one prefix
 * subscription. Returns the disposer; PluginRegistry sweeps it on
 * plugin unload (commit c4d31d3) so the caller doesn't need to track
 * it. We still return it for symmetry / explicit teardown in tests.
 */
export async function subscribeStream(api: PluginAPI): Promise<() => void> {
  return api.kernel.on<StreamChunkEvent | StreamDoneEvent>(TOPIC_PREFIX, (topic, payload) => {
    if (!payload || typeof payload !== 'object') return
    const sessionId = (payload as { session_id?: unknown }).session_id
    if (typeof sessionId !== 'string') return
    const store = useAiStore.getState()

    if (topic === TOPIC_CHUNK) {
      const chunk = (payload as StreamChunkEvent).chunk
      if (typeof chunk !== 'string' || chunk.length === 0) return
      store.appendChunk(sessionId, chunk)
      return
    }

    if (topic === TOPIC_DONE) {
      const text = (payload as StreamDoneEvent).text ?? ''
      if (watchdogRequestId === sessionId) clearWatchdog()
      store.finishStream(sessionId, text)
      return
    }

    // stream_start: no-op for Slice A.
  })
}

// ── Submit ────────────────────────────────────────────────────────────────

/** crypto.randomUUID is global in Node18+/modern browsers. Fallback
 *  exists for the rare CI shape that lacks it. */
function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

/**
 * Hydrate the AiConfig snapshot from the kernel. Called once on
 * plugin activate; the result is cached in the store so the view can
 * render provider/model labels without an extra round-trip.
 */
export async function hydrateConfig(api: PluginAPI): Promise<void> {
  try {
    const cfg = await api.kernel.invoke<AiConfig>(AI_PLUGIN_ID, HANDLER_CONFIG, {})
    useAiStore.getState().setConfig(cfg)
  } catch (err) {
    // Non-fatal — chat still works without the snapshot, the user
    // just won't see the model label until next activate. Log so the
    // failure isn't silent in the dev console.
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] hydrateConfig failed', err)
  }
}

/**
 * Send a question via `stream_ask`. The streaming response is
 * delivered through the event subscription set up by
 * `subscribeStream`; this function only initiates the request and
 * tracks the lifetime (timeouts, errors).
 *
 * Pre-conditions: kernel handle wired (setKernel called on activate),
 * subscribeStream already running (so the answer doesn't go to
 * /dev/null).
 */
export async function submitQuestion(
  api: PluginAPI,
  question: string,
): Promise<void> {
  const trimmed = question.trim()
  if (!trimmed) return

  const state = useAiStore.getState()
  if (state.status === 'asking' || state.status === 'streaming') {
    // Single-flight: ignore double-submits.
    return
  }

  const requestId = newRequestId()
  state.startAsk(requestId, trimmed)
  armWatchdog(requestId)

  try {
    if (!kernel) throw new Error('AI plugin not activated (kernel handle missing)')

    // The kernel expects a `messages` array, not a flat `question` —
    // it picks the last user message inside `handle_stream_ask`. For
    // Slice A we send a single user turn; Slice B will append prior
    // turns from the conversation model.
    await api.kernel.invoke(
      AI_PLUGIN_ID,
      HANDLER_STREAM_ASK,
      {
        messages: [{ role: 'user', content: trimmed }],
        session_id: requestId,
        limit: DEFAULT_LIMIT,
      },
      REQUEST_TIMEOUT_MS,
    )
    // The invoke promise resolves on the same path that fires
    // stream_done; the event handler has already populated
    // finalAnswer. Nothing to do here.
  } catch (err) {
    // Only surface the error if we're still the in-flight request.
    // If the user clicked Stop or a stale rejection arrives after a
    // new submit, the store has already moved on and we shouldn't
    // clobber it.
    const cur = useAiStore.getState().currentRequestId
    if (cur !== requestId && cur !== null) return
    if (watchdogRequestId === requestId) clearWatchdog()
    // KernelIpcError extends Error, so this catches both typed and
    // raw rejections. Non-Error rejections get wrapped so the store
    // always sees an Error subclass.
    if (err instanceof Error) {
      useAiStore.getState().setError(err)
    } else {
      useAiStore.getState().setError(new Error(String(err)))
    }
  }
}

/**
 * Shell-side cancel. Drops the request_id correlation so any further
 * chunks/done events from the kernel are ignored. Does NOT cancel
 * the kernel-side stream — that needs a future `stream_cancel`
 * handler. Visible effect: streaming buffer clears, status idles,
 * Stop button disappears.
 */
export function cancelInFlight(): void {
  clearWatchdog()
  useAiStore.getState().cancel()
}

/**
 * Retry the last submitted question. Used by the error banner's
 * Retry button.
 */
export async function retryLast(api: PluginAPI): Promise<void> {
  const last = useAiStore.getState().lastQuestion
  if (!last) return
  await submitQuestion(api, last)
}
