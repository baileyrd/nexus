// BL-054 Phase 4 — store for the observability plugin.
//
// Three independent panes share this store: usage (activity rollup),
// automation (foundation workflow listing), and vault feed (file-
// activity stream from the kernel bus). Each pane's data is loaded
// independently so a cold open of one tab doesn't pay for the others.

import { create } from 'zustand'

import type { ActivityEntry } from '../activityTimeline/activityTimelineStore'
import type { UsageRollup } from './usageAggregate'

export type ObservabilityTab = 'usage' | 'automation' | 'vault'

export interface AutomationEntry {
  /** Workflow name — primary key for the list IPC. */
  name: string
  description: string
  triggerType: string
  stepCount: number
}

/** Most recent persisted run for a workflow — projection of
 *  `com.nexus.workflow::run_history` (BL-054 Phase 4 follow-up). */
export interface WorkflowRunRecord {
  /** RFC-3339 UTC timestamp the run finished (success or failure). */
  finishedAt: string
  /** True when the executor reported `success` for the run. */
  success: boolean
  /** True when the workflow's `[condition]` short-circuited. */
  conditionSkipped: boolean
  /** Free-form failure message when `success === false`. */
  error: string | null
}

/** A single file-activity event surfaced from the kernel bus. */
export interface VaultFeedEntry {
  /** Activity entry id; used for de-dup. */
  id: string
  /** RFC-3339 timestamp from the activity entry. */
  timestamp: string
  /** Human-readable summary line — comes verbatim from
   *  `ActivityEntry.prompt` (e.g. "saved raw/notes.md"). */
  prompt: string
  /** Forge-relative paths the entry references. */
  files: string[]
}

interface ObservabilityState {
  activeTab: ObservabilityTab

  // ── Usage ─────────────────────────────────────────────────────────
  usageLoading: boolean
  usageError: string | null
  usageEntries: ActivityEntry[]
  usageRollup: UsageRollup | null

  // ── Automation ────────────────────────────────────────────────────
  automationLoading: boolean
  automationError: string | null
  automationWorkflows: AutomationEntry[]
  /** BL-054 Phase 4 follow-up — last persisted run per workflow
   *  name. Empty for workflows that haven't run yet. */
  automationLastRun: Record<string, WorkflowRunRecord>

  // ── Vault feed ───────────────────────────────────────────────────
  vaultEntries: VaultFeedEntry[]

  setActiveTab(t: ObservabilityTab): void
  setUsageLoading(b: boolean): void
  setUsageError(e: string | null): void
  setUsageData(entries: ActivityEntry[], rollup: UsageRollup): void
  setAutomationLoading(b: boolean): void
  setAutomationError(e: string | null): void
  setAutomations(entries: AutomationEntry[]): void
  setAutomationLastRun(byName: Record<string, WorkflowRunRecord>): void
  prependVault(entry: VaultFeedEntry): void
  reset(): void
}

const VAULT_FEED_CAP = 200

const INITIAL: Pick<
  ObservabilityState,
  | 'activeTab'
  | 'usageLoading'
  | 'usageError'
  | 'usageEntries'
  | 'usageRollup'
  | 'automationLoading'
  | 'automationError'
  | 'automationWorkflows'
  | 'automationLastRun'
  | 'vaultEntries'
> = {
  activeTab: 'usage',
  usageLoading: false,
  usageError: null,
  usageEntries: [],
  usageRollup: null,
  automationLoading: false,
  automationError: null,
  automationWorkflows: [],
  automationLastRun: {},
  vaultEntries: [],
}

export const useObservabilityStore = create<ObservabilityState>((set) => ({
  ...INITIAL,

  setActiveTab: (t) => set({ activeTab: t }),
  setUsageLoading: (b) => set({ usageLoading: b }),
  setUsageError: (e) => set({ usageError: e }),
  setUsageData: (entries, rollup) =>
    set({ usageLoading: false, usageError: null, usageEntries: entries, usageRollup: rollup }),
  setAutomationLoading: (b) => set({ automationLoading: b }),
  setAutomationError: (e) => set({ automationError: e }),
  setAutomations: (entries) =>
    set({ automationLoading: false, automationError: null, automationWorkflows: entries }),
  setAutomationLastRun: (byName) => set({ automationLastRun: byName }),
  prependVault: (entry) =>
    set((s) => {
      // Dedup by id — the dual-topic publish (BL-052) means a single
      // file event can hit the bus twice. Keep the first one through.
      if (s.vaultEntries.some((e) => e.id === entry.id)) return {}
      const next = [entry, ...s.vaultEntries]
      if (next.length > VAULT_FEED_CAP) next.length = VAULT_FEED_CAP
      return { vaultEntries: next }
    }),
  reset: () => set({ ...INITIAL }),
}))
