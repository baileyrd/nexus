// shell/src/plugins/nexus/workflow/workflowStore.test.ts
//
// WI-09 unit tests for the workflow store's validate-panel actions.
// The validate path is wired into the kernel from `index.ts`; the
// store itself only owns the textarea state and the verdict pill, so
// these tests exercise the pure reducers without an IPC mock.
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/workflow/workflowStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { useWorkflowStore } from './workflowStore.ts'

function freshStore() {
  useWorkflowStore.getState().reset()
  return useWorkflowStore
}

test('validate state starts idle with empty text', () => {
  const store = freshStore()
  const v = store.getState().validate
  assert.equal(v.status, 'idle')
  assert.equal(v.text, '')
  assert.equal(v.error, null)
  assert.equal(v.validatedName, null)
})

test('setValidateText preserves text and clears prior verdict', () => {
  const store = freshStore()
  // Simulate a prior failure verdict.
  store.getState().setValidateStatus('error', { error: 'bad' })
  assert.equal(store.getState().validate.status, 'error')
  store.getState().setValidateText('[workflow]\nname = "X"')
  const v = store.getState().validate
  assert.equal(v.text, '[workflow]\nname = "X"')
  assert.equal(v.status, 'idle', 'editing text returns the panel to idle')
  assert.equal(v.error, null)
  assert.equal(v.validatedName, null)
})

test('setValidateStatus("ok") records the validated name', () => {
  const store = freshStore()
  store.getState().setValidateText('[workflow]\nname = "Daily"')
  store.getState().setValidateStatus('ok', { validatedName: 'Daily' })
  const v = store.getState().validate
  assert.equal(v.status, 'ok')
  assert.equal(v.validatedName, 'Daily')
  assert.equal(v.error, null)
  // Text persists so the user can keep editing after a successful pass.
  assert.equal(v.text, '[workflow]\nname = "Daily"')
})

test('setValidateStatus("error") records the parser message', () => {
  const store = freshStore()
  store.getState().setValidateText('not toml')
  store.getState().setValidateStatus('error', { error: 'expected `=`, found `t` at line 1 column 5' })
  const v = store.getState().validate
  assert.equal(v.status, 'error')
  assert.match(v.error ?? '', /line 1 column 5/)
  assert.equal(v.validatedName, null)
})

test('setValidateStatus("validating") preserves text and clears error/name', () => {
  const store = freshStore()
  store.getState().setValidateText('[workflow]\nname = "X"')
  store.getState().setValidateStatus('error', { error: 'old error' })
  store.getState().setValidateStatus('validating')
  const v = store.getState().validate
  assert.equal(v.status, 'validating')
  assert.equal(v.error, null)
  assert.equal(v.validatedName, null)
  assert.equal(v.text, '[workflow]\nname = "X"')
})

test('resetValidate returns to fresh state', () => {
  const store = freshStore()
  store.getState().setValidateText('something')
  store.getState().setValidateStatus('ok', { validatedName: 'X' })
  store.getState().resetValidate()
  const v = store.getState().validate
  assert.equal(v.status, 'idle')
  assert.equal(v.text, '')
  assert.equal(v.error, null)
  assert.equal(v.validatedName, null)
})

test('reset() clears validate alongside list/run state', () => {
  const store = freshStore()
  store.getState().setValidateText('xyz')
  store.getState().setValidateStatus('error', { error: 'bad' })
  store.getState().setRunStatus('Foo', 'running')
  store.getState().reset()
  const s = store.getState()
  assert.equal(s.validate.status, 'idle')
  assert.equal(s.validate.text, '')
  assert.deepEqual(s.runs, {})
})
