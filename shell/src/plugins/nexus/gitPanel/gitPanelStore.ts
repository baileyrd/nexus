import { create } from 'zustand'

export interface GitFileEntry {
  path: string
  /** Stringified FileStatus from the Rust engine: "Staged", "Added", "Modified",
   *  "Untracked", "Removed", "Renamed", or "Conflicted". */
  status: string
}

export interface DiffLine {
  kind: string   // "Added" | "Removed" | "Context"
  content: string
}

export interface DiffHunk {
  old_start: number
  old_count: number
  new_start: number
  new_count: number
  lines: DiffLine[]
}

export interface GitFileDiff {
  path: string
  hunks: DiffHunk[]
}

export interface BranchEntry {
  name: string
  is_head: boolean
  upstream?: string
}

export interface LogEntry {
  hash: string
  author: string
  date: string
  message: string
  parents: string[]
}

export interface StashEntry {
  index: number
  message: string
  oid: string
}

export type GitPanelTab = 'changes' | 'branches' | 'log'

/**
 * BL-084: state for the conflict-resolution flow. Populated when a
 * conflicted file is selected; cleared on `reset()` and when the user
 * navigates away or finishes resolving.
 */
export interface ConflictState {
  /** Current working-tree contents of the selected conflicted file. */
  content: string | null
  /** `true` while a write is in flight. */
  saving: boolean
  /** Most recent error from a load / write, if any. */
  error: string | null
}

interface GitPanelState {
  activeTab: GitPanelTab

  // ── Changes tab ────────────────────────────────────────────────────
  files: GitFileEntry[]
  loadingFiles: boolean
  selectedFile: string | null
  selectedHunks: DiffHunk[]
  loadingDiff: boolean
  commitMessage: string
  committing: boolean
  pushAfterCommit: boolean

  // C49 (#425) — pull is repo-wide, not scoped to the Changes tab, but
  // lives alongside the other commit-area async-action flags for
  // consistency.
  pulling: boolean
  pullError: string | null

  // ── Branches tab ───────────────────────────────────────────────────
  branches: BranchEntry[]
  loadingBranches: boolean
  newBranchName: string

  // ── Log tab ────────────────────────────────────────────────────────
  logEntries: LogEntry[]
  loadingLog: boolean

  // ── Stash ──────────────────────────────────────────────────────────
  stashEntries: StashEntry[]
  loadingStash: boolean

  // ── Conflict resolution (BL-084) ───────────────────────────────────
  conflict: ConflictState

  // ── Actions ────────────────────────────────────────────────────────
  setActiveTab(tab: GitPanelTab): void
  setFiles(files: GitFileEntry[]): void
  setLoadingFiles(v: boolean): void
  setSelectedFile(path: string | null): void
  setSelectedHunks(hunks: DiffHunk[]): void
  setLoadingDiff(v: boolean): void
  setCommitMessage(msg: string): void
  setCommitting(v: boolean): void
  setPushAfterCommit(v: boolean): void
  setPulling(v: boolean): void
  setPullError(v: string | null): void
  setBranches(b: BranchEntry[]): void
  setLoadingBranches(v: boolean): void
  setNewBranchName(name: string): void
  setLogEntries(entries: LogEntry[]): void
  setLoadingLog(v: boolean): void
  setStashEntries(entries: StashEntry[]): void
  setLoadingStash(v: boolean): void
  setConflict(v: Partial<ConflictState>): void
  resetConflict(): void
  reset(): void
}

export const useGitPanelStore = create<GitPanelState>((set) => ({
  activeTab: 'changes',
  files: [],
  loadingFiles: false,
  selectedFile: null,
  selectedHunks: [],
  loadingDiff: false,
  commitMessage: '',
  committing: false,
  pushAfterCommit: false,
  pulling: false,
  pullError: null,
  branches: [],
  loadingBranches: false,
  newBranchName: '',
  logEntries: [],
  loadingLog: false,
  stashEntries: [],
  loadingStash: false,
  conflict: { content: null, saving: false, error: null },

  setActiveTab: (tab) => set({ activeTab: tab }),
  setFiles: (files) => set({ files }),
  setLoadingFiles: (v) => set({ loadingFiles: v }),
  setSelectedFile: (path) => set({ selectedFile: path }),
  setSelectedHunks: (hunks) => set({ selectedHunks: hunks }),
  setLoadingDiff: (v) => set({ loadingDiff: v }),
  setCommitMessage: (msg) => set({ commitMessage: msg }),
  setCommitting: (v) => set({ committing: v }),
  setPushAfterCommit: (v) => set({ pushAfterCommit: v }),
  setPulling: (v) => set({ pulling: v }),
  setPullError: (v) => set({ pullError: v }),
  setBranches: (b) => set({ branches: b }),
  setLoadingBranches: (v) => set({ loadingBranches: v }),
  setNewBranchName: (name) => set({ newBranchName: name }),
  setLogEntries: (entries) => set({ logEntries: entries }),
  setLoadingLog: (v) => set({ loadingLog: v }),
  setStashEntries: (entries) => set({ stashEntries: entries }),
  setLoadingStash: (v) => set({ loadingStash: v }),
  setConflict: (v) => set((s) => ({ conflict: { ...s.conflict, ...v } })),
  resetConflict: () => set({
    conflict: { content: null, saving: false, error: null },
  }),
  reset: () => set({
    files: [], selectedFile: null, selectedHunks: [],
    branches: [], logEntries: [], commitMessage: '',
    newBranchName: '', committing: false,
    pulling: false, pullError: null,
    conflict: { content: null, saving: false, error: null },
  }),
}))
