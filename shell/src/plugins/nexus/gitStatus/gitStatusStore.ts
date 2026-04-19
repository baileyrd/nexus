import { create } from 'zustand'

/**
 * Current git state as returned by `com.nexus.git::status`. The kernel's
 * git plugin serializes state with `serde_json::json!` literal keys (no
 * camelCase rename), so field names match the Rust shape exactly:
 *
 *   { branch: Option<String>, head: String, is_dirty: bool, repo_state: String }
 *
 * `head` is a 7-char short SHA, or the sentinel `"(none)"` when the repo
 * has no commits yet. `repo_state` is the Debug-formatted variant name
 * (`"Clean"`, `"Merge"`, `"Rebase"`, etc.) — we don't render it today,
 * but it's surfaced so consumers can react to in-progress merges /
 * rebases without another round-trip.
 */
export interface GitStatus {
  branch: string | null
  head: string
  is_dirty: boolean
  repo_state: string
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
