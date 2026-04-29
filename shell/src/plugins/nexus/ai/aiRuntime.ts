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
import {
  useAiStore,
  type AiConfig,
  type AiSessionMeta,
  type AiSource,
  type AiCitation,
  type AiTurn,
} from './aiStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_CONFIG = 'config'
const HANDLER_SET_CONFIG = 'set_config'
const HANDLER_STREAM_ASK = 'stream_ask'
// Slice C session handlers — verified against
// `crates/nexus-ai/src/core_plugin.rs` (HANDLER_SESSION_LOAD = 8 etc.,
// dispatched by string id in `dispatch_handler`).
const HANDLER_SESSION_LIST = 'session_list'
const HANDLER_SESSION_LOAD = 'session_load'
const HANDLER_SESSION_SAVE = 'session_save'
const HANDLER_SESSION_DELETE = 'session_delete'

const TOPIC_PREFIX = 'com.nexus.ai.stream_'
const TOPIC_CHUNK = 'com.nexus.ai.stream_chunk'
const TOPIC_DONE = 'com.nexus.ai.stream_done'
// `stream_start` carries `sources` too — Slice B could pre-attach them
// to the assistant turn so the chips render before any tokens arrive.
// Skipped for now: the `stream_done` payload also carries them, and
// rendering chips beside a still-empty bubble feels jarring.

/** Top-k RAG sources fetched per question. Match nexus-ai's default. */
const RAG_TOP_K = 5

/** Wall-clock budget for a single Q&A. Closes legacy ChatPanel.tsx
 *  reference §6 "no client-side timeout, no abort button" gap. */
const AI_REQUEST_TIMEOUT_MS = 60_000

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
        `AI request timed out after ${AI_REQUEST_TIMEOUT_MS / 1000}s with no stream_done`,
      ),
    )
    watchdogTimer = null
    watchdogRequestId = null
  }, AI_REQUEST_TIMEOUT_MS)
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

/** Mirrors `Citation` in `crates/nexus-ai/src/rag.rs`. */
interface RawCitation {
  index?: number
  file_path?: string
  block_id?: number
  start_line?: number | null
  end_line?: number | null
  excerpt?: string
  score?: number
}

interface StreamDoneEvent {
  session_id?: string
  text?: string
  sources?: RawChunkMatch[]
  citations?: RawCitation[]
}

interface StreamAskResult {
  session_id?: string
  text?: string
  sources?: RawChunkMatch[]
  citations?: RawCitation[]
}

/** Coerce a raw `Citation` payload from the kernel into the store's
 *  `AiCitation`. Drops entries without a usable `file_path` or
 *  non-numeric index. BL-038. */
