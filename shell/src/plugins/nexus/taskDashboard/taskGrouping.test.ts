import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  decodeTasks,
  groupPendingTasks,
  completedTasks,
  isKnownPriority,
  type TaskEntry,
} from './taskGrouping'

function task(overrides: Partial<TaskEntry>): TaskEntry {
  return {
    id: 1,
    file_id: 1,
    file_path: 'notes/a.md',
    content: 'do the thing',
    completed: false,
    line_number: 1,
    due_date: null,
    priority: null,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  }
}

test('decodeTasks decodes a well-formed array of task rows', () => {
  const rows = decodeTasks([
    {
      id: 1,
      file_id: 2,
      file_path: 'notes/a.md',
      content: 'Ship it',
      completed: false,
      line_number: 3,
      due_date: '2026-07-04',
      priority: 'high',
      created_at: 100,
      updated_at: 200,
    },
  ])
  assert.strictEqual(rows.length, 1)
  assert.deepStrictEqual(rows[0], {
    id: 1,
    file_id: 2,
    file_path: 'notes/a.md',
    content: 'Ship it',
    completed: false,
    line_number: 3,
    due_date: '2026-07-04',
    priority: 'high',
    created_at: 100,
    updated_at: 200,
  })
})

test('decodeTasks defaults missing optional fields and drops rows without an id/path', () => {
  const rows = decodeTasks([
    { id: 1, file_path: 'a.md' },
    { file_path: 'no-id.md' },
    { id: 2 },
    'not an object',
    null,
  ])
  assert.strictEqual(rows.length, 1)
  assert.strictEqual(rows[0]?.due_date, null)
  assert.strictEqual(rows[0]?.priority, null)
  assert.strictEqual(rows[0]?.completed, false)
})

test('decodeTasks tolerates non-array input', () => {
  assert.deepStrictEqual(decodeTasks(null), [])
  assert.deepStrictEqual(decodeTasks({ tasks: [] }), [])
})

test('groupPendingTasks buckets by due date relative to today, excluding completed', () => {
  const tasks = [
    task({ id: 1, due_date: '2026-07-01', content: 'overdue' }),
    task({ id: 2, due_date: '2026-07-03', content: 'today' }),
    task({ id: 3, due_date: '2026-07-10', content: 'upcoming' }),
    task({ id: 4, due_date: null, content: 'no date' }),
    task({ id: 5, due_date: '2026-07-01', completed: true, content: 'done, excluded' }),
  ]
  const groups = groupPendingTasks(tasks, '2026-07-03')
  assert.strictEqual(groups.overdue.length, 1)
  assert.strictEqual(groups.overdue[0]?.content, 'overdue')
  assert.strictEqual(groups.today.length, 1)
  assert.strictEqual(groups.today[0]?.content, 'today')
  assert.strictEqual(groups.upcoming.length, 1)
  assert.strictEqual(groups.upcoming[0]?.content, 'upcoming')
  assert.strictEqual(groups.noDate.length, 1)
  assert.strictEqual(groups.noDate[0]?.content, 'no date')
})

test('groupPendingTasks sorts overdue/upcoming by due date then file path', () => {
  const tasks = [
    task({ id: 1, due_date: '2026-07-02', file_path: 'z.md' }),
    task({ id: 2, due_date: '2026-07-01', file_path: 'b.md' }),
    task({ id: 3, due_date: '2026-07-01', file_path: 'a.md' }),
  ]
  const groups = groupPendingTasks(tasks, '2026-07-10')
  assert.deepStrictEqual(
    groups.overdue.map((t) => t.id),
    [3, 2, 1],
  )
})

test('completedTasks returns only completed rows, newest-updated first', () => {
  const tasks = [
    task({ id: 1, completed: true, updated_at: 100 }),
    task({ id: 2, completed: false, updated_at: 300 }),
    task({ id: 3, completed: true, updated_at: 200 }),
  ]
  const done = completedTasks(tasks)
  assert.deepStrictEqual(
    done.map((t) => t.id),
    [3, 1],
  )
})

test('isKnownPriority recognizes exactly high/medium/low', () => {
  assert.strictEqual(isKnownPriority('high'), true)
  assert.strictEqual(isKnownPriority('medium'), true)
  assert.strictEqual(isKnownPriority('low'), true)
  assert.strictEqual(isKnownPriority('urgent'), false)
  assert.strictEqual(isKnownPriority(null), false)
})
