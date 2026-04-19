import { create } from 'zustand'

/**
 * Plan + Step shapes mirror crates/nexus-agent/src/lib.rs::{Plan,Step}.
 * `tool_call` is optional — informational steps have no side-effects
 * and the executor treats them as no-ops that still appear in the
 * observation log. `args` is preserved as-is so the UI can render
 * arbitrary JSON without flattening.
 */
export interface ToolCall {
  target_plugin_id: string
  command_id: string
  args: unknown
}

export interface PlanStep {
  id: string
  description: string
  tool_call: ToolCall | null
}

export interface Plan {
  id: string
  goal: string
  steps: PlanStep[]
}

/** Per-step run state, keyed off the step's id. */
export type StepStatus = 'queued' | 'running' | 'ok' | 'failed' | 'skipped'

export interface StepRuntime {
  status: StepStatus
  /** Error message when status === 'failed'; null otherwise. */
  error: string | null
}

/**
 * Final per-step result from `com.nexus.agent::run`'s Observation.
 * Mirrors crates/nexus-agent/src/lib.rs::StepResult — `response` is
 * the raw tool-call return value (any JSON), absent for informational
 * steps that didn't dispatch a call.
 */
export interface StepResult {
  step_id: string
  response: unknown
  status: 'ok' | 'failed' | 'skipped'
}

export interface Observation {
  plan_id: string
  steps: StepResult[]
  success: boolean
}

/**
 * One row in the history list. Projected from
 * `com.nexus.agent::history_list` — see crates/nexus-agent/src/core_plugin.rs::handle_history_list.
 */
export interface HistoryRow {
  plan_id: string
  goal: string | null
  created_at: string | null
  success: boolean | null
  steps: number
  bytes: number
}

export type RunPhase = 'idle' | 'planning' | 'planned' | 'running' | 'awaiting' | 'done' | 'error'

/**
 * `auto` — call `com.nexus.agent::run` and let the kernel execute
 * every step server-side. Persists to history. The default.
 *
 * `step` — call `plan` first, then iterate `execute_step` per step
 * with explicit user Approve/Skip/Stop. Does NOT persist to history
 * (the kernel's `execute_step` handler doesn't save records).
 */
export type RunMode = 'auto' | 'step'

interface AgentStoreState {
  // ── Composer ──
  goal: string
  runMode: RunMode

  // ── Current plan + run ──
  phase: RunPhase
  plan: Plan | null
  /** Per-step runtime status, keyed by step id. */
  stepRuntime: Record<string, StepRuntime>
  observation: Observation | null
  /** Top-level error from plan/run that prevented an Observation. */
  runError: string | null
  /**
   * In step-by-step mode: the index of the next step awaiting the
   * user's Approve/Skip decision. Null in auto mode and at the end
   * of a stepped run.
   */
  pendingApprovalIndex: number | null

  // ── History ──
  historyLoading: boolean
  historyError: string | null
  history: HistoryRow[]

  setGoal(g: string): void
  setRunMode(m: RunMode): void

  setPhase(p: RunPhase): void
  setPlan(p: Plan | null): void
  setRunError(e: string | null): void
  setObservation(o: Observation | null): void
  setPendingApprovalIndex(i: number | null): void

  /** Mark every step queued; used right before a run starts. */
  resetRuntime(): void
  setStepStatus(stepId: string, status: StepStatus, error?: string | null): void

  setHistoryLoading(b: boolean): void
  setHistoryError(e: string | null): void
  setHistory(h: HistoryRow[]): void

  reset(): void
}

const INITIAL_STEP_RUNTIME: StepRuntime = { status: 'queued', error: null }

export const useAgentStore = create<AgentStoreState>((set) => ({
  goal: '',
  runMode: 'auto',

  phase: 'idle',
  plan: null,
  stepRuntime: {},
  observation: null,
  runError: null,
  pendingApprovalIndex: null,

  historyLoading: false,
  historyError: null,
  history: [],

  setGoal: (goal) => set({ goal }),
  setRunMode: (runMode) => set({ runMode }),

  setPhase: (phase) => set({ phase }),
  setPlan: (plan) =>
    set({
      plan,
      stepRuntime: plan
        ? Object.fromEntries(plan.steps.map((s) => [s.id, INITIAL_STEP_RUNTIME]))
        : {},
      observation: null,
      runError: null,
      pendingApprovalIndex: null,
    }),
  setRunError: (runError) => set({ runError }),
  setObservation: (observation) => set({ observation }),
  setPendingApprovalIndex: (pendingApprovalIndex) => set({ pendingApprovalIndex }),

  resetRuntime: () =>
    set((s) => ({
      stepRuntime: s.plan
        ? Object.fromEntries(s.plan.steps.map((step) => [step.id, INITIAL_STEP_RUNTIME]))
        : {},
      observation: null,
      runError: null,
    })),
  setStepStatus: (stepId, status, error = null) =>
    set((s) => ({
      stepRuntime: {
        ...s.stepRuntime,
        [stepId]: {
          status,
          error: status === 'failed' ? error : null,
        },
      },
    })),

  setHistoryLoading: (b) => set({ historyLoading: b }),
  setHistoryError: (e) => set({ historyError: e }),
  setHistory: (history) => set({ history }),

  reset: () =>
    set({
      goal: '',
      runMode: 'auto',
      phase: 'idle',
      plan: null,
      stepRuntime: {},
      observation: null,
      runError: null,
      pendingApprovalIndex: null,
      historyLoading: false,
      historyError: null,
      history: [],
    }),
}))
