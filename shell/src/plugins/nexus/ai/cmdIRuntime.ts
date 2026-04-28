// shell/src/plugins/nexus/ai/cmdIRuntime.ts
//
// BL-032 — submit-side plumbing for the Cmd+I overlay.
//
// Hand-off contract with the rest of the AI plugin:
//
//   open()        — resolve every registered context contributor,
//                   push the resulting chips into the overlay store.
//                   Pure: does NOT call the kernel.
//
//   submit()      — assemble the prompt, fire `com.nexus.ai::stream_ask`
//                   with a fresh session id (so this one-shot doesn't
//                   pollute the persistent chat session), and route
//                   the streaming response into the overlay store.
//
// Reuses the AI plugin's existing `com.nexus.ai.stream_*` subscription
// (set up by `aiRuntime.subscribeStream`) to receive chunks. We don't
// open a second subscription — the unified handler in this file is
// installed by the plugin's activate and dispatches to either the chat
// store or this overlay store based on the `session_id` correlation.

import type { PluginAPI } from '../../../types/plugin'
import {
  contextContributors,
  assemblePrompt,
  type AssembledPrompt,
  type ContextChip,
} from './contextContributors'
import { useCmdIStore } from './cmdIStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
/** `stream_chat` (no RAG retrieval) — citations are out of scope for
 *  BL-032 (BL-038 territory) and the overlay assembles its own context
 *  via `contextContributors`, so embedding-store retrieval would be
 *  redundant work. Mapping verified at
 *  `crates/nexus-bootstrap/src/lib.rs:638` (`"stream_chat"` →
 *  `HANDLER_STREAM_CHAT`). */
const HANDLER_STREAM_CHAT = 'stream_chat'

/** Wall-clock budget — same as the chat surface (aiRuntime.ts:56). A
 *  one-shot overlay is no different from a regular Q&A in that
 *  respect. */
const CMD_I_REQUEST_TIMEOUT_MS = 60_000

/** Correlation prefix lets the unified stream handler tell overlay
 *  requests apart from chat requests on the same subscription. */
const CMD_I_SESSION_PREFIX = 'cmdi-'

/** True iff the given session id was minted by this overlay. The
 *  unified stream router consults this so chat session ids never land
 *  in our store and vice versa. */
export function isCmdISessionId(id: string): boolean {
  return typeof id === 'string' && id.startsWith(CMD_I_SESSION_PREFIX)
}

