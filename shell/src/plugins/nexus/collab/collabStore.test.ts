// BL-143 Phase 2.1 — collab store unit tests.
//
// Run via the wrapper at shell/tests/collab-store.test.ts which CI's
// `pnpm --filter nexus-shell test` glob picks up.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { useCollabStore } from './collabStore.ts'

function reset(): void {
  useCollabStore.getState().reset()
}

test('initial state is idle with no peers', () => {
  reset()
  const s = useCollabStore.getState()
  assert.equal(s.connection, 'idle')
  assert.deepEqual(s.peers, {})
})

test('onPeerJoined adds the peer keyed by peer_id', () => {
  reset()
  useCollabStore.getState().onPeerJoined({ peer_id: 'alice', display_name: 'Alice' })
  const peers = useCollabStore.getState().peers
  assert.equal(peers['alice']?.display_name, 'Alice')
  assert.equal(peers['alice']?.user_id, 'alice')
  assert.equal(peers['alice']?.cursor, undefined)
})

test('onPeerLeft removes the peer; missing peer is a no-op', () => {
  reset()
  const s = useCollabStore.getState()
  s.onPeerJoined({ peer_id: 'alice', display_name: 'Alice' })
  s.onPeerJoined({ peer_id: 'bob',   display_name: 'Bob' })

  s.onPeerLeft({ peer_id: 'alice' })
  assert.deepEqual(Object.keys(useCollabStore.getState().peers), ['bob'])

  // Removing again is harmless.
  s.onPeerLeft({ peer_id: 'alice' })
  assert.deepEqual(Object.keys(useCollabStore.getState().peers), ['bob'])
})

test('onPresence updates cursor without dropping prior name', () => {
  reset()
  const s = useCollabStore.getState()
  s.onPeerJoined({ peer_id: 'alice', display_name: 'Alice' })
  s.onPresence({
    user_id: 'alice',
    display_name: 'Alice',
    cursor: { relpath: 'notes/today.md', block_id: 'b-7' },
  })
  const a = useCollabStore.getState().peers['alice']!
  assert.equal(a.display_name, 'Alice')
  assert.equal(a.cursor?.relpath, 'notes/today.md')
  assert.equal(a.cursor?.block_id, 'b-7')
})

test('onPresence creates a peer if joined was never seen', () => {
  // The relay always sends PeerJoined before any presence, but the
  // local author publishes presence directly on the bus without a
  // joined frame — the store must still create the peer.
  reset()
  useCollabStore.getState().onPresence({
    user_id: 'me',
    display_name: 'Me',
    cursor: { relpath: 'x.md' },
  })
  assert.equal(useCollabStore.getState().peers['me']?.display_name, 'Me')
  assert.equal(useCollabStore.getState().peers['me']?.cursor?.relpath, 'x.md')
})

test('onPresence with no cursor clears any prior cursor', () => {
  reset()
  const s = useCollabStore.getState()
  s.onPresence({
    user_id: 'alice',
    display_name: 'Alice',
    cursor: { relpath: 'a.md' },
  })
  s.onPresence({ user_id: 'alice', display_name: 'Alice' })
  assert.equal(useCollabStore.getState().peers['alice']?.cursor, undefined)
})

test('onConnection cycles through the four states', () => {
  reset()
  const s = useCollabStore.getState()
  s.onConnection({ state: 'connecting' })
  assert.equal(useCollabStore.getState().connection, 'connecting')
  s.onConnection({ state: 'connected' })
  assert.equal(useCollabStore.getState().connection, 'connected')
  s.onConnection({ state: 'disconnected' })
  assert.equal(useCollabStore.getState().connection, 'disconnected')
})

test('reset clears peers and connection back to idle', () => {
  const s = useCollabStore.getState()
  s.onPeerJoined({ peer_id: 'alice', display_name: 'Alice' })
  s.onConnection({ state: 'connected' })
  s.reset()
  const after = useCollabStore.getState()
  assert.equal(after.connection, 'idle')
  assert.deepEqual(after.peers, {})
})
