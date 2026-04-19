import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { AgentView } from './AgentView'
import {
  useAgentStore,
  type HistoryRow,
  type Observation,
  type Plan,
  type PlanStep,
} from './agentStore'

const PLUGIN_ID = 'nexus.agent'
const VIEW_ID = 'nexus.agent.view'
const ACTIVITY_ITEM_ID = 'nexus.agent.activityItem'

const COMMAND_SHOW = 'nexus.agent.show'
const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const AGENT_PLUGIN_ID = 'com.nexus.agent'
// Verified against crates/nexus-agent/src/core_plugin.rs:
//   `plan`         args `{ goal, archetype? }`            → `Plan`
//   `run`          args `{ goal, archetype? }`            → `Observation` (plans + executes)
//   `run_plan`     args `{ plan }`                        → `Observation` (executes a known plan)
//   `history_list`   args `{}`                            → `[{ plan_id, goal, created_at, success, steps, bytes }]`
//   `history_get`    args `{ plan_id }`                   → full `{ plan, observation, goal, created_at }`
//   `history_delete` args `{ plan_id }`                   → `{ ok: true }` (or error)
//   `execute_step`   args `{ plan, index }`               → one `StepResult { step_id, response, status }`. Does NOT persist history.
const PLAN_COMMAND = 'plan'
const RUN_COMMAND = 'run'
const HISTORY_LIST_COMMAND = 'history_list'
const HISTORY_GET_COMMAND = 'history_get'
const HISTORY_DELETE_COMMAND = 'history_delete'
const EXECUTE_STEP_COMMAND = 'execute_step'

// Topic prefix covers run_start / step_start / step_done / run_done.
// Matches crates/nexus-agent/src/core_plugin.rs::EVENT_RUN_START etc.
const AGENT_TOPIC_PREFIX = 'com.nexus.agent.'

// Plans + runs are LLM-bound and may stretch — pick a 5-minute ceiling.
// `dispatch_async` already enforces its own 60s chat / tool timeouts
// per-call inside the kernel; this is the bridge-side cap.
const RUN_TIMEOUT_MS = 5 * 60_000

interface StepEventPayload {
  plan_id?: string
  step_id?: string
  index?: number
  status?: string
  error?: string
  description?: string
}

interface RunEventPayload {
  plan_id?: string
  steps?: number
  goal?: string
  success?: boolean
}

function decodePlan(raw: unknown): Plan | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (typeof r.id !== 'string' || typeof r.goal !== 'string') return null
  if (!Array.isArray(r.steps)) return null
  const steps: PlanStep[] = []
  for (const item of r.steps) {
    if (!item || typeof item !== 'object') continue
    const s = item as Record<string, unknown>
    if (typeof s.id !== 'string' || typeof s.description !== 'string') continue
    let toolCall: PlanStep['tool_call'] = null
    if (s.tool_call && typeof s.tool_call === 'object') {
      const tc = s.tool_call as Record<string, unknown>
      if (typeof tc.target_plugin_id === 'string' && typeof tc.command_id === 'string') {
        toolCall = {
          target_plugin_id: tc.target_plugin_id,
          command_id: tc.command_id,
          args: tc.args,
        }
      }
    }
    steps.push({ id: s.id, description: s.description, tool_call: toolCall })
  }
  return { id: r.id, goal: r.goal, steps }
}

function decodeObservation(raw: unknown): Observation | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (typeof r.plan_id !== 'string') return null
  const stepsRaw = Array.isArray(r.steps) ? r.steps : []
  const steps = stepsRaw
    .map((item) => {
      if (!item || typeof item !== 'object') return null
      const s = item as Record<string, unknown>
      if (typeof s.step_id !== 'string') return null
      const status = s.status === 'ok' || s.status === 'failed' || s.status === 'skipped' ? s.status : 'failed'
      return { step_id: s.step_id, response: s.response, status }
    })
    .filter((x): x is Observation['steps'][number] => x !== null)
  return { plan_id: r.plan_id, steps, success: r.success === true }
}