function coerceCitations(raw: unknown): AiCitation[] {
  if (!Array.isArray(raw)) return []
  const out: AiCitation[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as RawCitation
    if (typeof r.file_path !== 'string' || r.file_path.length === 0) continue
    if (typeof r.index !== 'number') continue
    out.push({
      index: r.index,
      path: r.file_path,
      blockId: typeof r.block_id === 'number' ? r.block_id : 0,
      startLine: typeof r.start_line === 'number' ? r.start_line : null,
      endLine: typeof r.end_line === 'number' ? r.end_line : null,
      excerpt: typeof r.excerpt === 'string' ? r.excerpt : '',
      score: typeof r.score === 'number' ? r.score : 0,
    })
  }
  // Sort by index for stable rendering even if the kernel order shifts.
  out.sort((a, b) => a.index - b.index)
  return out
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
      const citations = coerceCitations((payload as StreamDoneEvent).citations)
      if (watchdogRequestId === sessionId) clearWatchdog()
      store.finishStream(sessionId, text, sources, citations)
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

/** Shape of the user-saved AI provider settings, read out of the
 *  shell's config store (`useConfigStore`). All fields optional —
 *  blank values fall back to environment-variable detection on the
 *  kernel side. */
export interface AiUserConfig {
  /** Chat provider id: 'anthropic' | 'openai' | 'ollama' | '' (=clear). */
  provider?: string
  /** Optional model override. */
  model?: string
  /** API key for authenticated providers (Anthropic, OpenAI). */
  apiKey?: string
  /** Optional endpoint override (Ollama URL or OpenAI-compatible proxy). */
  baseUrl?: string
  /** Embedding provider for RAG. Defaults to chat provider when blank
   *  and the chat provider supports embeddings (currently OpenAI). */
  embedProvider?: string
  embedApiKey?: string
  embedBaseUrl?: string
}

/** Build the kernel-side `set_config` payload from a user config. An
 *  empty `provider` clears that side (kernel falls back to env). */
function buildSetConfigPayload(user: AiUserConfig): Record<string, unknown> {
  const payload: Record<string, unknown> = {}
  const ai = (user.provider ?? '').trim()
  if (ai.length === 0) {
    payload.ai = null
  } else {
    payload.ai = {
      provider: ai,
      model: (user.model ?? '').trim() || null,
      api_key: (user.apiKey ?? '').trim() || null,
      base_url: (user.baseUrl ?? '').trim() || null,
    }
  }
  // Embedding side: explicit provider always wins; otherwise mirror
  // the chat provider when it supports embeddings (OpenAI/Ollama),
  // reusing the chat key/url so the user only fills one form.
  const explicitEmbed = (user.embedProvider ?? '').trim()
  if (explicitEmbed.length > 0) {
    payload.embedding = {
      provider: explicitEmbed,
      model: null,
      api_key: (user.embedApiKey ?? '').trim() || null,
      base_url: (user.embedBaseUrl ?? '').trim() || null,
    }
  } else if (ai === 'openai' || ai === 'ollama') {
    payload.embedding = {
      provider: ai,
      model: null,
      api_key: (user.apiKey ?? '').trim() || null,
      base_url: (user.baseUrl ?? '').trim() || null,
    }
  } else {
    // Anthropic doesn't ship embeddings — clear so the kernel either
    // falls back to env (OPENAI_API_KEY) or surfaces the missing
    // provider error instead of silently using stale state.
    payload.embedding = null
  }
  return payload
}

/**
 * Push user-saved provider settings into the kernel via `set_config`,
 * then re-hydrate the snapshot so the chat view renders the new
 * provider/model labels. No-op when every field is blank — the kernel
 * keeps whatever it picked up from env vars on init.
 */
export async function pushUserConfig(
  api: PluginAPI,
  user: AiUserConfig,
): Promise<void> {
  const allBlank =
    !user.provider &&
    !user.model &&
    !user.apiKey &&
    !user.baseUrl &&
    !user.embedProvider &&
    !user.embedApiKey &&
    !user.embedBaseUrl
  if (allBlank) {
    // First-run / user blanked everything: don't override env-detected
    // config with a clear, just refresh the snapshot in case anything
    // upstream changed.
    await hydrateConfig(api)
    return
  }
  try {
    const payload = buildSetConfigPayload(user)
    const cfg = await api.kernel.invoke<AiConfig>(
      AI_PLUGIN_ID,
      HANDLER_SET_CONFIG,
      payload,
    )
    useAiStore.getState().setConfig(cfg)
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] pushUserConfig failed', err)
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
        limit: RAG_TOP_K,
      },
      AI_REQUEST_TIMEOUT_MS,
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
      const citations = coerceCitations(result.citations)
      if (watchdogRequestId === requestId) clearWatchdog()
      useAiStore.getState().finishStream(requestId, text, sources, citations)
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

// ── Session management (Slice C) ──────────────────────────────────────────
//
// IPC contract — verified against `crates/nexus-ai/src/core_plugin.rs`:
//
//   session_list   -> []  | [{ id, title?, updated_at?, bytes }]
//   session_load   -> null | { id?, title?, turns: [...], ... }
//                     args: { id }
//   session_save   -> { bytes, id }
//                     args: { id?, title?, turns: [...], updated_at? }
//                     (kernel persists the bare object verbatim)
//   session_delete -> { deleted: true, id }
//                     args: { id }
//
// There is NO dedicated `session_rename` handler; rename = save with
// the same id and a new `title` string. The kernel doesn't inspect
// the payload shape, so we drive it from the shell.

/** Min delay between auto-saves of the active session. Slice C target:
 *  one persistence write per assistant `done`, debounced so a fast
 *  retry / cancel-then-resend doesn't double-write. */
const AUTOSAVE_DEBOUNCE_MS = 1000

/** Title cap mirrors legacy ChatPanel.tsx:101 (48 chars + ellipsis). */
const AI_TITLE_MAX_CHARS = 48

/** Generated id format mirrors legacy makeSessionId (ChatPanel.tsx:158)
 *  with `s-` prefix to keep it short on disk. The kernel validates
 *  `[A-Za-z0-9_-]{1,64}` (core_plugin.rs `validate_session_id`); the
 *  prefix + crypto suffix lands well inside that. */
function newSessionId(): string {
  const rand =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID().replace(/-/g, '').slice(0, 12)
      : Math.random().toString(36).slice(2, 14)
  return `s-${Date.now().toString(36)}-${rand}`
}

/** Auto-derive a session title from the first user turn. Whitespace
 *  collapsed, trimmed, capped at AI_TITLE_MAX_CHARS. Returns the empty string
 *  if no user turn exists yet (caller decides what to do). Mirrors
 *  legacy ChatPanel.tsx:101–106 verbatim. */
function deriveTitle(turns: AiTurn[]): string {
  for (const t of turns) {
    if (t.kind === 'user') {
      const trimmed = t.question.trim().replace(/\s+/g, ' ')
      if (trimmed.length === 0) return ''
      return trimmed.length > AI_TITLE_MAX_CHARS ? `${trimmed.slice(0, AI_TITLE_MAX_CHARS)}…` : trimmed
    }
  }
  return ''
}

/** Coerce a raw `session_list` entry from the kernel into AiSessionMeta.
 *  Drops entries with no id (defensive — the kernel always populates
 *  id, but the wire is `unknown`). */
function coerceSessionMeta(raw: unknown): AiSessionMeta | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (typeof r.id !== 'string' || r.id.length === 0) return null
  return {
    id: r.id,
    title: typeof r.title === 'string' ? r.title : '',
    updatedAt: typeof r.updated_at === 'string' ? r.updated_at : null,
    bytes: typeof r.bytes === 'number' ? r.bytes : 0,
  }
}

