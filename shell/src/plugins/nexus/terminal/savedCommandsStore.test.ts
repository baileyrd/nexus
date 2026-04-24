// shell/src/plugins/nexus/terminal/savedCommandsStore.test.ts
//
// WI-05 unit tests for the saved-commands store. We exercise each of
// the public actions (load / create / update / delete / reorder) with a
// mocked kernel that records every invoke + drives the response.
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/terminal/savedCommandsStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  useSavedCommandsStore,
  type SavedCommand,
  type SavedKernelAPI,
} from './savedCommandsStore.ts'

interface InvokeCall {
  pluginId: string
  command: string
  args: unknown
}

/** Minimal SavedKernelAPI mock: caller stages a queue of responses
 *  per (pluginId, command) and we record every call. */
function makeKernel(): {
  api: SavedKernelAPI
  calls: InvokeCall[]
  /** Programmatic response queue keyed by command id. Each call to
   *  invoke(plugin, cmd, …) shifts the head off `responses[cmd]`.
   *  Missing entries fall through to the `defaults` table. */
  responses: Record<string, unknown[]>
  defaults: Record<string, unknown>
} {
  const calls: InvokeCall[] = []
  const responses: Record<string, unknown[]> = {}
  const defaults: Record<string, unknown> = {
    saved_create: {},
    saved_update: {},
    saved_delete: {},
    saved_reorder: {},
  }
  const api: SavedKernelAPI = {
    invoke: async <T = unknown>(
      pluginId: string,
      command: string,
      args?: unknown,
    ): Promise<T> => {
      calls.push({ pluginId, command, args })
      const queue = responses[command]
      if (queue && queue.length > 0) {
        return queue.shift() as T
      }
      return (defaults[command] ?? null) as T
    },
  }
  return { api, calls, responses, defaults }
}

function row(slug: string, name = slug): SavedCommand {
  return {
    slug,
    name,
    shell: '/bin/bash',
    shell_cmd: `echo ${slug}`,
    working_dir: null,
    env_vars: {},
    env_file: null,
    icon: 'terminal',
    auto_restart: false,
    auto_restart_delay_ms: 2_000,
    memory_limit_mb: null,
    sidebar_order: null,
    pre_commands: [],
    created_at: 0,
    updated_at: 0,
  }
}

function reset(): void {
  useSavedCommandsStore.getState().reset()
}

// ── loadSaved ────────────────────────────────────────────────────────────────

test('loadSaved: populates cache from kernel saved_list response', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  responses['saved_list'] = [[row('a'), row('b')]]

  await useSavedCommandsStore.getState().loadSaved(api)

  const after = useSavedCommandsStore.getState()
  assert.equal(after.loaded, true)
  assert.equal(after.commands.length, 2)
  assert.equal(after.commands[0].slug, 'a')
  assert.equal(after.commands[1].slug, 'b')
  assert.equal(after.error, null)

  // One round-trip, correct plugin + command id.
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.terminal')
  assert.equal(calls[0].command, 'saved_list')
})

test('loadSaved: kernel error stored as `error`, cache untouched', async () => {
  reset()
  const api: SavedKernelAPI = {
    invoke: async () => {
      throw new Error('boom')
    },
  }
  await useSavedCommandsStore.getState().loadSaved(api)
  const after = useSavedCommandsStore.getState()
  assert.equal(after.loaded, false, 'failed load must NOT flip the loaded flag')
  assert.equal(after.commands.length, 0)
  assert.match(after.error ?? '', /boom/)
})

// ── createSaved ──────────────────────────────────────────────────────────────

test('createSaved: posts saved_create then refreshes via saved_list', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  // Create returns the row; refresh load returns the canonical list.
  responses['saved_list'] = [[row('build')]]

  await useSavedCommandsStore.getState().createSaved(api, {
    slug: 'build',
    name: 'Build',
    shell: '/bin/bash',
    shell_cmd: 'npm run build',
    working_dir: null,
    icon: 'terminal',
  })

  const after = useSavedCommandsStore.getState()
  assert.equal(after.commands.length, 1)
  assert.equal(after.commands[0].slug, 'build')

  // Two calls: create then list.
  assert.equal(calls.length, 2)
  assert.equal(calls[0].command, 'saved_create')
  const createArgs = calls[0].args as SavedCommand
  assert.equal(createArgs.slug, 'build')
  assert.equal(createArgs.name, 'Build')
  assert.equal(createArgs.shell_cmd, 'npm run build')
  // Defaults filled in (handler deserialises into SavedCommand directly).
  assert.equal(createArgs.auto_restart, false)
  assert.equal(createArgs.icon, 'terminal')
  assert.deepEqual(createArgs.env_vars, {})
  assert.deepEqual(createArgs.pre_commands, [])
  assert.equal(calls[1].command, 'saved_list')
})