function decodeHistoryList(raw: unknown): HistoryRow[] {
  if (!Array.isArray(raw)) return []
  const out: HistoryRow[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const plan_id = typeof r.plan_id === 'string' ? r.plan_id : null
    if (!plan_id) continue
    out.push({
      plan_id,
      goal: typeof r.goal === 'string' ? r.goal : null,
      created_at: typeof r.created_at === 'string' ? r.created_at : null,
      success: typeof r.success === 'boolean' ? r.success : null,
      steps: typeof r.steps === 'number' ? r.steps : 0,
      bytes: typeof r.bytes === 'number' ? r.bytes : 0,
    })
  }
  // Newest first when timestamps are present.
  return out.sort((a, b) => {
    if (a.created_at && b.created_at) return b.created_at.localeCompare(a.created_at)
    return b.plan_id.localeCompare(a.plan_id)
  })
}

export const agentPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Agent',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.paneMode'],
    contributes: {
      commands: [{ id: COMMAND_SHOW, title: 'Show Agent', category: 'Agent' }],
    },
  },

  async activate(api: PluginAPI) {
    const refreshHistory = async () => {
      const store = useAgentStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setHistoryError('Open a workspace to load agent history.')
        store.setHistory([])
        store.setHistoryLoading(false)
        return
      }
      store.setHistoryLoading(true)
      store.setHistoryError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(AGENT_PLUGIN_ID, HISTORY_LIST_COMMAND, {})
        useAgentStore.getState().setHistory(decodeHistoryList(raw))
        useAgentStore.getState().setHistoryLoading(false)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useAgentStore.getState().setHistoryError(message)
        useAgentStore.getState().setHistory([])
        useAgentStore.getState().setHistoryLoading(false)
      }
    }

    const planOnly = async () => {
      const store = useAgentStore.getState()
      const goal = store.goal.trim()
      if (!goal) return
      store.setPhase('planning')
      store.setPlan(null)
      store.setRunError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          PLAN_COMMAND,
          { goal },
          RUN_TIMEOUT_MS,
        )
        const plan = decodePlan(raw)
        if (!plan) throw new Error('Agent returned an unparseable plan.')
        useAgentStore.getState().setPlan(plan)
        useAgentStore.getState().setPhase('planned')
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useAgentStore.getState().setRunError(message)
        useAgentStore.getState().setPhase('error')
      }
    }

    const planAndRun = async () => {
      const store = useAgentStore.getState()
      const goal = store.goal.trim()
      if (!goal) return
      // Step-by-step mode peels apart plan + execute so the user can
      // approve each step. The kernel's `execute_step` doesn't persist
      // history, so this mode trades supervisability for an audit
      // trail — flagged in the agentStore type comment.
      if (store.runMode === 'step') {
        await planThenAwaitApproval(goal)
        return
      }
      store.setPhase('planning')
      store.setPlan(null)
      store.setRunError(null)
      try {
        // `run` plans + executes server-side and emits the lifecycle
        // topics during execution. We don't get a separate Plan back
        // from this handler — only the Observation. So plan steps are
        // assembled live from the topic stream during the run, and
        // backfilled from history_get once the observation lands.
        //
        // Practically: the user sees "running…" with a plan view that
        // populates as step_start events arrive. This avoids two LLM
        // round-trips (separate plan + run_plan).
        store.setPhase('running')
        const raw = await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          RUN_COMMAND,
          { goal },
          RUN_TIMEOUT_MS,
        )
        const obs = decodeObservation(raw)
        if (!obs) throw new Error('Agent returned an unparseable observation.')
        useAgentStore.getState().setObservation(obs)
        useAgentStore.getState().setPhase('done')
        // History gained an entry — refresh so it appears in the left
        // column.
        void refreshHistory()
        // Backfill the full plan now that the run is over.
        await loadPlanIntoState(obs.plan_id)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useAgentStore.getState().setRunError(message)
        useAgentStore.getState().setPhase('error')
      }
    }

    // ── Step-by-step state machine ──────────────────────────────────────
    //
    // The shell drives the loop instead of the kernel:
    //   1. plan(goal) → store.setPlan(plan), phase='awaiting',
    //                   pendingApprovalIndex=0 (or skip-ahead to first
    //                   informational step's first non-tool? nope —
    //                   user always approves, even informational ones)
    //   2. user clicks Approve → execute_step(plan, idx) → store the
    //                   per-step status, advance the index
    //   3. user clicks Skip → mark skipped locally, advance the index
    //   4. user clicks Stop → mark all remaining as skipped, finish
    //   5. when index == steps.length → build observation locally,
    //                   phase='done'. No history persist (limitation).
    const planThenAwaitApproval = async (goal: string) => {
      const store = useAgentStore.getState()
      store.setPhase('planning')
      store.setPlan(null)
      store.setRunError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          PLAN_COMMAND,
          { goal },
          RUN_TIMEOUT_MS,
        )
        const plan = decodePlan(raw)
        if (!plan) throw new Error('Agent returned an unparseable plan.')
        useAgentStore.getState().setPlan(plan)
        if (plan.steps.length === 0) {
          finishStepRun()
          return
        }
        useAgentStore.getState().setPhase('awaiting')
        useAgentStore.getState().setPendingApprovalIndex(0)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useAgentStore.getState().setRunError(message)
        useAgentStore.getState().setPhase('error')
      }
    }

    const finishStepRun = () => {
      // Build a local Observation from the current per-step runtime so
      // the right column can render the success summary. This stays
      // shell-only — `execute_step` never persists, so this Observation
      // doesn't appear in history_list.
      const s = useAgentStore.getState()
      if (!s.plan) {
        s.setPhase('done')
        return
      }
      const steps = s.plan.steps.map((step) => {
        const rt = s.stepRuntime[step.id]
        const status: 'ok' | 'failed' | 'skipped' =
          rt?.status === 'ok' ? 'ok' : rt?.status === 'failed' ? 'failed' : 'skipped'
        return { step_id: step.id, response: undefined, status }
      })
      const success = steps.every((r) => r.status === 'ok')
      useAgentStore.getState().setObservation({
        plan_id: s.plan.id,
        steps,
        success,
      })
      useAgentStore.getState().setPendingApprovalIndex(null)
      useAgentStore.getState().setPhase('done')
    }

    const advanceStep = () => {
      const s = useAgentStore.getState()
      const next = (s.pendingApprovalIndex ?? -1) + 1
      if (!s.plan || next >= s.plan.steps.length) {
        finishStepRun()
      } else {
        useAgentStore.getState().setPendingApprovalIndex(next)
      }
    }

    const handleApproveStep = async () => {
      const s = useAgentStore.getState()
      const idx = s.pendingApprovalIndex
      if (idx === null || !s.plan) return
      const step = s.plan.steps[idx]
      if (!step) return
      // Block re-entrancy: flip the step to running so a double-click
      // doesn't fire two execute_step calls.
      useAgentStore.getState().setStepStatus(step.id, 'running')
      try {
        const raw = await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          EXECUTE_STEP_COMMAND,
          { plan: s.plan, index: idx },
          RUN_TIMEOUT_MS,
        )
        // Decode just enough to learn whether it succeeded — the full
        // response shape (response field) isn't surfaced in v1.
        const result = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : null
        const status = result?.status === 'ok' || result?.status === 'failed' || result?.status === 'skipped'
          ? result.status
          : 'failed'
        useAgentStore.getState().setStepStatus(
          step.id,
          status === 'ok' ? 'ok' : status === 'skipped' ? 'skipped' : 'failed',
        )
        if (status === 'failed') {
          // Match `run`'s behaviour: a failed step aborts the rest.
          handleStopRun()
        } else {
          advanceStep()
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useAgentStore.getState().setStepStatus(step.id, 'failed', message)
        handleStopRun()
      }
    }

    const handleSkipStep = () => {
      const s = useAgentStore.getState()
      const idx = s.pendingApprovalIndex
      if (idx === null || !s.plan) return
      const step = s.plan.steps[idx]
      if (!step) return
      useAgentStore.getState().setStepStatus(step.id, 'skipped')
      advanceStep()
    }

    const handleStopRun = () => {
      const s = useAgentStore.getState()
      if (!s.plan) {
        useAgentStore.getState().setPendingApprovalIndex(null)
        useAgentStore.getState().setPhase('done')
        return
      }
      // Mark every still-queued step as skipped so the observation
      // summary is accurate.
      for (const step of s.plan.steps) {
        const rt = s.stepRuntime[step.id]
        if (!rt || rt.status === 'queued' || rt.status === 'running') {
          useAgentStore.getState().setStepStatus(step.id, 'skipped')
        }
      }
      finishStepRun()
    }

    const loadPlanIntoState = async (planId: string) => {
      try {
        const raw = await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          HISTORY_GET_COMMAND,
          { plan_id: planId },
        )
        if (!raw || typeof raw !== 'object') return
        const r = raw as Record<string, unknown>
        const plan = decodePlan(r.plan)
        const obs = decodeObservation(r.observation)
        if (plan) {
          useAgentStore.getState().setPlan(plan)
          if (obs) useAgentStore.getState().setObservation(obs)
          if (typeof r.goal === 'string') useAgentStore.getState().setGoal(r.goal)
        }
      } catch (err) {
        console.warn('[nexus.agent] history_get failed:', err)
      }
    }

    const handleLoadHistory = (planId: string) => {
      useAgentStore.getState().setPhase('done')
      void loadPlanIntoState(planId)
    }

    const handleDeleteHistory = async (planId: string) => {
      // Delete is destructive — confirm before firing. The native
      // confirm() is a stop-gap; a bespoke modal lives behind the
      // same TODO as the editor's close-while-dirty prompt.
      const ok = window.confirm('Delete this run from agent history? This cannot be undone.')
      if (!ok) return
      try {
        await api.kernel.invoke<unknown>(
          AGENT_PLUGIN_ID,
          HISTORY_DELETE_COMMAND,
          { plan_id: planId },
        )
        // If the deleted run was loaded into the right column, clear
        // it so we're not pointing at a now-vanished plan.
        if (useAgentStore.getState().plan?.id === planId) {
          useAgentStore.getState().setPlan(null)
          useAgentStore.getState().setObservation(null)
          useAgentStore.getState().setPhase('idle')
        }
        await refreshHistory()
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          type: 'error',
          message: `Delete failed: ${message}`,
        })
      }
    }

    // ── Kernel topic subscriptions ──────────────────────────────────────
    //
    // Live per-step status during a run. The kernel publishes four
    // topics on `com.nexus.agent.*`; we route them into store
    // mutations so the plan view updates without a second IPC call.
    let agentUnsub: (() => void) | null = null

    const handleAgentTopic = (topic: string, payload: unknown) => {
      if (!payload || typeof payload !== 'object') return
      const local = topic.replace(AGENT_TOPIC_PREFIX, '')
      const store = useAgentStore.getState()
      switch (local) {
        case 'run_start': {
          // No-op for now: the UI already flipped to phase 'running'
          // when the invoke fired. A future enhancement could use
          // payload.steps to render a placeholder count before the
          // plan backfills.
          break
        }
        case 'step_start': {
          const p = payload as StepEventPayload
          if (p.step_id) store.setStepStatus(p.step_id, 'running')
          break
        }
        case 'step_done': {
          const p = payload as StepEventPayload
          if (!p.step_id) break
          if (p.status === 'ok') store.setStepStatus(p.step_id, 'ok')
          else if (p.status === 'skipped') store.setStepStatus(p.step_id, 'skipped')
          else store.setStepStatus(p.step_id, 'failed', p.error ?? null)
          break
        }
        case 'run_done': {
          // The run_done topic precedes the IPC return value. Don't
          // flip phase here — let the awaiting invoke do that with
          // the full observation in hand. Touch the payload so a
          // future surfaced field doesn't get accidentally squashed.
          void (payload as RunEventPayload)
          break
        }
      }
    }

    const subscribeAgentTopics = async () => {
      if (agentUnsub) return
      try {
        agentUnsub = await api.kernel.on(AGENT_TOPIC_PREFIX, handleAgentTopic)
      } catch (err) {
        console.warn('[nexus.agent] failed to subscribe to agent topics:', err)
      }
    }

    const unsubscribeAgentTopics = () => {
      if (agentUnsub) {
        try {
          agentUnsub()
        } catch (err) {
          console.warn('[nexus.agent] unsubscribe failed:', err)
        }
        agentUnsub = null
      }
    }

    // ── View + activity bar ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(AgentView, {
          onPlan: () => void planOnly(),
          onRun: () => void planAndRun(),
          onLoadHistory: handleLoadHistory,
          onRefreshHistory: () => void refreshHistory(),
          onDeleteHistory: (planId) => void handleDeleteHistory(planId),
          onApproveStep: () => void handleApproveStep(),
          onSkipStep: handleSkipStep,
          onStopRun: handleStopRun,
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
      void refreshHistory()
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    // Pane-mode routing — same dance as nexus.processes.
    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        void refreshHistory()
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else {
        const current = usePaneModeStore.getState().activeViewId
        if (current === VIEW_ID) {
          void api.commands.execute(COMMAND_PANE_MODE_EXIT)
        }
      }
    })

    // ── Workspace lifecycle ────────────────────────────────────────────
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refreshHistory()
      void subscribeAgentTopics()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useAgentStore.getState().reset()
      unsubscribeAgentTopics()
    })
    if (await api.kernel.available()) {
      void refreshHistory()
      void subscribeAgentTopics()
    }
  },
}