function coerceSessionList(raw: unknown): AiSessionMeta[] {
  if (!Array.isArray(raw)) return []
  const out: AiSessionMeta[] = []
  for (const item of raw) {
    const meta = coerceSessionMeta(item)
    if (meta) out.push(meta)
  }
  // Newest-first by updated_at (lexicographic on ISO strings is
  // chronological). Sessions without updated_at sink to the bottom.
  out.sort((a, b) => {
    if (a.updatedAt && b.updatedAt) return b.updatedAt.localeCompare(a.updatedAt)
    if (a.updatedAt) return -1
    if (b.updatedAt) return 1
    return a.id.localeCompare(b.id)
  })
  return out
}

/** Strip non-persistable runtime state from turns before save. Currently
 *  the AiTurn shape is already serializable (see Slice B), but if a
 *  future field is request-lifetime only (e.g. an AbortController), it
 *  would be filtered here. Also drops still-streaming assistant turns
 *  — half-finished bubbles never hit disk (legacy ChatPanel.tsx:441). */
function turnsForPersist(turns: AiTurn[]): AiTurn[] {
  const out: AiTurn[] = []
  for (const t of turns) {
    if (t.kind === 'assistant' && t.status === 'streaming') continue
    out.push(t)
  }
  return out
}

/**
 * Refresh the saved-session list from the kernel.
 *
 * Toggles `sessionsLoading` around the round-trip so the picker can
 * render a skeleton. List-refresh policy (Slice C decision): we ONLY
 * call this on activate, after save, and after delete — NOT on every
 * `turns.length` change. The legacy was chatty (reference §5); we're
 * deliberately quieter.
 */
export async function loadSessions(api: PluginAPI): Promise<void> {
  const store = useAiStore.getState()
  store.setSessionsLoading(true)
  try {
    const raw = await api.kernel.invoke<unknown>(
      AI_PLUGIN_ID,
      HANDLER_SESSION_LIST,
      {},
    )
    useAiStore.getState().setSessions(coerceSessionList(raw))
  } catch (err) {
    // Plugin may not be wired yet — swallow per legacy (ChatPanel.tsx:287).
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] loadSessions failed', err)
    useAiStore.getState().setSessions([])
  } finally {
    useAiStore.getState().setSessionsLoading(false)
  }
}

