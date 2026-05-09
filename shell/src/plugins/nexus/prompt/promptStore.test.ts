// Unit tests for the styled prompt store.

import test from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetPromptStoreForTests,
  requestPrompt,
  usePromptStore,
} from './promptStore.ts'

test('requestPrompt: enqueues and resolves with the typed value', async () => {
  _resetPromptStoreForTests()
  const promise = requestPrompt('What?')
  const current = usePromptStore.getState().current
  assert.ok(current)
  usePromptStore.getState().resolveCurrent('answer')
  assert.equal(await promise, 'answer')
  assert.equal(usePromptStore.getState().current, null)
})

test('requestPrompt: cancel resolves with null', async () => {
  _resetPromptStoreForTests()
  const promise = requestPrompt('What?')
  usePromptStore.getState().resolveCurrent(null)
  assert.equal(await promise, null)
})

test('requestPrompt: empty-string commit is distinct from cancel', async () => {
  _resetPromptStoreForTests()
  const promise = requestPrompt('What?')
  usePromptStore.getState().resolveCurrent('')
  // Empty string is a valid commit — callers that want to reject
  // empty input do so themselves (matches the prior window.prompt
  // semantics).
  assert.equal(await promise, '')
})

test('requestPrompt: uses placeholder as the initial value', async () => {
  _resetPromptStoreForTests()
  void requestPrompt('Rename to:', 'old_name')
  const current = usePromptStore.getState().current
  assert.ok(current)
  assert.equal(current.placeholder, 'old_name')
  assert.equal(current.initialValue, 'old_name')
  usePromptStore.getState().resolveCurrent(null)
})

test('requestPrompt: serialises concurrent requests behind a single modal', async () => {
  _resetPromptStoreForTests()
  const a = requestPrompt('first?')
  const b = requestPrompt('second?')
  let s = usePromptStore.getState()
  assert.equal(s.current?.message, 'first?')
  assert.equal(s.queue.length, 1)
  assert.equal(s.queue[0].message, 'second?')

  usePromptStore.getState().resolveCurrent('A')
  assert.equal(await a, 'A')
  s = usePromptStore.getState()
  assert.equal(s.current?.message, 'second?')

  usePromptStore.getState().resolveCurrent('B')
  assert.equal(await b, 'B')
  assert.equal(usePromptStore.getState().current, null)
})

test('requestPrompt: resolveCurrent on idle store is a no-op', () => {
  _resetPromptStoreForTests()
  usePromptStore.getState().resolveCurrent('whatever')
  assert.equal(usePromptStore.getState().current, null)
})
