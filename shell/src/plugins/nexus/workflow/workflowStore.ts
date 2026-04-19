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

interface WorkflowStoreState {
  /** List load state. */
  loading: boolean
  loadError: string | null
  workflows: WorkflowEntry[]
  /** Per-workflow run state, keyed by workflow name. */
  runs: Record<string, RunState>

  setLoading(b: boolean): void
  setLoadError(e: string | null): void
  setWorkflows(ws: WorkflowEntry[]): void
  setRunStatus(name: string, status: RunStatus, error?: string | null): void
  reset(): void
}

const INITIAL_RUN_STATE: RunState = {
  status: 'idle',
  error: null,
  finishedAt: null,
}

export const useWorkflowStore = create<WorkflowStoreState>((set) => ({
  loading: false,
  loadError: null,
  workflows: [],
  runs: {},

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
  reset: () =>
    set({
      loading: false,
      loadError: null,
      workflows: [],
      runs: {},
    }),
}))

export function getRunState(name: string): RunState {
  return useWorkflowStore.getState().runs[name] ?? INITIAL_RUN_STATE
}
