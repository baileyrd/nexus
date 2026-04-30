import { create } from 'zustand'
import type { Thread } from './types'

interface CommentsState {
  /** Forge-relative path of the file whose threads we're showing. */
  currentRelpath: string | null
  /** Threads for the current file, in storage order (oldest-first). */
  threads: Thread[]
  /** True while a list call is in flight for the current file. */
  loading: boolean
  /** Human-readable error string when the last load failed, else null. */
  error: string | null

  setCurrent(relpath: string | null): void
  setThreads(ts: Thread[]): void
  /** Replace one thread in place; appended if absent. */
  upsertThread(t: Thread): void
  removeThread(threadId: string): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  /** Reset everything — used on workspace close. */
  clear(): void
}

export const useCommentsStore = create<CommentsState>((set) => ({
  currentRelpath: null,
  threads: [],
  loading: false,
  error: null,
  setCurrent: (currentRelpath) => set({ currentRelpath }),
  setThreads: (threads) => set({ threads }),
  upsertThread: (t) =>
    set((s) => {
      const idx = s.threads.findIndex((x) => x.id === t.id)
      if (idx === -1) return { threads: [...s.threads, t] }
      const next = s.threads.slice()
      next[idx] = t
      return { threads: next }
    }),
  removeThread: (threadId) =>
    set((s) => ({ threads: s.threads.filter((t) => t.id !== threadId) })),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  clear: () =>
    set({ currentRelpath: null, threads: [], loading: false, error: null }),
}))
