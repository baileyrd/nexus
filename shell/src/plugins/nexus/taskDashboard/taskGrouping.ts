// C7 (#360) — pure decode + grouping logic for the task dashboard, kept
// separate from index.ts/TaskDashboardView.tsx so it's testable without
// mocking the plugin/modal stack (mirrors memoryDashboard/index.ts's
// decodeMemories, crdtConflict/applyResolution.ts's pattern).

import { isoDate } from '../bases/dateUtils'

/** Wire shape of a `com.nexus.storage::query_tasks` row (`TaskRecord`). */
export interface TaskEntry {
  id: number
  file_id: number
  file_path: string
  content: string
  completed: boolean
  line_number: number
  due_date: string | null
  priority: string | null
  created_at: number
  updated_at: number
}

/** Coerce a `query_tasks` response (a bare JSON array of task rows). */
export function decodeTasks(raw: unknown): TaskEntry[] {
  if (!Array.isArray(raw)) return []
  const out: TaskEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    if (typeof r.id !== 'number' || typeof r.file_path !== 'string') continue
    out.push({
      id: r.id,
      file_id: typeof r.file_id === 'number' ? r.file_id : 0,
      file_path: r.file_path,
      content: typeof r.content === 'string' ? r.content : '',
      completed: r.completed === true,
      line_number: typeof r.line_number === 'number' ? r.line_number : 0,
      due_date: typeof r.due_date === 'string' ? r.due_date : null,
      priority: typeof r.priority === 'string' ? r.priority : null,
      created_at: typeof r.created_at === 'number' ? r.created_at : 0,
      updated_at: typeof r.updated_at === 'number' ? r.updated_at : 0,
    })
  }
  return out
}

/** Today's date in `YYYY-MM-DD` form, local time. */
export function todayIso(): string {
  return isoDate(new Date())
}

export interface GroupedTasks {
  overdue: TaskEntry[]
  today: TaskEntry[]
  upcoming: TaskEntry[]
  noDate: TaskEntry[]
}

/**
 * Bucket the *pending* (not completed) tasks in `tasks` by due date
 * relative to `todayIsoDate` (`YYYY-MM-DD` — string comparison is safe
 * since both sides are zero-padded ISO dates). Each bucket is sorted by
 * due date then file path; `today`/`noDate` share one due date (or none)
 * per bucket, so they sort by file path alone.
 */
export function groupPendingTasks(tasks: TaskEntry[], todayIsoDate: string): GroupedTasks {
  const groups: GroupedTasks = { overdue: [], today: [], upcoming: [], noDate: [] }
  for (const t of tasks) {
    if (t.completed) continue
    if (!t.due_date) {
      groups.noDate.push(t)
    } else if (t.due_date < todayIsoDate) {
      groups.overdue.push(t)
    } else if (t.due_date === todayIsoDate) {
      groups.today.push(t)
    } else {
      groups.upcoming.push(t)
    }
  }
  const byDueThenPath = (a: TaskEntry, b: TaskEntry): number =>
    (a.due_date ?? '').localeCompare(b.due_date ?? '') || a.file_path.localeCompare(b.file_path)
  const byPath = (a: TaskEntry, b: TaskEntry): number => a.file_path.localeCompare(b.file_path)
  groups.overdue.sort(byDueThenPath)
  groups.upcoming.sort(byDueThenPath)
  groups.today.sort(byPath)
  groups.noDate.sort(byPath)
  return groups
}

/** Completed tasks, newest-updated first. */
export function completedTasks(tasks: TaskEntry[]): TaskEntry[] {
  return tasks.filter((t) => t.completed).sort((a, b) => b.updated_at - a.updated_at)
}

const PRIORITY_RANK: Record<string, number> = { high: 0, medium: 1, low: 2 }

/** `true` when `priority` is one of the three recognized levels. */
export function isKnownPriority(priority: string | null): priority is 'high' | 'medium' | 'low' {
  return priority !== null && priority in PRIORITY_RANK
}
