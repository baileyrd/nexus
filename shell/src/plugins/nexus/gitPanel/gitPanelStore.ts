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

export type GitPanelTab = 'changes' | 'branches' | 'log'

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

  // ── Branches tab ───────────────────────────────────────────────────
  branches: BranchEntry[]
  loadingBranches: boolean
  newBranchName: string

  // ── Log tab ────────────────────────────────────────────────────────
  logEntries: LogEntry[]
  loadingLog: boolean

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
  setBranches(b: BranchEntry[]): void
  setLoadingBranches(v: boolean): void
  setNewBranchName(name: string): void
  setLogEntries(entries: LogEntry[]): void
  setLoadingLog(v: boolean): void
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
  branches: [],
  loadingBranches: false,
  newBranchName: '',
  logEntries: [],
  loadingLog: false,

  setActiveTab: (tab) => set({ activeTab: tab }),
  setFiles: (files) => set({ files }),
  setLoadingFiles: (v) => set({ loadingFiles: v }),
  setSelectedFile: (path) => set({ selectedFile: path }),
  setSelectedHunks: (hunks) => set({ selectedHunks: hunks }),
  setLoadingDiff: (v) => set({ loadingDiff: v }),
  setCommitMessage: (msg) => set({ commitMessage: msg }),
  setCommitting: (v) => set({ committing: v }),
  setPushAfterCommit: (v) => set({ pushAfterCommit: v }),
  setBranches: (b) => set({ branches: b }),
  setLoadingBranches: (v) => set({ loadingBranches: v }),
  setNewBranchName: (name) => set({ newBranchName: name }),
  setLogEntries: (entries) => set({ logEntries: entries }),
  setLoadingLog: (v) => set({ loadingLog: v }),
  reset: () => set({
    files: [], selectedFile: null, selectedHunks: [],
    branches: [], logEntries: [], commitMessage: '',
    newBranchName: '', committing: false,
  }),
}))