interface PersistedSession {
  id?: string
  title?: string
  turns?: unknown
  updated_at?: string
}

/** Reconstruct a turn from the persisted JSON. Mirrors the inverse of
 *  `turnsForPersist`. Defensive: drops malformed entries silently so a
 *  partially-corrupt session file doesn't strand the UI. */
function decodeTurn(raw: unknown): AiTurn | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (r.kind === 'user') {
    if (typeof r.id !== 'string' || typeof r.question !== 'string') return null
    return {
      kind: 'user',
      id: r.id,
      question: r.question,
      askedAt: typeof r.askedAt === 'number' ? r.askedAt : Date.now(),
    }
  }
  if (r.kind === 'assistant') {
    if (typeof r.id !== 'string' || typeof r.requestId !== 'string') return null
    const sources = Array.isArray(r.sources)
      ? (r.sources as unknown[]).filter(
          (s): s is AiSource =>
            !!s && typeof s === 'object' && typeof (s as AiSource).path === 'string',
        )
      : []
    const citations = Array.isArray(r.citations)
      ? (r.citations as unknown[]).filter(
          (c): c is AiCitation =>
            !!c &&
            typeof c === 'object' &&
            typeof (c as AiCitation).index === 'number' &&
            typeof (c as AiCitation).path === 'string',
        )
      : []
    // Persisted turns are never `streaming` (filtered by turnsForPersist),
    // and `error` is rehydrated as 'done' since the Error object can't
    // round-trip through JSON without losing its prototype. The persisted
    // body still shows in the bubble.
    return {
      kind: 'assistant',
      id: r.id,
      requestId: r.requestId,
      status: 'done',
      streamedText: '',
      finalText: typeof r.finalText === 'string' ? r.finalText : null,
      sources,
      citations,
      error: null,
    }
  }
  return null
}

function decodeTurns(raw: unknown): AiTurn[] {
  if (!Array.isArray(raw)) return []
  const out: AiTurn[] = []
  for (const item of raw) {
    const t = decodeTurn(item)
    if (t) out.push(t)
  }
  return out
}

/**
 * Load a saved session by id. Cancels any in-flight stream first so
 * late chunks from the previous request can't land into the freshly
 * hydrated turns. Sets activeSessionId on success.
 */
export async function loadSession(api: PluginAPI, id: string): Promise<void> {
  // Cancel before swapping turns — otherwise a tail chunk from the
  // departing stream would write into the hydrated assistant turn (or
  // bounce off the missing-turn guard, depending on requestId).
  cancelInFlight()
  try {
    const raw = await api.kernel.invoke<PersistedSession | null>(
      AI_PLUGIN_ID,
      HANDLER_SESSION_LOAD,
      { id },
    )
    if (!raw || typeof raw !== 'object') {
      // Empty / missing — show as empty and adopt id so subsequent
      // saves overwrite the empty file rather than minting a new one.
      useAiStore.getState().hydrateTurns([])
      useAiStore.getState().setActiveSessionId(id)
      return
    }
    const turns = decodeTurns(raw.turns)
    useAiStore.getState().hydrateTurns(turns)
    useAiStore.getState().setActiveSessionId(id)
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] loadSession failed', err)
  }
}

interface SaveResult {
  bytes?: number
  id?: string | null
}

/**
 * Persist the current `turns` to the kernel.
 *
 * If `activeSessionId` is null, mints a new id (Slice C: this is how
 * "fork from existing" works — call after `newSession` to start a
 * fresh saved conversation). Title resolution order:
 *
 *   1. caller-supplied `title` arg (used by rename + explicit "Save as")
 *   2. existing session's title (preserve user-edits across auto-saves)
 *   3. `deriveTitle(turns)` — auto from first user turn
 *   4. empty string (no user turns yet, no title supplied)
 *
 * Refreshes the session list on success so the picker reflects the
 * new updated_at + (for new sessions) the new entry. Empty
 * conversations are NOT persisted — they'd just be noise in the list.
 */
