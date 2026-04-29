// shell/src/plugins/nexus/ai/actions/builtins.test.ts
//
// BL-035 — sanity coverage for the four shipped built-in AI actions.
// We exercise registration shape (id + surfaces) and that `run`
// dispatches to `com.nexus.ai::stream_chat` with `tools: 'auto'`. The
// kernel side is stubbed via `setKernel` + a fake `api.kernel.invoke`
// — no live kernel.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/actions/builtins.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { AiActionEditorSelectionContext } from '@nexus/extension-api'
import {
  ACTION_ID_EXPLAIN,
  ACTION_ID_REWRITE,
  ACTION_ID_SUMMARIZE,
  ACTION_ID_TRANSLATE,
  buildBuiltinAiActions,
  registerBuiltinAiActions,
} from './builtins.ts'
import { aiActionRegistry } from './registry.ts'
import { setKernel } from '../aiRuntime.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
}

function stubApi(reply: string): {
  api: Parameters<typeof registerBuiltinAiActions>[0]
  calls: InvokeCall[]
} {
  const calls: InvokeCall[] = []
  const kernel = {
    invoke: async (pluginId: string, commandId: string, args: unknown) => {
      calls.push({ pluginId, commandId, args })
      return { text: reply, session_id: 'stub' }
    },
    on: async () => () => {},
    off: async () => {},
    available: () => true,
  }
  const api = {
    kernel,
    notifications: {
      show: () => {},
    },
  } as unknown as Parameters<typeof registerBuiltinAiActions>[0]
  setKernel(kernel as Parameters<typeof setKernel>[0])
  return { api, calls }
}

const SAMPLE_CTX: AiActionEditorSelectionContext = {
  surface: 'editor.selection',
  relpath: 'note.md',
  selection: 'The quick brown fox jumps over the lazy dog.',
  selectionRange: { from: 0, to: 44 },
}

test('built-ins: ships exactly four actions with stable ids', () => {
  const { api } = stubApi('ok')
  const actions = buildBuiltinAiActions(api)
  assert.equal(actions.length, 4)
  assert.deepEqual(
    actions.map((a) => a.id),
    [
      ACTION_ID_SUMMARIZE,
      ACTION_ID_REWRITE,
      ACTION_ID_TRANSLATE,
      ACTION_ID_EXPLAIN,
    ],
  )
})

test('built-ins: every action surfaces on editor.selection AND block', () => {
  const { api } = stubApi('ok')
  const actions = buildBuiltinAiActions(api)
  for (const a of actions) {
    assert.ok(
      a.surfaces.includes('editor.selection'),
      `${a.id} missing editor.selection`,
    )
    assert.ok(a.surfaces.includes('block'), `${a.id} missing block`)
  }
})

test('built-ins: translate honours the targetLanguage option', () => {
  const { api } = stubApi('ok')
  const french = buildBuiltinAiActions(api, { targetLanguage: 'French' })
  const tr = french.find((a) => a.id === ACTION_ID_TRANSLATE)
  assert.ok(tr)
  assert.match(tr!.label, /French/)
})

test('built-ins: run dispatches stream_chat with tools=auto', async () => {
  const { api, calls } = stubApi('SUMMARY')
  const actions = buildBuiltinAiActions(api)
  const summarize = actions.find((a) => a.id === ACTION_ID_SUMMARIZE)
  assert.ok(summarize)
  await summarize!.run(SAMPLE_CTX)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.ai')
  assert.equal(calls[0].commandId, 'stream_chat')
  const args = calls[0].args as Record<string, unknown>
  assert.equal(args.tools, 'auto')
  assert.ok(typeof args.system === 'string')
  assert.ok(Array.isArray(args.messages))
})

test('registerBuiltinAiActions: registers and disposer sweeps all four', () => {
  aiActionRegistry._resetForTests()
  const { api } = stubApi('ok')
  const dispose = registerBuiltinAiActions(api)
  assert.equal(aiActionRegistry.list().length, 4)
  dispose()
  assert.equal(aiActionRegistry.list().length, 0)
  // Idempotent.
  dispose()
})
