/**
 * Kernel-facing runtime for `nexus.sessions` (RFC 0008, Phase 5.4).
 *
 * Extracted from `index.ts` so `node:test` can exercise the IPC plumbing
 * without dragging the React view (and its CSS import) into a test context.
 * Drives the read side (`session_list` / `session_get`) plus the session-tree
 * fork verbs (`session_resume` / `session_branch` / `session_rewind`) and the
 * checkpoint surface. Every fork runs `auto_approve: true` — the panel is for
 * navigating + re-running stored sessions, not interactive approval (that's the
 * `nexus.agent` run view's job).
 */

import { clientLogger } from '../../../clientLogger'
import { LONG_RUNNING_OP_TIMEOUT_MS } from '../constants'
import { decodeTranscript } from '../agent/sessionStore'
import { decodeSessionNodes } from './sessionTree'
import { decodeCheckpoints, useSessionsStore } from './sessionsStore'

export const AGENT_PLUGIN_ID = 'com.nexus.agent'

const SESSION_LIST = 'session_list'
const SESSION_GET = 'session_get'
const SESSION_DELETE = 'session_delete'
const SESSION_RESUME = 'session_resume'
const SESSION_BRANCH = 'session_branch'
const SESSION_REWIND = 'session_rewind'
const SESSION_CHECKPOINT = 'session_checkpoint'
const SESSION_CHECKPOINTS = 'session_checkpoints'
const SESSION_CHECKPOINT_DELETE = 'session_checkpoint_delete'

/** A fork re-runs the loop and only returns when the session ends. */
const FORK_TIMEOUT_MS = LONG_RUNNING_OP_TIMEOUT_MS
/** Reads + checkpoint CRUD are sub-second. */
const QUICK_IPC_TIMEOUT_MS = 30_000

/**
 * Narrow API surface — the same shape the unit-test stub satisfies. Kept
 * explicit (rather than `Pick<PluginAPI, …>`) so the test stub stays trivial.
 */
export interface SessionsRuntimeDeps {
  kernel: {
    invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
      timeoutMs?: number,
    ): Promise<T>
    available(): Promise<boolean>
  }
  notifications: {
    show(notification: {
      message: string
      type?: 'info' | 'warning' | 'error' | 'success'
    }): void
  }
}

export function createSessionsRuntime(api: SessionsRuntimeDeps) {
  const isAvailable = async (): Promise<boolean> => {
    try {
      return await api.kernel.available()
    } catch {
      return false
    }
  }

  // ── Read side ───────────────────────────────────────────────────────────
  const refreshSessions = async (): Promise<void> => {
    const store = useSessionsStore.getState()
    if (!(await isAvailable())) {
      store.setNodes([])
      store.setError('Open a workspace to load sessions.')
      store.setLoading(false)
      return
    }
    store.setLoading(true)
    store.setError(null)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_LIST,
        {},
        QUICK_IPC_TIMEOUT_MS,
      )
      useSessionsStore.getState().setNodes(decodeSessionNodes(raw))
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      useSessionsStore.getState().setError(message)
      useSessionsStore.getState().setNodes([])
    } finally {
      useSessionsStore.getState().setLoading(false)
    }
  }

  const selectSession = async (id: string): Promise<void> => {
    useSessionsStore.getState().setSelected(id, null, null)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_GET,
        { id },
        QUICK_IPC_TIMEOUT_MS,
      )
      const transcript = decodeTranscript(raw)
      if (!transcript) {
        useSessionsStore
          .getState()
          .setSelected(id, null, 'Session returned an unparseable transcript.')
        return
      }
      useSessionsStore.getState().setSelected(id, transcript, null)
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      useSessionsStore.getState().setSelected(id, null, message)
    }
  }

  const refreshCheckpoints = async (): Promise<void> => {
    if (!(await isAvailable())) return
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_CHECKPOINTS,
        {},
        QUICK_IPC_TIMEOUT_MS,
      )
      useSessionsStore.getState().setCheckpoints(decodeCheckpoints(raw))
    } catch (err) {
      clientLogger.warn('[nexus.sessions] session_checkpoints failed:', err)
    }
  }

  // ── Fork verbs ──────────────────────────────────────────────────────────
  /**
   * Run a fork verb, then refresh the forest and select the new child so the
   * panel jumps to the freshly-created line. Returns the child id, or `null`
   * on failure (a notification is surfaced).
   */
  const runFork = async (
    command: string,
    args: Record<string, unknown>,
    label: string,
  ): Promise<string | null> => {
    if (!(await isAvailable())) {
      api.notifications.show({ type: 'warning', message: 'Open a workspace first.' })
      return null
    }
    useSessionsStore.getState().setBusy(true)
    try {
      const raw = await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        command,
        { ...args, auto_approve: true },
        FORK_TIMEOUT_MS,
      )
      const child = decodeTranscript(raw)
      await refreshSessions()
      if (child) {
        useSessionsStore.getState().setSelected(child.id, child, null)
        return child.id
      }
      return null
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `${label} failed: ${message}` })
      return null
    } finally {
      useSessionsStore.getState().setBusy(false)
    }
  }

  const resume = (sessionId: string, message: string): Promise<string | null> =>
    runFork(SESSION_RESUME, { session_id: sessionId, message }, 'Resume')

  const branch = (
    sessionId: string,
    atRound: number,
    message: string,
  ): Promise<string | null> =>
    runFork(SESSION_BRANCH, { session_id: sessionId, at_round: atRound, message }, 'Branch')

  const rewind = (
    sessionId: string,
    atRound: number,
    message?: string,
  ): Promise<string | null> => {
    const args: Record<string, unknown> = { session_id: sessionId, at_round: atRound }
    if (message && message.trim().length > 0) args.message = message
    return runFork(SESSION_REWIND, args, 'Rewind')
  }

  // ── Checkpoints ─────────────────────────────────────────────────────────
  const checkpoint = async (
    sessionId: string,
    round: number,
    name: string,
  ): Promise<void> => {
    if (!name.trim()) return
    try {
      await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_CHECKPOINT,
        { session_id: sessionId, round, name: name.trim() },
        QUICK_IPC_TIMEOUT_MS,
      )
      api.notifications.show({ type: 'success', message: `Checkpoint '${name.trim()}' saved.` })
      void refreshCheckpoints()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Checkpoint failed: ${msg}` })
    }
  }

  const deleteCheckpoint = async (name: string): Promise<void> => {
    try {
      await api.kernel.invoke<unknown>(
        AGENT_PLUGIN_ID,
        SESSION_CHECKPOINT_DELETE,
        { name },
        QUICK_IPC_TIMEOUT_MS,
      )
      void refreshCheckpoints()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Remove checkpoint failed: ${msg}` })
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
      const store = useSessionsStore.getState()
      if (store.selectedId === id) store.setSelected(null, null, null)
      void refreshSessions()
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      api.notifications.show({ type: 'error', message: `Delete failed: ${message}` })
    }
  }

  return {
    refreshSessions,
    selectSession,
    refreshCheckpoints,
    resume,
    branch,
    rewind,
    checkpoint,
    deleteCheckpoint,
    deleteSession,
  }
}

export type SessionsRuntime = ReturnType<typeof createSessionsRuntime>
