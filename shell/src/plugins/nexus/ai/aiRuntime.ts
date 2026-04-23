// shell/src/plugins/nexus/ai/aiRuntime.ts
//
// WI-01 Slice B — kernel-bridge plumbing for the chat plugin.
// Held out of React so the focus command (and activate-time
// hydration) work independently of the view's mount lifecycle.
//
// Contract with the AI core plugin (`crates/nexus-ai/src/core_plugin.rs`):
//
//   - `com.nexus.ai::config`      → returns AiConfig snapshot (sync)
//   - `com.nexus.ai::stream_ask`  → RAG retrieve + streaming chat
//       args:    { messages: [{role,content}], session_id, limit? }
//       returns: { session_id, text, sources: ChunkMatch[] }
//
// Events published by the kernel (forwarded to JS via api.kernel.on):
//
//   - com.nexus.ai.stream_start  { session_id, sources }
//   - com.nexus.ai.stream_chunk  { session_id, chunk, index }
//   - com.nexus.ai.stream_done   { session_id, text, sources }
//
// `session_id` is the kernel's term for our request_id; treated as a
// single correlation key by the store.
//
// Cancel semantics (Slice B unchanged from A): no kernel-side abort.
// `cancelInFlight()` flips the assistant turn to 'done' (preserving
// streamedText as finalText) and drops the request_id correlation;
// further chunks bounce off the matching-turn guard in the store.

import type { KernelAPI, PluginAPI } from '../../../types/plugin'
import { useAiStore, type AiConfig, type AiSource } from './aiStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_CONFIG = 'config'
const HANDLER_STREAM_ASK = 'stream_ask'

const TOPIC_PREFIX = 'com.nexus.ai.stream_'
const TOPIC_CHUNK = 'com.nexus.ai.stream_chunk'
const TOPIC_DONE = 'com.nexus.ai.stream_done'
// `stream_start` carries `sources` too — Slice B could pre-attach them
// to the assistant turn so the chips render before any tokens arrive.
// Skipped for now: the `stream_done` payload also carries them, and
// rendering chips beside a still-empty bubble feels jarring.

/** Top-k RAG sources fetched per question. Match nexus-ai's default. */
const DEFAULT_LIMIT = 5

/** Wall-clock budget for a single Q&A. Closes legacy ChatPanel.tsx
 *  reference §6 "no client-side timeout, no abort button" gap. */
const REQUEST_TIMEOUT_MS = 60_000

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI): void {
  kernel = api
}

// ── Focus plumbing ────────────────────────────────────────────────────────

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

/** Mirrors `ChunkMatch` in `crates/nexus-ai/src/vectorstore.rs`. */
interface RawChunkMatch {
  file_path?: string
  block_id?: number
  chunk_text?: string
  score?: number
}

interface StreamDoneEvent {
  session_id?: string
  text?: string
  sources?: RawChunkMatch[]
}

interface StreamAskResult {
  session_id?: string
  text?: string
  sources?: RawChunkMatch[]
}

/** Coerce a raw `ChunkMatch` payload from the kernel into the store's
 *  `AiSource`. Drops entries without a usable `file_path` — those are
 *  unrenderable as chips. */
function coerceSources(raw: unknown): AiSource[] {
  if (!Array.isArray(raw)) return []
  const out: AiSource[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as RawChunkMatch
    if (typeof r.file_path !== 'string' || r.file_path.length === 0) continue
    out.push({
      path: r.file_path,
      excerpt: typeof r.chunk_text === 'string' ? r.chunk_text : undefined,
      score: typeof r.score === 'number' ? r.score : undefined,
      blockId: typeof r.block_id === 'number' ? r.block_id : undefined,
    })
  }
  return out
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
      const sources = coerceSources((payload as StreamDoneEvent).sources)
      if (watchdogRequestId === sessionId) clearWatchdog()
      store.finishStream(sessionId, text, sources)
      return
    }

    // stream_start: no-op for Slice B (see TOPIC_START comment above).
  })
}

