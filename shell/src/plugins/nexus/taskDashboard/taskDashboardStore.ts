// C7 (#360) — task dashboard store. Mirrors activityTimelineStore.ts's
// shape (hydrated | tasks) but without a live-append bus topic: tasks
// change via toggle (optimistic, in-place) or a full re-fetch on
// `files:saved` (see index.ts), not an incremental stream.

import { create } from 'zustand'
import type { TaskEntry } from './taskGrouping'

interface TaskDashboardState {
  hydrated: boolean
  tasks: TaskEntry[]
  hydrate(tasks: TaskEntry[]): void
  setCompleted(id: number, completed: boolean): void
}

export const useTaskDashboardStore = create<TaskDashboardState>((set) => ({
  hydrated: false,
  tasks: [],
  hydrate(tasks) {
    set({ hydrated: true, tasks })
  },
  setCompleted(id, completed) {
    set((s) => ({
      tasks: s.tasks.map((t) => (t.id === id ? { ...t, completed } : t)),
    }))
  },
}))
