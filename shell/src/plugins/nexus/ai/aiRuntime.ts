// Module-scoped holder for kernel handle + focus plumbing, mirroring
// the pattern used by nexus.search. Held out of React so the focus
// command works even when ChatView isn't currently mounted.

import type { KernelAPI } from '../../../types/plugin'
import { useAiStore, type AiMessage } from './aiStore'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_ASK = 'ask'
/** Top-k RAG sources fetched per question. Match nexus.ai's default. */
const DEFAULT_LIMIT = 5

let kernel: KernelAPI | null = null

export function setKernel(api: KernelAPI) {
  kernel = api
}

// ── Focus plumbing ──────────────────────────────────────────────────

type Focuser = () => void
let focuser: Focuser | null = null
let pendingFocus = false

export function registerFocuser(fn: Focuser | null) {
  focuser = fn
  if (fn && pendingFocus) {
    pendingFocus = false
    fn()
  }
}

/** Focus the chat input if mounted; otherwise queue for next mount. */
export function requestFocus() {
  if (focuser) {
    focuser()
  } else {
    pendingFocus = true
  }
}

// ── Send flow ───────────────────────────────────────────────────────

/** RAG response shape returned by com.nexus.ai::ask. */
interface AskResponse {
  answer: string
  sources?: Array<{
    file_path?: string
    block_id?: number
    excerpt?: string
    score?: number
  }>
  model?: string
}

/** crypto.randomUUID is available in Electron / Tauri webviews. */
function uuid(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`
}

/**
 * Send the current input to com.nexus.ai::ask, append the user
 * message, and then append either the assistant response or an error
 * message. Returns when the round-trip is done.
 *
 * Note: ask is a stateless RAG query — the kernel does NOT hold
 * conversation context. Each send embeds the question, fetches the
 * top-k matching chunks via com.nexus.storage, and runs a single
 * grounded chat completion. That's fine for v1; multi-turn context
 * lands when we move to stream_chat + a client-side message buffer.
 */
export async function send(): Promise<void> {
  const state = useAiStore.getState()
  const prompt = state.input.trim()
  if (!prompt || state.sending) return

  const userMsg: AiMessage = {
    id: uuid(),
    role: 'user',
    content: prompt,
    createdAtMs: Date.now(),
  }
  useAiStore.getState().appendMessage(userMsg)
  useAiStore.getState().setInput('')
  useAiStore.getState().setSending(true)
  useAiStore.getState().setError(null)

  try {
    const k = kernel
    if (!k) throw new Error('Kernel not ready')
    if (!(await k.available())) throw new Error('Kernel not ready')

    const resp = await k.invoke<AskResponse>(AI_PLUGIN_ID, HANDLER_ASK, {
      question: prompt,
      limit: DEFAULT_LIMIT,
    })

    const sources = Array.isArray(resp.sources)
      ? resp.sources
          .filter((s): s is { file_path: string; block_id?: number; excerpt?: string; score?: number } =>
            !!s && typeof s.file_path === 'string',
          )
          .map((s) => ({
            file_path: s.file_path,
            block_id: s.block_id,
            excerpt: s.excerpt,
            score: s.score,
          }))
      : undefined

    useAiStore.getState().appendMessage({
      id: uuid(),
      role: 'assistant',
      content: resp.answer ?? '',
      createdAtMs: Date.now(),
      sources,
    })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    useAiStore.getState().appendMessage({
      id: uuid(),
      role: 'error',
      content: message,
      createdAtMs: Date.now(),
    })
    useAiStore.getState().setError(message)
  } finally {
    useAiStore.getState().setSending(false)
  }
}
