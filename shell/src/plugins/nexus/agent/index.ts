/**
 * `nexus.agent` shell plugin — session-driven (ADR 0024 + 0025).
 *
 * Drives `com.nexus.agent::session_run` with `auto_approve: false`,
 * subscribes to the `com.nexus.agent.round_proposed` event the
 * core plugin emits whenever a round needs user approval, and
 * posts `round_decide` once the user clicks. Past sessions are
 * surfaced through `session_list` / `session_get` /
 * `session_delete`.
 *
 * The runtime (`createAgentRuntime`) is extracted from `activate`
 * so unit tests can drive each piece (start a session, simulate
 * an event, post a decision) against a stub kernel.
 */

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { LONG_RUNNING_OP_TIMEOUT_MS } from '../constants'
import { AgentSessionView } from './AgentSessionView'
import {
  decodeArchetypes,
  decodeProposedRound,
  decodeSessionList,
  decodeTranscript,
  useAgentSessionStore,
  type RoundRecord,
  type SessionTranscript,
} from './sessionStore'

const PLUGIN_ID = 'nexus.agent'
const VIEW_ID = 'nexus.agent.view'
const ACTIVITY_ITEM_ID = 'nexus.agent.activityItem'

const COMMAND_SHOW = 'nexus.agent.show'
const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

export const AGENT_PLUGIN_ID = 'com.nexus.agent'
const SESSION_RUN = 'session_run'
const SESSION_LIST = 'session_list'
const SESSION_GET = 'session_get'
const SESSION_DELETE = 'session_delete'
const ROUND_DECIDE = 'round_decide'
const LIST_ARCHETYPES = 'list_archetypes'

/** Bus event the agent emits when a round needs approval. */
const TOPIC_ROUND_PROPOSED = 'com.nexus.agent.round_proposed'

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

  // ── Topic subscription ──────────────────────────────────────────────
  const handleTopic = (topic: string, payload: unknown) => {
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
      // Pull the sidebar list back in — it now includes this run.
      void refreshSessions()
    }
  }

  /**
   * Reconcile the live transcript against the authoritative
   * one returned by `session_run`. The store may already hold
   * partial rounds that the round_proposed-driven path appended —
   * the final transcript is always the source of truth, so we
   * replace wholesale.
   */
  const applyFinalTranscript = (transcript: SessionTranscript): void => {
    const store = useAgentSessionStore.getState()
    if (!store.currentSessionId) {
      // Session ended before the first round event arrived (e.g.
      // outright abort). Just store the id so subsequent bookkeeping
      // is correct.
      store.beginSession(transcript.id)
    }
    // Replace the live transcript with the persisted rounds, then
    // flip the phase based on outcome.
    useAgentSessionStore.setState((s) => ({
      liveTranscript: transcript.rounds,
      pendingRound: null,
      phase: transcript.outcome === 'errored' ? 'errored' : 'completed',
      liveError: s.liveError, // preserve any transport error already shown
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
    // will produce its own `RoundRecord` that we'll reconcile when
    // session_run returns. Showing it now keeps the UI responsive
    // while the model thinks about the next round.
    const optimistic: RoundRecord = {
      round: pending.round,
      text: pending.text,
      tool_calls: pending.toolCalls.map((tc) => {
        const approved = decision === 'approve_all' || (decision === 'partial' && pending.approvals[tc.id] === true)
        return {
          id: tc.id,
          name: tc.name,
          approved,
          reason: approved ? '' : decision === 'abort' ? (reason ?? 'aborted') : 'denied by user',
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
        useAgentSessionStore.getState().setSelectedSession(id, null, 'Session returned an unparseable transcript.')
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
      // Drop the selection if it pointed at the deleted entry.
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

  return {
    loadArchetypes,
    subscribeTopics,
    unsubscribeTopics,
    handleTopic,
    startSession,
    submitDecision,
    clearLive,
    refreshSessions,
    selectSession,
    deleteSession,
  }
}

// ── Plugin manifest ────────────────────────────────────────────────────

export const agentPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Agent',
    version: '0.2.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.paneMode'],
    contributes: {
      commands: [{ id: COMMAND_SHOW, title: 'Show Agent', category: 'Agent' }],
    },
  },

  async activate(api: PluginAPI) {
    const runtime = createAgentRuntime(api)

    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(AgentSessionView, {
          onRun: () => void runtime.startSession(),
          onApprove: (decision, reason) => void runtime.submitDecision(decision, reason),
          onSelectSession: (id) => void runtime.selectSession(id),
          onDeleteSession: (id) => void runtime.deleteSession(id),
          onRefreshSessions: () => void runtime.refreshSessions(),
          onClearLive: runtime.clearLive,
        }),
      priority: 20,
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'sparkle',
      title: 'Agent',
      viewId: VIEW_ID,
      priority: 70,
    })

    api.commands.register(COMMAND_SHOW, async () => {
      void runtime.refreshSessions()
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        void runtime.refreshSessions()
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else {
        const current = usePaneModeStore.getState().activeViewId
        if (current === VIEW_ID) {
          void api.commands.execute(COMMAND_PANE_MODE_EXIT)
        }
      }
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void runtime.refreshSessions()
      void runtime.subscribeTopics()
      void runtime.loadArchetypes()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useAgentSessionStore.getState().reset()
      runtime.unsubscribeTopics()
    })

    if (await api.kernel.available()) {
      void runtime.refreshSessions()
      void runtime.subscribeTopics()
      void runtime.loadArchetypes()
    }
  },
}

export default agentPlugin