export async function saveCurrentSession(
  api: PluginAPI,
  title?: string,
): Promise<string | null> {
  const state = useAiStore.getState()
  const persistTurns = turnsForPersist(state.turns)
  if (persistTurns.length === 0) return null

  const id = state.activeSessionId ?? newSessionId()
  let resolvedTitle = title
  if (resolvedTitle === undefined) {
    const existing = state.sessions.find((s) => s.id === id)
    resolvedTitle = existing?.title || deriveTitle(persistTurns)
  }
  const updated_at = new Date().toISOString()

  try {
    await api.kernel.invoke<SaveResult>(AI_PLUGIN_ID, HANDLER_SESSION_SAVE, {
      id,
      title: resolvedTitle,
      turns: persistTurns,
      updated_at,
    })
    if (state.activeSessionId !== id) {
      useAiStore.getState().setActiveSessionId(id)
    }
    // Refresh list so the picker shows the new title / updated_at.
    await loadSessions(api)
    return id
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] saveCurrentSession failed', err)
    return null
  }
}

/**
 * Delete a session by id. If the deleted session is the active one,
 * also clears the local conversation (the user's looking at content
 * that no longer has a backing file — leaving it on screen invites
 * an accidental save under a new id).
 */
export async function deleteSession(api: PluginAPI, id: string): Promise<void> {
  try {
    await api.kernel.invoke<unknown>(AI_PLUGIN_ID, HANDLER_SESSION_DELETE, { id })
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] deleteSession failed', err)
    // Still proceed to refresh the list — legacy ChatPanel.tsx:798
    // pattern (warn + carry on; the file may already be gone).
  }
  const state = useAiStore.getState()
  if (state.activeSessionId === id) {
    cancelInFlight()
    state.newSession()
  }
  await loadSessions(api)
}

/**
 * Rename a session. The kernel doesn't expose a dedicated handler;
 * this is `session_save` with the existing id + new title. We re-save
 * the existing turns from disk to preserve the body (we may not have
 * them in memory if the session isn't the active one).
 *
 * For the active session we shortcut and reuse the in-memory `turns`
 * — saves the round-trip and avoids a momentary state where the disk
 * file has fewer turns than the screen.
 */
export async function renameSession(
  api: PluginAPI,
  id: string,
  title: string,
): Promise<void> {
  const trimmed = title.trim()
  if (trimmed.length === 0) return
  const state = useAiStore.getState()

  let turnsToWrite: AiTurn[]
  if (state.activeSessionId === id) {
    turnsToWrite = turnsForPersist(state.turns)
  } else {
    try {
      const raw = await api.kernel.invoke<PersistedSession | null>(
        AI_PLUGIN_ID,
        HANDLER_SESSION_LOAD,
        { id },
      )
      turnsToWrite = decodeTurns(raw?.turns)
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn('[nexus.ai] renameSession load failed', err)
      return
    }
  }

  try {
    await api.kernel.invoke<SaveResult>(AI_PLUGIN_ID, HANDLER_SESSION_SAVE, {
      id,
      title: trimmed,
      turns: turnsToWrite,
      updated_at: new Date().toISOString(),
    })
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn('[nexus.ai] renameSession save failed', err)
    return
  }
  await loadSessions(api)
}

/**
 * "New chat" entrypoint. Auto-saves the outgoing conversation under
 * its current id (so the user doesn't lose work mid-stream), cancels
 * any in-flight request, then clears local state via `newSession`.
 *
 * The auto-save is fire-and-forget on the partial — the cancel-stream
 * pathway flips the streaming assistant turn to `done` first
 * (preserving streamedText as finalText), so what lands on disk is
 * coherent and renderable on the next load.
 */
export async function startNewChat(api: PluginAPI): Promise<void> {
  // Cancel BEFORE the autosave so the streaming turn is finalized into
  // its partial finalText — turnsForPersist would otherwise drop it.
  cancelInFlight()
  // Best-effort; never block the new-chat action on save failure.
  await saveCurrentSession(api).catch(() => undefined)
  useAiStore.getState().newSession()
}

// ── Auto-save debouncer ───────────────────────────────────────────────────