// ── updateSaved ──────────────────────────────────────────────────────────────

test('updateSaved: merges draft onto cached row, preserves env + auto_restart', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  // Seed cache via load.
  responses['saved_list'] = [
    [
      {
        ...row('build', 'Build'),
        env_vars: { CI: '1' },
        auto_restart: true,
        auto_restart_delay_ms: 5_000,
        sidebar_order: 0,
      },
    ],
    // Second list call after the update — same row with new name.
    [{ ...row('build', 'Build (renamed)'), env_vars: { CI: '1' } }],
  ]
  await useSavedCommandsStore.getState().loadSaved(api)

  await useSavedCommandsStore.getState().updateSaved(api, {
    slug: 'build',
    name: 'Build (renamed)',
    shell: '/bin/bash',
    shell_cmd: 'npm run build',
    working_dir: null,
    icon: 'terminal',
  })

  const after = useSavedCommandsStore.getState()
  assert.equal(after.commands[0].name, 'Build (renamed)')

  // The update payload must carry forward env_vars + auto_restart from
  // the cached row — they're not editable from the form but must round-
  // trip through saved_update intact.
  const updateCall = calls.find((c) => c.command === 'saved_update')
  if (!updateCall) throw new Error('saved_update must have been called')
  const args = updateCall.args as SavedCommand
  assert.equal(args.slug, 'build')
  assert.equal(args.name, 'Build (renamed)')
  assert.deepEqual(args.env_vars, { CI: '1' })
  assert.equal(args.auto_restart, true)
  assert.equal(args.auto_restart_delay_ms, 5_000)
  assert.equal(args.sidebar_order, 0)
})

// ── deleteSaved ──────────────────────────────────────────────────────────────

test('deleteSaved: optimistically prunes then refreshes from kernel', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  responses['saved_list'] = [[row('a'), row('b')], [row('b')]]
  await useSavedCommandsStore.getState().loadSaved(api)
  assert.equal(useSavedCommandsStore.getState().commands.length, 2)

  await useSavedCommandsStore.getState().deleteSaved(api, 'a')

  const after = useSavedCommandsStore.getState()
  assert.equal(after.commands.length, 1)
  assert.equal(after.commands[0].slug, 'b')

  const deleteCall = calls.find((c) => c.command === 'saved_delete')
  if (!deleteCall) throw new Error('saved_delete must have been called')
  assert.deepEqual(deleteCall.args, { slug: 'a' })
})

// ── reorderSaved ─────────────────────────────────────────────────────────────

test('reorderSaved: sends one saved_reorder per slug with dense indices, then reloads', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  responses['saved_list'] = [
    [row('a'), row('b'), row('c')],
    // After reorder the kernel returns rows ordered by sidebar_order.
    [
      { ...row('c'), sidebar_order: 0 },
      { ...row('a'), sidebar_order: 1 },
      { ...row('b'), sidebar_order: 2 },
    ],
  ]
  await useSavedCommandsStore.getState().loadSaved(api)

  await useSavedCommandsStore.getState().reorderSaved(api, ['c', 'a', 'b'])

  const reorderCalls = calls.filter((c) => c.command === 'saved_reorder')
  assert.equal(reorderCalls.length, 3, 'one reorder call per slug')
  assert.deepEqual(reorderCalls[0].args, { slug: 'c', sidebar_order: 0 })
  assert.deepEqual(reorderCalls[1].args, { slug: 'a', sidebar_order: 1 })
  assert.deepEqual(reorderCalls[2].args, { slug: 'b', sidebar_order: 2 })

  // Cache reflects the new order from the post-reorder list response.
  const after = useSavedCommandsStore.getState()
  assert.deepEqual(
    after.commands.map((c) => c.slug),
    ['c', 'a', 'b'],
  )
})

// ── reset ────────────────────────────────────────────────────────────────────

test('reset: clears cache + loaded flag (workspace:closed contract)', async () => {
  reset()
  const { api, responses } = makeKernel()
  responses['saved_list'] = [[row('a')]]
  await useSavedCommandsStore.getState().loadSaved(api)
  assert.equal(useSavedCommandsStore.getState().loaded, true)

  useSavedCommandsStore.getState().reset()

  const after = useSavedCommandsStore.getState()
  assert.equal(after.loaded, false)
  assert.equal(after.commands.length, 0)
  assert.equal(after.error, null)
})
