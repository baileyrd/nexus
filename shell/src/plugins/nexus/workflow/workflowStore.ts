import { create } from 'zustand'

/**
 * One workflow row, projected from `com.nexus.workflow::list`.
 *
 * The kernel returns the full `Workflow` struct (see
 * crates/nexus-workflow/src/lib.rs::Workflow); the sidebar only
 * renders a handful of fields so we project at decode time and keep
 * everything else off the store. `name` doubles as the unique key the
 * `run` handler accepts.
 *
 * `triggerType` mirrors `trigger.type` from the TOML — `manual`,
 * `cron`, `file_event`, `webhook`, `git_event`, `mcp_event`. The
 * sidebar exposes a Run button for every entry regardless of trigger:
 * the `run` IPC always permits manual invocation, and the trigger
 * engine simply controls *automatic* firings.
 */
export interface WorkflowEntry {
  name: string
  description: string
  triggerType: string
  stepCount: number
  hasInputs: boolean
}

/** Status for the last `run` attempt against this workflow name. */
export type RunStatus = 'idle' | 'running' | 'done' | 'error'

export interface RunState {
  status: RunStatus
  /** Error message when status === 'error'; null otherwise. */
  error: string | null
  /** Epoch ms of the last terminal status (done/error). */
  finishedAt: number | null
}

/**
 * Status for the most recent `validate` attempt. The kernel handler
 * (`crates/nexus-workflow/src/core_plugin.rs::dispatch_validate`)
 * accepts `{ text }` and either returns the parsed `Workflow` JSON or
 * raises an `ExecutionFailed` carrying the parser error message —
 * which we surface verbatim because the underlying TOML/serde
 * messages already include a useful position hint.
 */
export type ValidateStatus = 'idle' | 'validating' | 'ok' | 'error'

export interface ValidateState {
  status: ValidateStatus
  /** TOML text last submitted (echoed back so the panel persists). */
  text: string
  /** Parser-error message when status === 'error'. */
  error: string | null
  /** `workflow.name` of the parsed file when status === 'ok'. */
  validatedName: string | null
}

interface WorkflowStoreState {
  /** List load state. */
  loading: boolean
  loadError: string | null
  workflows: WorkflowEntry[]
  /** Per-workflow run state, keyed by workflow name. */
  runs: Record<string, RunState>
  /** Validation panel state. */
  validate: ValidateState

  setLoading(b: boolean): void
  setLoadError(e: string | null): void
  setWorkflows(ws: WorkflowEntry[]): void
  setRunStatus(name: string, status: RunStatus, error?: string | null): void
  setValidateText(text: string): void
  setValidateStatus(status: ValidateStatus, opts?: { error?: string | null; validatedName?: string | null }): void
  resetValidate(): void
  reset(): void
}

const INITIAL_RUN_STATE: RunState = {
  status: 'idle',
  error: null,
  finishedAt: null,
}

const INITIAL_VALIDATE_STATE: ValidateState = {
  status: 'idle',
  text: '',
  error: null,
  validatedName: null,
}

export const useWorkflowStore = create<WorkflowStoreState>((set) => ({
  loading: false,
  loadError: null,
  workflows: [],
  runs: {},
  validate: { ...INITIAL_VALIDATE_STATE },

  setLoading: (b) => set({ loading: b }),
  setLoadError: (e) => set({ loadError: e }),
  setWorkflows: (workflows) => set({ workflows }),
  setRunStatus: (name, status, error = null) =>
    set((s) => ({
      runs: {
        ...s.runs,
        [name]: {
          status,
          error: status === 'error' ? error : null,
          finishedAt: status === 'done' || status === 'error' ? Date.now() : s.runs[name]?.finishedAt ?? null,
        },
      },
    })),
  setValidateText: (text) =>
    set((s) => ({
      // Editing the text invalidates any prior verdict so the user
      // doesn't see a stale "ok" pill while typing changes.
      validate: {
        ...s.validate,
        text,
        status: s.validate.status === 'validating' ? s.validate.status : 'idle',
        error: null,
        validatedName: null,
      },
    })),
  setValidateStatus: (status, opts = {}) =>
    set((s) => ({
      validate: {
        ...s.validate,
        status,
        error: status === 'error' ? opts.error ?? null : null,
        validatedName: status === 'ok' ? opts.validatedName ?? null : null,
      },
    })),
  resetValidate: () => set({ validate: { ...INITIAL_VALIDATE_STATE } }),
  reset: () =>
    set({
      loading: false,
      loadError: null,
      workflows: [],
      runs: {},
      validate: { ...INITIAL_VALIDATE_STATE },
    }),
}))

export function getRunState(name: string): RunState {
  return useWorkflowStore.getState().runs[name] ?? INITIAL_RUN_STATE
}
