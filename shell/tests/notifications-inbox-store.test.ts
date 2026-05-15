import { describe, it, beforeEach } from 'node:test'
import assert from 'node:assert/strict'
import {
  deriveStats,
  parsePayloadTaskId,
  useNotificationsInboxStore,
  type InboxEntry,
} from '../src/plugins/nexus/notificationsInbox/notificationsInboxStore'

function row(over: Partial<InboxEntry>): InboxEntry {
  return {
    id: 'id-1',
    source: 'wf',
    severity: 'info',
    title: null,
    body: 'b',
    channels: [],
    ts: 1_715_000_000,
    read_at: null,
    dismissed_at: null,
    payload_json: null,
    ...over,
  }
}

describe('useNotificationsInboxStore', () => {
  beforeEach(() => {
    useNotificationsInboxStore.getState().clear()
    useNotificationsInboxStore.setState({ hydrated: false })
  })

  it('hydrate replaces entries and flips hydrated', () => {
    useNotificationsInboxStore
      .getState()
      .hydrate([row({ id: 'a' }), row({ id: 'b' })])
    const state = useNotificationsInboxStore.getState()
    assert.equal(state.entries.length, 2)
    assert.equal(state.hydrated, true)
  })

  it('prepend dedupes by id', () => {
    const s = useNotificationsInboxStore.getState()
    s.hydrate([row({ id: 'a' })])
    s.prepend(row({ id: 'a' }))
    s.prepend(row({ id: 'b' }))
    assert.deepEqual(
      useNotificationsInboxStore.getState().entries.map((e) => e.id),
      ['b', 'a'],
    )
  })

  it('markRead only flips unread rows', () => {
    const s = useNotificationsInboxStore.getState()
    s.hydrate([row({ id: 'a' }), row({ id: 'b', read_at: 100 })])
    s.markRead(['a', 'b'])
    const after = useNotificationsInboxStore.getState().entries
    const a = after.find((e) => e.id === 'a')!
    const b = after.find((e) => e.id === 'b')!
    assert.notEqual(a.read_at, null, 'a flipped')
    assert.equal(b.read_at, 100, 'b kept its earlier timestamp')
  })

  it('markDismissed sets dismissed_at and read_at when missing', () => {
    const s = useNotificationsInboxStore.getState()
    s.hydrate([row({ id: 'a' }), row({ id: 'b', read_at: 50 })])
    s.markDismissed(['a', 'b'])
    const after = useNotificationsInboxStore.getState().entries
    const a = after.find((e) => e.id === 'a')!
    const b = after.find((e) => e.id === 'b')!
    assert.notEqual(a.dismissed_at, null, 'a dismissed')
    assert.notEqual(a.read_at, null, 'a read at gets stamped')
    assert.equal(b.read_at, 50, 'b read_at preserved')
    assert.notEqual(b.dismissed_at, null, 'b dismissed')
  })
})

describe('deriveStats', () => {
  it('counts unread and per-source unread, skipping dismissed', () => {
    const stats = deriveStats([
      row({ id: 'a', source: 'wf' }),
      row({ id: 'b', source: 'wf', read_at: 1 }),
      row({ id: 'c', source: 'ai_runtime' }),
      row({ id: 'd', source: 'wf', dismissed_at: 1 }),
    ])
    assert.equal(stats.total, 3, 'dismissed rows excluded')
    assert.equal(stats.unread, 2)
    assert.deepEqual(stats.by_source, { wf: 1, ai_runtime: 1 })
  })
})

describe('parsePayloadTaskId', () => {
  it('returns null for empty / non-JSON / malformed shapes', () => {
    assert.equal(parsePayloadTaskId(null), null)
    assert.equal(parsePayloadTaskId(''), null)
    assert.equal(parsePayloadTaskId('not json'), null)
    assert.equal(parsePayloadTaskId('{}'), null)
    assert.equal(parsePayloadTaskId('{"task_id": 7}'), null, 'non-string task_id')
  })

  it('extracts task_id from a well-formed payload', () => {
    assert.equal(
      parsePayloadTaskId('{"task_id": "abc-123"}'),
      'abc-123',
    )
  })
})
