/**
 * Kernel-facing runtime for `nexus.agent`. Extracted from
 * `index.ts` so unit tests can exercise the IPC + bus-event
 * plumbing without dragging the React view (and its CSS import)
 * into a node:test context.
 *
 * The runtime is the closure-bag the activate function wires into
 * its callbacks; everything visual lives in `AgentSessionView`.
 */

import { clientLogger } from '../../../clientLogger'
import { LONG_RUNNING_OP_TIMEOUT_MS } from '../constants'
import { isRoundEntirelySafe } from './riskClassifier'
import {
  decodeArchetypes,
  decodeAskRequested,
  decodeProposedRound,
  decodeSessionList,
  decodeTranscript,
  useAgentSessionStore,
  type PendingRound,
  type RoundRecord,
  type SessionTranscript,
  type StepPolicy,
} from './sessionStore'

export const AGENT_PLUGIN_ID = 'com.nexus.agent'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const STORAGE_READ_FILE = 'read_file'
const SESSION_RUN = 'session_run'
const SESSION_LIST = 'session_list'
const SESSION_GET = 'session_get'
const SESSION_DELETE = 'session_delete'
const ROUND_DECIDE = 'round_decide'
const ASK_RESPOND = 'ask_respond'
const LIST_ARCHETYPES = 'list_archetypes'

/** Bus event the agent emits when a round needs approval. */
const TOPIC_ROUND_PROPOSED = 'com.nexus.agent.round_proposed'
/** Bus event the agent emits when the `ask` tool needs the user's answers. */
const TOPIC_ASK_REQUESTED = 'com.nexus.agent.ask_requested'

/** session_run is long-running by design — it returns only when the
 *  whole session ends. Use the same generous ceiling the legacy
 *  surface used; the kernel's own timeout still applies per-call. */
const SESSION_RUN_TIMEOUT_MS = LONG_RUNNING_OP_TIMEOUT_MS
/** All other agent IPC is fast (sub-second). */
const QUICK_IPC_TIMEOUT_MS = 30_000

/**
 * Narrow API surface — same shape the unit-test stub satisfies.
 * Keep this explicit (rather than `Pick<PluginAPI, …>`) so the
 * surface stays readable + the test stub stays trivial.
 */
export interface AgentRuntimeDeps {
  kernel: {
    invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
      timeoutMs?: number,
    ): Promise<T>
    on<T = unknown>(
      topicPrefix: string,
      handler: (topic: string, payload: T) => void,
    ): Promise<() => void>
    available(): Promise<boolean>
  }
  notifications: {
    show(notification: {
      message: string
      type?: 'info' | 'warning' | 'error' | 'success'
    }): void
  }
}

