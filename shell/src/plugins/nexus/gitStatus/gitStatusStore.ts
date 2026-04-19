import { create } from 'zustand'

export interface GitStatus {
  branch: string | null
  shortSha: string | null
  dirty: boolean
}

interface GitStatusState {
  /** Current git state, or null when workspace is not a git repo / no workspace open / load failed. */
  status: GitStatus | null
  setStatus: (status: GitStatus | null) => void
}

export const useGitStatusStore = create<GitStatusState>((set) => ({
  status: null,
  setStatus: (status) => set({ status }),
}))
