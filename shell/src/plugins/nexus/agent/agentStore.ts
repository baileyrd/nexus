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
  /**
   * Raw tool-call return value captured during step-mode execution
   * (the `response` field from the kernel's `execute_step` result).
   * `undefined` when the step hasn't run yet, was skipped, or was
   * informational (no tool call). The plan view renders this via a
   * truncated `<pre>` mirroring the legacy AgentHistoryPanel behaviour.
   */
  response?: unknown
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

/**
 * Archetype id forwarded to `com.nexus.agent::{plan,run}` as the
 * optional `archetype` arg. The kernel resolves the string
 * case-insensitively and falls back to the default planner when
 * unknown — see crates/nexus-agent/src/archetypes.rs::resolve_prompt.
 *
 * `null` means "default" (omit `archetype` from the IPC args). The
 * known archetype ids ship in `KNOWN_ARCHETYPES` below; once the
 * kernel exposes a `list_archetypes` IPC we can drop the hardcoded
 * list and fetch at startup.
 */
export type ArchetypeId = 'writer' | 'coder' | 'researcher'

/**
 * Hardcoded archetype catalogue mirrors
 * crates/nexus-agent/src/archetypes.rs (WRITER_ID / CODER_ID /
 * RESEARCHER_ID). TODO(WI-07 follow-up): replace with a kernel
 * `list_archetypes` IPC so the dropdown stays in sync if the set
 * grows.
 */
export const KNOWN_ARCHETYPES: ReadonlyArray<{ id: ArchetypeId; label: string; description: string }> = [
  {
    id: 'writer',
    label: 'Writer',
    description: 'Markdown-authoring bias; prefers storage writes over shell.',
  },
  {
    id: 'coder',
    label: 'Coder',
    description: 'Code edits + git + builds; small reversible steps.',
  },
  {
    id: 'researcher',
    label: 'Researcher',
    description: 'RAG + storage search; reads over writes.',
  },
]

interface AgentStoreState {
  // ── Composer ──
  goal: string
  runMode: RunMode
  /** Selected archetype, or null for the default planner. */
  archetype: ArchetypeId | null

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
  setArchetype(a: ArchetypeId | null): void

  setPhase(p: RunPhase): void
  setPlan(p: Plan | null): void
  setRunError(e: string | null): void
  setObservation(o: Observation | null): void
  setPendingApprovalIndex(i: number | null): void

  /** Mark every step queued; used right before a run starts. */
  resetRuntime(): void
  setStepStatus(stepId: string, status: StepStatus, error?: string | null): void
  /** Store the raw `response` payload returned by `execute_step` for one step. */
  setStepResponse(stepId: string, response: unknown): void

  setHistoryLoading(b: boolean): void
  setHistoryError(e: string | null): void
  setHistory(h: HistoryRow[]): void

  reset(): void
}

const INITIAL_STEP_RUNTIME: StepRuntime = { status: 'queued', error: null }

export const useAgentStore = create<AgentStoreState>((set) => ({
  goal: '',
  runMode: 'auto',
  archetype: null,

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
  setArchetype: (archetype) => set({ archetype }),

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
          // Preserve any prior `response` so the in-progress
          // status flip (queued → running → ok) doesn't clobber a
          // payload captured by an earlier write.
          ...(s.stepRuntime[stepId] ?? INITIAL_STEP_RUNTIME),
          status,
          error: status === 'failed' ? error : null,
        },
      },
    })),
  setStepResponse: (stepId, response) =>
    set((s) => ({
      stepRuntime: {
        ...s.stepRuntime,
        [stepId]: {
          ...(s.stepRuntime[stepId] ?? INITIAL_STEP_RUNTIME),
          response,
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
      archetype: null,
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