export function createAgentRuntime(api: AgentRuntimeDeps) {
  let topicUnsub: (() => void) | null = null

  const isAvailable = async (): Promise<boolean> => {
    try {
      return await api.kernel.available()
    } catch {
      return false
    }
  }

  // ── Composer / archetypes ───────────────────────────────────────────
  const loadArchetypes = async (): Promise<void> => {
    if (useAgentSessionStore.getState().archetypesLoaded) return
    if (!(await isAvailable())) return
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        LIST_ARCHETYPES,
        {},
        QUICK_IPC_TIMEOUT_MS,
      )
      useAgentSessionStore.getState().setArchetypes(decodeArchetypes(raw))
    } catch (err) {
      clientLogger.warn('[nexus.agent] list_archetypes failed:', err)
    }
  }

  /**
   * AIG-02 — decide whether the policy lets us short-circuit the
   * approval card. Returns `true` when the round is auto-approved
   * (and the runtime has dispatched `round_decide` itself); `false`
   * means the card must be shown.
   */
  const shouldAutoApprove = (
    policy: StepPolicy,
    decoded: PendingRound,
  ): boolean => {
    if (policy === 'auto_approve') return true
    if (policy === 'always_ask') return false
    // ask_on_risky — allow only when every call is read-only-safe.
    return isRoundEntirelySafe(decoded.toolCalls)
  }

  // ── Topic subscription ──────────────────────────────────────────────
  const handleTopic = (topic: string, payload: unknown) => {
    if (topic === TOPIC_ASK_REQUESTED) {
      handleAskRequested(payload)
      return
    }
    if (topic !== TOPIC_ROUND_PROPOSED) return
    if (!payload || typeof payload !== 'object') return
    const p = payload as Record<string, unknown>
    const sessionId = typeof p.session_id === 'string' ? p.session_id : null
    if (!sessionId) return
    const store = useAgentSessionStore.getState()
    // We may receive the very first round before `session_run`
    // returns its session id — accept it and back-fill.
    if (store.currentSessionId && store.currentSessionId !== sessionId) {
      // Event for a different session (shouldn't happen with one
      // active session, but guard anyway). Ignore.
      return
    }
    if (!store.currentSessionId) {
      store.beginSession(sessionId)
    }
    const decoded = decodeProposedRound(sessionId, payload)
    if (!decoded) return
    if (shouldAutoApprove(store.stepPolicy, decoded)) {
      // Surface the round briefly so the transcript still records
      // what ran, then submit approval without blocking on the user.
      // The optimistic transcript append happens inside submitDecision.
      useAgentSessionStore.getState().setPendingRound(decoded)
      void submitDecision('approve_all')
      return
    }
    useAgentSessionStore.getState().setPendingRound(decoded)
  }

  const subscribeTopics = async (): Promise<void> => {
    if (topicUnsub) return
    try {
      topicUnsub = await api.kernel.on('com.nexus.agent.', handleTopic)
    } catch (err) {
      clientLogger.warn('[nexus.agent] subscribe failed:', err)
    }
  }

  const unsubscribeTopics = (): void => {
    if (!topicUnsub) return
    try {
      topicUnsub()
    } catch (err) {
      clientLogger.warn('[nexus.agent] unsubscribe failed:', err)
    }
    topicUnsub = null
  }

  // ── Session lifecycle ───────────────────────────────────────────────
  const startSession = async (): Promise<void> => {
    const store = useAgentSessionStore.getState()
    const goal = store.goal.trim()
    if (!goal) return
    if (!(await isAvailable())) {
      useAgentSessionStore.getState().setLiveError('Open a workspace first.')
      return
    }
    const args: Record<string, unknown> = { goal, auto_approve: false }
    if (store.archetype) args.archetype = store.archetype
    useAgentSessionStore.getState().beginSession(null)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_RUN,
        args,
        SESSION_RUN_TIMEOUT_MS,
      )
      const transcript = decodeTranscript(raw)
      if (transcript) {
        applyFinalTranscript(transcript)
      } else {
        useAgentSessionStore.getState().finishSession('errored')
        useAgentSessionStore.getState().setLiveError('Session returned no transcript.')
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      useAgentSessionStore.getState().setLiveError(message)
    } finally {
      void refreshSessions()
    }
  }

  /**
   * Reconcile the live transcript against the authoritative one
   * returned by `session_run`. The store may already hold partial
   * rounds the round_proposed-driven path appended — the final
   * transcript is the source of truth, so we replace wholesale.
   */
  const applyFinalTranscript = (transcript: SessionTranscript): void => {
    const store = useAgentSessionStore.getState()
    if (!store.currentSessionId) {
      store.beginSession(transcript.id)
    }
    useAgentSessionStore.setState((s) => ({
      liveTranscript: transcript.rounds,
      pendingRound: null,
      pendingAsk: null,
      phase: transcript.outcome === 'errored' ? 'errored' : 'completed',
      liveError: s.liveError,
    }))
  }

  const submitDecision = async (
    decision: 'approve_all' | 'partial' | 'abort',
    reason?: string,
  ): Promise<void> => {
    const store = useAgentSessionStore.getState()
    const pending = store.pendingRound
    if (!pending) return
    const sessionId = store.currentSessionId
    if (!sessionId) return

    const args: Record<string, unknown> = {
      session_id: sessionId,
      kind: decision,
    }
    if (decision === 'partial') {
      args.entries = pending.toolCalls.map((tc) => ({
        tool_use_id: tc.id,
        approve: pending.approvals[tc.id] === true,
        ...(pending.approvals[tc.id] === true ? {} : { reason: 'denied by user' }),
      }))
    } else if (decision === 'abort') {
      args.reason = reason ?? 'aborted by user'
    }

    // Append the round to the live transcript optimistically — the
    // session loop will run with the user's choice and the server
    // will produce its own RoundRecord we'll reconcile when
    // session_run returns. Showing it now keeps the UI responsive
    // while the model thinks about the next round.
    const optimistic: RoundRecord = {
      round: pending.round,
      text: pending.text,
      tool_calls: pending.toolCalls.map((tc) => {
        const approved =
          decision === 'approve_all' ||
          (decision === 'partial' && pending.approvals[tc.id] === true)
        return {
          id: tc.id,
          name: tc.name,
          approved,
          reason: approved
            ? ''
            : decision === 'abort'
              ? (reason ?? 'aborted')
              : 'denied by user',
          response: null,
          error: '',
        }
      }),
    }
    useAgentSessionStore.getState().appendRound(optimistic)
    useAgentSessionStore.getState().setPendingRound(null)

    try {
      await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        ROUND_DECIDE,
        args,
        QUICK_IPC_TIMEOUT_MS,
      )
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Approval failed: ${message}` })
    }
  }

  // ── Interactive `ask` prompts ───────────────────────────────────────
  const handleAskRequested = (payload: unknown): void => {
    const decoded = decodeAskRequested(payload)
    if (!decoded) return
    useAgentSessionStore.getState().setPendingAsk(decoded)
  }

  /**
   * Deliver the user's drafted answers to the waiting `ask` call via
   * `ask_respond`. Answers map each question to `{ id, selected,
   * custom_input? }` — the wire shape `ask` returns to the model. The
   * card clears optimistically; a transport failure surfaces a
   * notification (the backend `ask` will fall back to a timeout).
   */
  const submitAnswer = async (): Promise<void> => {
    const pending = useAgentSessionStore.getState().pendingAsk
    if (!pending) return
    const answers = pending.questions.map((q) => {
      const draft = pending.answers[q.id] ?? { selected: [], customInput: '' }
      const custom = draft.customInput.trim()
      const answer: { id: string; selected: string[]; custom_input?: string } = {
        id: q.id,
        selected: draft.selected,
      }
      if (custom) answer.custom_input = custom
      return answer
    })
    useAgentSessionStore.getState().setPendingAsk(null)
    try {
      await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        ASK_RESPOND,
        { ask_id: pending.askId, answers },
        QUICK_IPC_TIMEOUT_MS,
      )
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Answer failed: ${message}` })
    }
  }

  const clearLive = (): void => {
    useAgentSessionStore.getState().clearLive()
  }

  // ── Sessions sidebar ────────────────────────────────────────────────
  const refreshSessions = async (): Promise<void> => {
    if (!(await isAvailable())) {
      useAgentSessionStore.getState().setSessions([])
      useAgentSessionStore.getState().setSessionsError('Open a workspace to load sessions.')
      useAgentSessionStore.getState().setSessionsLoading(false)
      return
    }
    useAgentSessionStore.getState().setSessionsLoading(true)
    useAgentSessionStore.getState().setSessionsError(null)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_LIST,
        {},
        QUICK_IPC_TIMEOUT_MS,
      )
      useAgentSessionStore.getState().setSessions(decodeSessionList(raw))
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      useAgentSessionStore.getState().setSessionsError(message)
      useAgentSessionStore.getState().setSessions([])
    } finally {
      useAgentSessionStore.getState().setSessionsLoading(false)
    }
  }

  const selectSession = async (id: string): Promise<void> => {
    useAgentSessionStore.getState().setSelectedSession(id, null, null)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_GET,
        { id },
        QUICK_IPC_TIMEOUT_MS,
      )
      const transcript = decodeTranscript(raw)
      if (!transcript) {
        useAgentSessionStore.getState().setSelectedSession(
          id,
          null,
          'Session returned an unparseable transcript.',
        )
        return
      }
      useAgentSessionStore.getState().setSelectedSession(id, transcript, null)
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      useAgentSessionStore.getState().setSelectedSession(id, null, message)
    }
  }

  const deleteSession = async (id: string): Promise<void> => {
    try {
      await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_DELETE,
        { id },
        QUICK_IPC_TIMEOUT_MS,
      )
      const store = useAgentSessionStore.getState()
      if (store.selectedSessionId === id) {
        store.setSelectedSession(null, null, null)
      }
      void refreshSessions()
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Delete failed: ${message}` })
    }
  }

  /**
   * AIG-02 — read a forge file via storage IPC. Powers the diff
   * preview on `write_file` approval cards. Returns `null` on any
   * error (most often "file does not exist yet"); the caller renders
   * a "new file" hint in that case rather than blocking the diff.
   */
  const readFile = async (path: string): Promise<string | null> => {
    if (typeof path !== 'string' || path.length === 0) return null
    if (!(await isAvailable())) return null
    try {
      const raw = await api.kernel.invoke<unknown>(
        STORAGE_PLUGIN_ID,
        STORAGE_READ_FILE,
        { path },
        QUICK_IPC_TIMEOUT_MS,
      )
      if (typeof raw === 'string') return raw
      if (raw && typeof raw === 'object') {
        // Some storage handlers wrap the bytes in `{ contents: string }`.
        const contents = (raw as Record<string, unknown>).contents
        if (typeof contents === 'string') return contents
      }
      return null
    } catch {
      return null
    }
  }

  return {
    loadArchetypes,
    subscribeTopics,
    unsubscribeTopics,
    handleTopic,
    startSession,
    submitDecision,
    submitAnswer,
    clearLive,
    refreshSessions,
    selectSession,
    deleteSession,
    readFile,
  }
}