function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${CMD_I_SESSION_PREFIX}${crypto.randomUUID()}`
  }
  return `${CMD_I_SESSION_PREFIX}${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

/**
 * Open the overlay: resolves every registered context contributor and
 * pushes the assembled chips into the store. The overlay component
 * reads chips reactively, so this returns once chip assembly is done
 * even if a contributor was async.
 */
export async function openCmdI(): Promise<void> {
  // Open eagerly so the modal renders within the same tick (with an
  // empty chip rail). If contributor work is slow we don't want a
  // perceptible "the shortcut did nothing" gap.
  useCmdIStore.getState().open()
  try {
    const contributions = await contextContributors.collect()
    const chips: ContextChip[] = contributions.flatMap((c) => c.chips)
    useCmdIStore.getState().setChips(chips)
  } catch (err) {
    // Defensive — `collect()` already swallows per-contributor throws,
    // but a bug in the registry itself shouldn't trap the user inside
    // a half-broken overlay.
    console.warn('[nexus.ai/cmdI] collect failed', err)
    useCmdIStore.getState().setChips([])
  }
}

/** Watchdog clears the overlay status if `stream_done` never arrives. */
let watchdogTimer: ReturnType<typeof setTimeout> | null = null
let watchdogRequestId: string | null = null

function armWatchdog(requestId: string): void {
  clearWatchdog()
  watchdogRequestId = requestId
  watchdogTimer = setTimeout(() => {
    const state = useCmdIStore.getState()
    if (state.currentRequestId !== requestId) return
    state.setError(
      new Error(
        `Cmd+I request timed out after ${CMD_I_REQUEST_TIMEOUT_MS / 1000}s`,
      ),
    )
    watchdogTimer = null
    watchdogRequestId = null
  }, CMD_I_REQUEST_TIMEOUT_MS)
}

function clearWatchdog(): void {
  if (watchdogTimer) clearTimeout(watchdogTimer)
  watchdogTimer = null
  watchdogRequestId = null
}

interface StreamAskResult {
  session_id?: string
  text?: string
  // sources omitted for v1 — citations are BL-038, deliberately out of scope.
}

/**
 * Submit the current overlay prompt. Builds the assembled context,
 * fires `stream_ask` with a fresh `cmdi-…` session id, and lets the
 * unified stream handler route chunks/done into our store.
 *
 * Single-flight: if a stream is already in flight, the call is a
 * no-op. The overlay's submit button mirrors `status` to disable the
 * input while a request is live.
 */
export async function submitCmdI(api: PluginAPI): Promise<AssembledPrompt | null> {
  const state = useCmdIStore.getState()
  if (state.status === 'submitting' || state.status === 'streaming') {
    return null
  }
  const trimmed = state.prompt.trim()
  if (trimmed.length === 0) return null

  const contributions = await contextContributors.collect()
  const assembled = assemblePrompt(trimmed, contributions)

  const requestId = newRequestId()
  useCmdIStore.getState().beginSubmit(requestId)
  armWatchdog(requestId)

  try {
    const result = await api.kernel.invoke<StreamAskResult>(
      AI_PLUGIN_ID,
      HANDLER_STREAM_CHAT,
      {
        // Single user message — the overlay is one-shot so no prior
        // turn history; the assembled prompt carries any context.
        messages: [{ role: 'user', content: assembled.assembled }],
        session_id: requestId,
      },
      CMD_I_REQUEST_TIMEOUT_MS,
    )
    // Reconcile in case stream_done events were dropped by the bus —
    // the chat runtime does the same dance (aiRuntime.ts:386). Only
    // touch the store if we're still streaming for this request.
    const cur = useCmdIStore.getState()
    if (cur.currentRequestId === requestId && cur.status !== 'done') {
      const text = typeof result?.text === 'string' ? result.text : cur.responseText
      if (watchdogRequestId === requestId) clearWatchdog()
      useCmdIStore.getState().finishResponse(requestId, text)
    }
  } catch (err) {
    if (watchdogRequestId === requestId) clearWatchdog()
    const cur = useCmdIStore.getState()
    if (cur.currentRequestId !== requestId) return assembled
    useCmdIStore.getState().setError(
      err instanceof Error ? err : new Error(String(err)),
    )
  }

  return assembled
}

/**
 * Stream-event router. Called by the AI plugin's existing
 * `subscribeStream` for every `com.nexus.ai.stream_*` event. We claim
 * events whose `session_id` carries the overlay prefix and leave the
 * rest for the chat store.
 *
 * Returns `true` when the event was handled here (router can
 * short-circuit downstream dispatch). The chat-side dispatch in
 * `aiRuntime.subscribeStream` already drops chunks for unknown
 * request ids, so a redundant pass-through would be a no-op — but
 * returning the claim flag lets us add metrics later without
 * changing the call site.
 */
export function routeStreamEvent(
  topic: string,
  payload: unknown,
): boolean {
  if (!payload || typeof payload !== 'object') return false
  const sessionId = (payload as { session_id?: unknown }).session_id
  if (typeof sessionId !== 'string' || !isCmdISessionId(sessionId)) {
    return false
  }
  const store = useCmdIStore.getState()
  if (topic === 'com.nexus.ai.stream_chunk') {
    const chunk = (payload as { chunk?: unknown }).chunk
    if (typeof chunk === 'string' && chunk.length > 0) {
      store.appendResponseChunk(sessionId, chunk)
    }
    return true
  }
  if (topic === 'com.nexus.ai.stream_done') {
    const text = (payload as { text?: unknown }).text
    if (watchdogRequestId === sessionId) clearWatchdog()
    store.finishResponse(sessionId, typeof text === 'string' ? text : '')
    return true
  }
  // stream_start: claimed (so chat doesn't see it) but no-op.
  return true
}
