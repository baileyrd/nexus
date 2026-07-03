// C7 (#360) — pane-mode view for the task dashboard.
//
//   ┌──────────────────────────────────────────────────────────────┐
//   │ Tasks   3 pending, 1 done            [ ] Show completed       │
//   ├──────────────────────────────────────────────────────────────┤
//   │ OVERDUE (1)                                                   │
//   │ [ ] Ship the release          !high   2026-07-01   a.md       │
//   │ TODAY (1)                                                     │
//   │ [ ] Renew certificate         !med    2026-07-03   b.md       │
//   │ NO DATE (1)                                                   │
//   │ [ ] Water the plants                                c.md      │
//   └──────────────────────────────────────────────────────────────┘
//
// Render-only: index.ts's activate() owns hydrate-on-open + the
// files:saved re-fetch subscription.

import { useMemo, useState } from 'react'
import { useTaskDashboardStore } from './taskDashboardStore'
import {
  groupPendingTasks,
  completedTasks,
  todayIso,
  isKnownPriority,
  type TaskEntry,
} from './taskGrouping'
import { getApi } from './taskDashboardRuntime'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const EVENT_FILE_OPEN = 'files:open'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** Toggle a task's completion (optimistic; reverts + toasts on IPC failure). */
async function toggleTask(task: TaskEntry): Promise<void> {
  const api = getApi()
  const next = !task.completed
  useTaskDashboardStore.getState().setCompleted(task.id, next)
  try {
    await api.kernel.invoke(STORAGE_PLUGIN_ID, 'toggle_task', { task_id: task.id })
  } catch (e) {
    useTaskDashboardStore.getState().setCompleted(task.id, task.completed)
    api.notifications.show({ message: `Failed to toggle task: ${String(e)}`, type: 'error' })
  }
}

/** Open the task's source file — mirrors search/index.ts's click-to-open. */
function openTaskFile(task: TaskEntry): void {
  getApi().events.emit(EVENT_FILE_OPEN, {
    relpath: task.file_path,
    name: basename(task.file_path),
  })
}

function priorityColor(priority: string | null): string {
  if (priority === 'high') return 'var(--risk)'
  if (priority === 'medium') return 'var(--warm)'
  if (priority === 'low') return 'var(--cool)'
  return 'var(--text-faint)'
}

function TaskRow({
  task,
  dueTone,
}: {
  task: TaskEntry
  dueTone?: 'overdue' | 'today'
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '4px 12px',
        borderBottom: '1px solid var(--divider-color)',
        fontSize: 12,
        fontFamily: 'var(--font-interface)',
      }}
    >
      <input
        type="checkbox"
        checked={task.completed}
        onChange={() => void toggleTask(task)}
        aria-label={task.completed ? 'Mark task incomplete' : 'Mark task complete'}
      />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          color: task.completed ? 'var(--text-faint)' : 'var(--text-normal)',
          textDecoration: task.completed ? 'line-through' : 'none',
        }}
      >
        {task.content || '(empty task)'}
      </span>
      {isKnownPriority(task.priority) && (
        <span
          style={{
            fontSize: 10,
            color: priorityColor(task.priority),
            border: `1px solid ${priorityColor(task.priority)}`,
            borderRadius: 4,
            padding: '0 4px',
            flexShrink: 0,
          }}
        >
          {task.priority}
        </span>
      )}
      {task.due_date && (
        <span
          style={{
            fontSize: 11,
            flexShrink: 0,
            color:
              dueTone === 'overdue'
                ? 'var(--risk)'
                : dueTone === 'today'
                  ? 'var(--warm)'
                  : 'var(--text-muted)',
          }}
        >
          {task.due_date}
        </span>
      )}
      <button
        type="button"
        onClick={() => openTaskFile(task)}
        title={task.file_path}
        style={{
          fontSize: 11,
          color: 'var(--text-muted)',
          background: 'none',
          border: 'none',
          cursor: 'pointer',
          padding: 0,
          flexShrink: 0,
          maxWidth: 140,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {basename(task.file_path)}
      </button>
    </div>
  )
}

function Section({
  title,
  tasks,
  dueTone,
}: {
  title: string
  tasks: TaskEntry[]
  dueTone?: 'overdue' | 'today'
}) {
  if (tasks.length === 0) return null
  return (
    <div>
      <div
        style={{
          padding: '6px 12px',
          fontSize: 11,
          fontWeight: 600,
          textTransform: 'uppercase',
          letterSpacing: 0.4,
          color: 'var(--text-faint)',
          background: 'var(--background-secondary)',
        }}
      >
        {title} ({tasks.length})
      </div>
      {tasks.map((t) => (
        <TaskRow key={t.id} task={t} dueTone={dueTone} />
      ))}
    </div>
  )
}

export function TaskDashboardView() {
  const hydrated = useTaskDashboardStore((s) => s.hydrated)
  const tasks = useTaskDashboardStore((s) => s.tasks)
  const [showCompleted, setShowCompleted] = useState(false)

  const today = useMemo(() => todayIso(), [])
  const groups = useMemo(() => groupPendingTasks(tasks, today), [tasks, today])
  const done = useMemo(() => completedTasks(tasks), [tasks])
  const pendingCount = tasks.length - done.length

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--background-primary)',
      }}
    >
      <div
        style={{
          flexShrink: 0,
          borderBottom: '1px solid var(--divider-color)',
          display: 'flex',
          alignItems: 'center',
          padding: '6px 12px',
          gap: 8,
        }}
      >
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-normal)' }}>Tasks</span>
        <span style={{ fontSize: 11, color: 'var(--text-faint)' }}>
          {pendingCount} pending, {done.length} done
        </span>
        <div style={{ flex: 1 }} />
        <label
          style={{
            fontSize: 11,
            color: 'var(--text-muted)',
            display: 'flex',
            alignItems: 'center',
            gap: 4,
            cursor: 'pointer',
          }}
        >
          <input
            type="checkbox"
            checked={showCompleted}
            onChange={(e) => setShowCompleted(e.target.checked)}
          />
          Show completed
        </label>
      </div>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {!hydrated ? (
          <div style={{ padding: 16, color: 'var(--text-faint)', fontSize: 12 }}>Loading…</div>
        ) : tasks.length === 0 ? (
          <div style={{ padding: 16, color: 'var(--text-faint)', fontSize: 12 }}>
            No tasks found. Add a checkbox item (<code>- [ ] …</code>) to any note.
          </div>
        ) : (
          <>
            <Section title="Overdue" tasks={groups.overdue} dueTone="overdue" />
            <Section title="Today" tasks={groups.today} dueTone="today" />
            <Section title="Upcoming" tasks={groups.upcoming} />
            <Section title="No date" tasks={groups.noDate} />
            {showCompleted && <Section title="Completed" tasks={done} />}
            {pendingCount === 0 && !showCompleted && done.length > 0 && (
              <div style={{ padding: 16, color: 'var(--text-faint)', fontSize: 12 }}>
                All caught up. {done.length} completed task{done.length === 1 ? '' : 's'} hidden —
                toggle "Show completed" to see them.
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}