// ── Submit ────────────────────────────────────────────────────────────────

function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

/** Build the `messages` array for `stream_ask`. Slice B sends the full
 *  user/assistant transcript so the model has conversational context.
 *  In-flight assistant turns are excluded (no point sending an empty
 *  bubble back to the LLM). */
function buildMessageHistory(): Array<{ role: 'user' | 'assistant'; content: string }> {
  const turns = useAiStore.getState().turns
  const out: Array<{ role: 'user' | 'assistant'; content: string }> = []
  for (const t of turns) {
    if (t.kind === 'user') {
      out.push({ role: 'user', content: t.question })
      continue
    }
    // Assistant: only include if we actually have body text.
    const body = t.finalText ?? t.streamedText
    if (t.status === 'streaming' || !body) continue
    out.push({ role: 'assistant', content: body })
  }
  return out
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
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] hydrateConfig failed', err)
  }
}

/**
 * Send a question via `stream_ask`. The streaming response is
 * delivered through the event subscription; this function only
 * initiates the request and tracks the lifetime (timeouts, errors).
 *
 * Slice B: the user turn + assistant turn are appended by `startAsk`
 * before invoke fires, so chunks arriving before invoke resolves
 * always have a turn to land into.
 *
 * The full conversation transcript is sent in `messages` (legacy
 * ChatPanel.tsx:540 pattern) — this is what gives us multi-turn
 * coherence on the model side, not just on the UI.
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
  // Append the user turn FIRST so buildMessageHistory below picks it up.
  state.startAsk(requestId, trimmed)
  armWatchdog(requestId)

  try {
    if (!kernel) throw new Error('AI plugin not activated (kernel handle missing)')

    const messages = buildMessageHistory()

    const result = await api.kernel.invoke<StreamAskResult>(
      AI_PLUGIN_ID,
      HANDLER_STREAM_ASK,
      {
        messages,
        session_id: requestId,
        limit: DEFAULT_LIMIT,
      },
      REQUEST_TIMEOUT_MS,
    )
    // The invoke promise resolves on the same path that fires
    // stream_done. The event handler usually populated the turn
    // already; if it didn't (e.g. the event was dropped), reconcile
    // here from the invoke result so the user still sees the answer.
    const stillStreaming =
      useAiStore.getState().turns.find(
        (t) => t.kind === 'assistant' && t.requestId === requestId && t.status === 'streaming',
      )
    if (stillStreaming && result && typeof result === 'object') {
      const text = typeof result.text === 'string' ? result.text : ''
      const sources = coerceSources(result.sources)
      if (watchdogRequestId === requestId) clearWatchdog()
      useAiStore.getState().finishStream(requestId, text, sources)
    }
  } catch (err) {
    const cur = useAiStore.getState().currentRequestId
    if (cur !== requestId && cur !== null) return
    if (watchdogRequestId === requestId) clearWatchdog()
    if (err instanceof Error) {
      useAiStore.getState().setError(err)
    } else {
      useAiStore.getState().setError(new Error(String(err)))
    }
  }
}

/**
 * Shell-side cancel. The store flips the in-flight assistant turn to
 * 'done' (preserving streamedText as finalText) and drops the
 * request_id correlation; further chunks bounce off the matching-turn
 * guard. Does NOT cancel the kernel-side stream.
 */
export function cancelInFlight(): void {
  clearWatchdog()
  useAiStore.getState().cancel()
}

/**
 * Retry the most recent user question. Used by the error banner's
 * Retry button. With Slice B, "the last question" is the most recent
 * user turn — there's no longer a flat `lastQuestion` field.
 */
export async function retryLast(api: PluginAPI): Promise<void> {
  const turns = useAiStore.getState().turns
  for (let i = turns.length - 1; i >= 0; i -= 1) {
    const t = turns[i]
    if (t.kind === 'user') {
      await submitQuestion(api, t.question)
      return
    }
  }
}