let autosaveTimer: ReturnType<typeof setTimeout> | null = null

/**
 * Schedule a debounced auto-save. Replaces any pending timer — only
 * the trailing call wins, so a streaming burst that produces three
 * `stream_done` events back-to-back collapses into one disk write.
 *
 * Wire this from `index.ts` via a `useAiStore.subscribe` on the turns
 * array; whenever the most-recent assistant turn becomes `done`, call
 * here. Empty conversations are no-ops (saveCurrentSession bails).
 */
export function scheduleAutosave(api: PluginAPI): void {
  if (autosaveTimer) clearTimeout(autosaveTimer)
  autosaveTimer = setTimeout(() => {
    autosaveTimer = null
    void saveCurrentSession(api)
  }, AUTOSAVE_DEBOUNCE_MS)
}

/** Tear down any pending autosave — used on plugin deactivate / test
 *  isolation. */
export function flushAutosave(): void {
  if (autosaveTimer) {
    clearTimeout(autosaveTimer)
    autosaveTimer = null
  }
}

// ── BL-035 — one-shot stream_chat for AI actions ──────────────────────────
//
// Direct chat path (no RAG retrieval) used by the right-click and
// block-handle AI actions. Bypasses the chat-store turn machinery on
// purpose: actions emit their result inline (toast / chat panel / future
// editor splice) rather than appending to the active conversation, so
// running "Summarize" three times in a row doesn't pollute the chat
// transcript with three unrelated user turns.
//
// Backed by `com.nexus.ai::stream_chat` (handler id 6) so the BL-016
// tool registry is reachable — `tools: 'auto'` lets the model call
// shipped tools when relevant. The handler's invoke promise resolves
// with the final assistant text on completion (it internally drains the
// stream); we don't subscribe to chunk events here.

const HANDLER_STREAM_CHAT = 'stream_chat'

/** A single conversation message in the `stream_chat` payload shape. */
export interface StreamChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

/** Args accepted by {@link streamChat}. Mirrors `AiStreamChatArgs` in
 *  `crates/nexus-ai/src/ipc.rs` minus the kernel-side fields the shell
 *  doesn't need to expose (`mode` is implicit `chat`, `trim` only
 *  applies to `complete`). */
export interface StreamChatRequest {
  messages: StreamChatMessage[]
  /** Optional system prompt forwarded to the provider. */
  system?: string
  /** Tool-advertisement policy. Defaults to `'auto'` — actions want
   *  the BL-016 tool registry advertised so the model can call tools
   *  during a summarize / rewrite / explain. */
  tools?: 'auto' | 'none'
  /** Optional explicit session id. When omitted, a fresh `action-<uuid>`
   *  is minted so events from concurrent actions don't cross-route. */
  sessionId?: string
  /** Optional generation cap forwarded to the provider. */
  maxTokens?: number
}

/** Final text shape returned by `stream_chat` on the invoke side. */
interface StreamChatResult {
  session_id?: string
  text?: string
}

/**
 * Fire a one-shot `com.nexus.ai::stream_chat` round-trip and resolve
 * with the final assistant text. Used by BL-035 AI actions; the chat
 * view continues to use {@link submitQuestion} (RAG-backed `stream_ask`).
 *
 * Errors propagate to the caller — actions catch them and surface a
 * toast rather than crashing the menu.
 */
export async function streamChat(
  api: PluginAPI,
  req: StreamChatRequest,
): Promise<string> {
  if (!kernel) {
    throw new Error('AI plugin not activated (kernel handle missing)')
  }
  const sessionId = req.sessionId ?? `action-${newRequestId()}`
  const payload: Record<string, unknown> = {
    messages: req.messages,
    session_id: sessionId,
    tools: req.tools ?? 'auto',
  }
  if (req.system !== undefined) payload.system = req.system
  if (req.maxTokens !== undefined) payload.max_tokens = req.maxTokens

  const result = await api.kernel.invoke<StreamChatResult>(
    AI_PLUGIN_ID,
    HANDLER_STREAM_CHAT,
    payload,
    AI_REQUEST_TIMEOUT_MS,
  )
  return typeof result?.text === 'string' ? result.text : ''
}
