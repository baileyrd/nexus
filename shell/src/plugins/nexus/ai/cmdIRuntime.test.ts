// shell/src/plugins/nexus/ai/cmdIRuntime.test.ts
//
// BL-032 — submit-side coverage for the Cmd+I overlay. We stub
// `api.kernel.invoke` so the flow doesn't reach Tauri / Rust, then
// assert the assembled prompt + the store transitions.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/cmdIRuntime.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  contextContributors,
} from './contextContributors.ts'
import { useCmdIStore } from './cmdIStore.ts'
import {
  isCmdISessionId,
  openCmdI,
  routeStreamEvent,
  submitCmdI,
} from './cmdIRuntime.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: Record<string, unknown>
}

function reset(): void {
  contextContributors._resetForTests()
  useCmdIStore.setState({
    visible: false,
    prompt: '',
    chips: [],
    status: 'idle',
    responseText: '',
    error: null,
    currentRequestId: null,
  })
}

/** Build a stub `PluginAPI` with just enough surface for the runtime
 *  to exercise. The kernel.invoke implementation captures the call so
 *  the test can assert on what was sent. */
function stubApi(invokeImpl: (call: InvokeCall) => Promise<unknown>) {
  const calls: InvokeCall[] = []
  return {
    api: {
      kernel: {
        invoke: async (
          pluginId: string,
          commandId: string,
          args: unknown,
        ) => {
          const call: InvokeCall = {
            pluginId,
            commandId,
            args: args as Record<string, unknown>,
          }
          calls.push(call)
          return invokeImpl(call)
        },
      },
    },
    calls,
  }
}

test('isCmdISessionId: only matches the cmdi- prefix', () => {
  assert.equal(isCmdISessionId('cmdi-abc'), true)
  assert.equal(isCmdISessionId('chat-abc'), false)
  assert.equal(isCmdISessionId(''), false)
})

test('openCmdI: opens overlay and hydrates chips from registered contributors', async () => {
  reset()
  contextContributors.register('editor', () => ({
    surfaceId: 'editor',
    chips: [
      { id: 'editor:file:foo.md', label: 'foo.md', kind: 'file' },
    ],
    promptBlock: 'CONTEXT_BLOCK',
  }))
  await openCmdI()
  const s = useCmdIStore.getState()
  assert.equal(s.visible, true)
  assert.equal(s.chips.length, 1)
  assert.equal(s.chips[0].id, 'editor:file:foo.md')
  assert.equal(s.status, 'idle') // chips arrived → flipped off 'collecting'
})

test('submitCmdI: empty prompt is a no-op', async () => {
  reset()
  useCmdIStore.getState().open()
  useCmdIStore.getState().setPrompt('   ')
  const { api, calls } = stubApi(async () => ({ text: 'never' }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await submitCmdI(api as any)
  assert.equal(result, null)
  assert.equal(calls.length, 0)
})

test('submitCmdI: assembles context, mints a cmdi- session id, calls stream_chat', async () => {
  reset()
  contextContributors.register('editor', () => ({
    surfaceId: 'editor',
    chips: [{ id: 'editor:file:foo.md', label: 'foo.md', kind: 'file' }],
    promptBlock: '### Current file: `foo.md`\n\nfoo body',
  }))
  await openCmdI()
  useCmdIStore.getState().setPrompt('what does this do?')

  const { api, calls } = stubApi(async () => ({
    text: 'It defines foo.',
    session_id: 'echoed',
  }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await submitCmdI(api as any)

  assert.ok(result, 'submit should return assembled prompt')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.ai')
  assert.equal(calls[0].commandId, 'stream_chat')
  const args = calls[0].args as {
    messages: Array<{ role: string; content: string }>
    session_id: string
  }
  assert.equal(args.messages.length, 1)
  assert.equal(args.messages[0].role, 'user')
  // Assembled body must contain BOTH the contributor's block and the
  // user prompt, with the conventional Question header between them.
  assert.match(args.messages[0].content, /### Current file: `foo\.md`/)
  assert.match(args.messages[0].content, /## Question\nwhat does this do\?/)
  assert.ok(
    isCmdISessionId(args.session_id),
    `session_id should carry cmdi- prefix; got ${args.session_id}`,
  )

  // Final response reconciled from the invoke result.
  const s = useCmdIStore.getState()
  assert.equal(s.status, 'done')
  assert.equal(s.responseText, 'It defines foo.')
  assert.equal(s.currentRequestId, null)
})

test('submitCmdI: kernel rejection flips status → error', async () => {
  reset()
  await openCmdI()
  useCmdIStore.getState().setPrompt('go')
  const { api } = stubApi(async () => {
    throw new Error('provider unavailable')
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await submitCmdI(api as any)
  const s = useCmdIStore.getState()
  assert.equal(s.status, 'error')
  assert.equal(s.error?.message, 'provider unavailable')
})

test('submitCmdI: single-flight — call while submitting is a no-op', async () => {
  reset()
  await openCmdI()
  useCmdIStore.getState().setPrompt('q')
  // Pre-stage the store as if another submit were already in flight.
  // Avoids racing the runtime's own async `contextContributors.collect`
  // against itself, which would let two concurrent `submitCmdI` calls
  // both clear the single-flight check before either reaches
  // `beginSubmit`.
  useCmdIStore.getState().beginSubmit('cmdi-existing')

  const { api, calls } = stubApi(async () => ({ text: 'unused' }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await submitCmdI(api as any)
  assert.equal(result, null)
  assert.equal(calls.length, 0)

  // Drain the simulated in-flight request so the watchdog timer
  // (60s wall-clock) doesn't outlive this test and trip later ones.
  useCmdIStore.getState().finishResponse('cmdi-existing', '')
})

test('routeStreamEvent: only claims cmdi- session ids', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-claim-me')

  // Foreign session — must be rejected.
  const claimedForeign = routeStreamEvent('com.nexus.ai.stream_chunk', {
    session_id: 'chat-other',
    chunk: 'noise',
  })
  assert.equal(claimedForeign, false)
  assert.equal(useCmdIStore.getState().responseText, '')

  // Own session — must land + return true.
  const claimedOwn = routeStreamEvent('com.nexus.ai.stream_chunk', {
    session_id: 'cmdi-claim-me',
    chunk: 'hello',
  })
  assert.equal(claimedOwn, true)
  assert.equal(useCmdIStore.getState().responseText, 'hello')

  routeStreamEvent('com.nexus.ai.stream_done', {
    session_id: 'cmdi-claim-me',
    text: 'final',
  })
  assert.equal(useCmdIStore.getState().status, 'done')
  assert.equal(useCmdIStore.getState().responseText, 'final')
})

test('routeStreamEvent: malformed payloads are ignored', () => {
  reset()
  assert.equal(routeStreamEvent('com.nexus.ai.stream_chunk', null), false)
  assert.equal(routeStreamEvent('com.nexus.ai.stream_chunk', {}), false)
  assert.equal(
    routeStreamEvent('com.nexus.ai.stream_chunk', { session_id: 42 }),
    false,
  )
})
