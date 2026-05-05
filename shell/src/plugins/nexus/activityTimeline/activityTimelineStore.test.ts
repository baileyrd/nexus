// shell/src/plugins/nexus/activityTimeline/activityTimelineStore.test.ts
//
// AIG-04 — coverage for the session + date-range filter actions added
// on top of the BL-037 store.
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/activityTimeline/activityTimelineStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  useActivityTimelineStore,
  type ActivityEntry,
} from './activityTimelineStore.ts'

function entry(
  id: string,
  session: string,
  ts: string,
  surface: ActivityEntry['surface'] = 'chat',
): ActivityEntry {
  return {
    id,
    timestamp: ts,
    session_id: session,
    surface,
    prompt: `prompt ${id}`,
    outcome: 'ok',
  }
}

function reset() {
  useActivityTimelineStore.setState({
    entries: [],
    filter: '',
    surfaceFilter: null,
    sessionFilter: null,
    dateFrom: null,
    dateTo: null,
    hydrated: false,
  })
}

test('setSessionFilter / setDateRange round-trip', () => {
  reset()
  const s = useActivityTimelineStore.getState()
  s.setSessionFilter('sess-A')
  s.setDateRange('2026-05-01', '2026-05-05')
  const after = useActivityTimelineStore.getState()
  assert.equal(after.sessionFilter, 'sess-A')
  assert.equal(after.dateFrom, '2026-05-01')
  assert.equal(after.dateTo, '2026-05-05')
})

test('resetFilters clears all four filter slots but keeps entries', () => {
  reset()
  const s = useActivityTimelineStore.getState()
  s.hydrate([entry('1', 'sess-A', '2026-05-01T10:00:00Z')])
  s.setFilter('foo')
  s.setSurfaceFilter('chat')
  s.setSessionFilter('sess-A')
  s.setDateRange('2026-05-01', '2026-05-05')
  s.resetFilters()
  const after = useActivityTimelineStore.getState()
  assert.equal(after.filter, '')
  assert.equal(after.surfaceFilter, null)
  assert.equal(after.sessionFilter, null)
  assert.equal(after.dateFrom, null)
  assert.equal(after.dateTo, null)
  assert.equal(after.entries.length, 1)
})

test('clear empties entries but leaves filters intact', () => {
  reset()
  const s = useActivityTimelineStore.getState()
  s.hydrate([entry('1', 'sess-A', '2026-05-01T10:00:00Z')])
  s.setSessionFilter('sess-A')
  s.clear()
  const after = useActivityTimelineStore.getState()
  assert.equal(after.entries.length, 0)
  assert.equal(after.sessionFilter, 'sess-A')
})
