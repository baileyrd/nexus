// BL-077 follow-up — unit tests for the list-picker store.

import test from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetPickStoreForTests,
  requestPick,
  usePickStore,
} from './pickStore.ts'

test('requestPick: empty items resolves immediately with null without enqueuing', async () => {
  _resetPickStoreForTests()
  const result = await requestPick([])
  assert.equal(result, null)
  assert.equal(usePickStore.getState().current, null)
})

test('requestPick: enqueues a request and resolves with the picked value', async () => {
  _resetPickStoreForTests()
  const promise = requestPick<number>([
    { label: 'one', value: 1 },
    { label: 'two', value: 2 },
  ])
  const current = usePickStore.getState().current
  assert.ok(current)
  // Pick the second item by value.
  usePickStore.getState().resolveCurrent(current.items[1])
  assert.equal(await promise, 2)
  // Store advances back to idle.
  assert.equal(usePickStore.getState().current, null)
})

test('requestPick: cancel resolves with null', async () => {
  _resetPickStoreForTests()
  const promise = requestPick([{ label: 'only', value: 'X' }])
  usePickStore.getState().resolveCurrent(null)
  assert.equal(await promise, null)
})

test('requestPick: serialises concurrent requests behind a single modal', async () => {
  _resetPickStoreForTests()
  const a = requestPick<string>([{ label: 'A', value: 'a' }])
  const b = requestPick<string>([{ label: 'B', value: 'b' }])

  // First request is current; second is queued.
  let s = usePickStore.getState()
  assert.equal(s.current?.items[0].label, 'A')
  assert.equal(s.queue.length, 1)
  assert.equal(s.queue[0].items[0].label, 'B')

  // Resolve A — the second request advances to current.
  usePickStore.getState().resolveCurrent(s.current!.items[0])
  assert.equal(await a, 'a')
  s = usePickStore.getState()
  assert.equal(s.current?.items[0].label, 'B')
  assert.equal(s.queue.length, 0)

  usePickStore.getState().resolveCurrent(s.current!.items[0])
  assert.equal(await b, 'b')
  assert.equal(usePickStore.getState().current, null)
})

test('requestPick: passes title and placeholder through to the request', async () => {
  _resetPickStoreForTests()
  void requestPick([{ label: 'X', value: 1 }], {
    title: 'Pick a thing',
    placeholder: 'Filter…',
  })
  const current = usePickStore.getState().current
  assert.ok(current)
  assert.equal(current.title, 'Pick a thing')
  assert.equal(current.placeholder, 'Filter…')
  // Clean up — don't leave a pending promise.
  usePickStore.getState().resolveCurrent(null)
})

test('requestPick: resolveCurrent on idle store is a no-op', () => {
  _resetPickStoreForTests()
  // Should not throw and should not flip into any odd state.
  usePickStore.getState().resolveCurrent(null)
  assert.equal(usePickStore.getState().current, null)
})

test('requestPick: typed value round-trips through the resolver', async () => {
  interface Action {
    title: string
    kind: string
  }
  _resetPickStoreForTests()
  const promise = requestPick<Action>([
    { label: 'rename', value: { title: 'rename', kind: 'refactor' } },
    { label: 'extract', value: { title: 'extract', kind: 'refactor.extract' } },
  ])
  const current = usePickStore.getState().current
  assert.ok(current)
  usePickStore.getState().resolveCurrent(current.items[1])
  const picked = await promise
  assert.deepEqual(picked, { title: 'extract', kind: 'refactor.extract' })
})
