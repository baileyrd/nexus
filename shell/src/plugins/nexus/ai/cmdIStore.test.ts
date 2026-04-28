// shell/src/plugins/nexus/ai/cmdIStore.test.ts
//
// BL-032 — store transitions for the Cmd+I overlay. Mirrors the
// `aiStore.test.ts` shape (sibling file, node:test).
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/cmdIStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { useCmdIStore } from './cmdIStore.ts'

function reset(): void {
  // No formal reset action; close + tear down is enough between tests.
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

test('open(): clears prior state and flips visible + collecting', () => {
  reset()
  useCmdIStore.setState({
    prompt: 'leftover',
    responseText: 'old answer',
    error: new Error('old'),
  })
  useCmdIStore.getState().open()
  const s = useCmdIStore.getState()
  assert.equal(s.visible, true)
  assert.equal(s.prompt, '')
  assert.equal(s.responseText, '')
  assert.equal(s.error, null)
  assert.equal(s.status, 'collecting')
})

test('setChips: flips status off `collecting` once chips arrive', () => {
  reset()
  useCmdIStore.getState().open()
  assert.equal(useCmdIStore.getState().status, 'collecting')
  useCmdIStore
    .getState()
    .setChips([{ id: 'a', label: 'A', kind: 'file' }])
  assert.equal(useCmdIStore.getState().status, 'idle')
  assert.equal(useCmdIStore.getState().chips.length, 1)
})

test('beginSubmit / appendResponseChunk / finishResponse: streaming roundtrip', () => {
  reset()
  useCmdIStore.getState().open()
  useCmdIStore.getState().setChips([])
  useCmdIStore.getState().beginSubmit('cmdi-1')
  let s = useCmdIStore.getState()
  assert.equal(s.status, 'submitting')
  assert.equal(s.currentRequestId, 'cmdi-1')

  useCmdIStore.getState().appendResponseChunk('cmdi-1', 'hello ')
  useCmdIStore.getState().appendResponseChunk('cmdi-1', 'world')
  s = useCmdIStore.getState()
  assert.equal(s.status, 'streaming')
  assert.equal(s.responseText, 'hello world')

  useCmdIStore.getState().finishResponse('cmdi-1', 'final answer')
  s = useCmdIStore.getState()
  assert.equal(s.status, 'done')
  assert.equal(s.responseText, 'final answer')
  assert.equal(s.currentRequestId, null)
})

test('appendResponseChunk: stale request id is dropped', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-1')
  useCmdIStore.getState().appendResponseChunk('cmdi-OTHER', 'noise')
  assert.equal(useCmdIStore.getState().responseText, '')
})

test('finishResponse: stale request id is dropped', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-1')
  useCmdIStore.getState().appendResponseChunk('cmdi-1', 'partial')
  useCmdIStore.getState().finishResponse('cmdi-OTHER', 'wrong')
  const s = useCmdIStore.getState()
  // partial preserved; no transition to done
  assert.equal(s.status, 'streaming')
  assert.equal(s.responseText, 'partial')
  assert.equal(s.currentRequestId, 'cmdi-1')
})

test('finishResponse: empty final text falls back to streamed body', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-1')
  useCmdIStore.getState().appendResponseChunk('cmdi-1', 'streamed')
  useCmdIStore.getState().finishResponse('cmdi-1', '')
  assert.equal(useCmdIStore.getState().responseText, 'streamed')
  assert.equal(useCmdIStore.getState().status, 'done')
})

test('setError: errors win and clear the in-flight request id', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-1')
  useCmdIStore.getState().setError(new Error('boom'))
  const s = useCmdIStore.getState()
  assert.equal(s.status, 'error')
  assert.equal(s.currentRequestId, null)
  assert.equal(s.error?.message, 'boom')
})

test('close(): drops currentRequestId so a tail chunk no longer lands', () => {
  reset()
  useCmdIStore.getState().beginSubmit('cmdi-1')
  useCmdIStore.getState().close()
  // Now a chunk for that id should be dropped.
  useCmdIStore.getState().appendResponseChunk('cmdi-1', 'late')
  assert.equal(useCmdIStore.getState().responseText, '')
})
