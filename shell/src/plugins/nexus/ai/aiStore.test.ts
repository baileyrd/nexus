// shell/src/plugins/nexus/ai/aiStore.test.ts
//
// WI-01 Slice A + B unit tests for the chat store. The streaming
// contract is the load-bearing thing here; everything else in the
// chat plugin is wiring on top.
//
// Run with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/aiStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { useAiStore, type AiTurn } from './aiStore.ts'

function reset(): void {
  useAiStore.getState().reset()
  useAiStore.setState({ config: null })
}

/** Find the assistant turn whose requestId matches. Throws if absent
 *  — tests use this where the turn must exist, so a thrown error is
 *  the right failure mode (and saves us a `assert.ok` + non-null
 *  assertion at every call site). */
function findAssistant(requestId: string): Extract<AiTurn, { kind: 'assistant' }> {
  for (const t of useAiStore.getState().turns) {
    if (t.kind === 'assistant' && t.requestId === requestId) return t
  }
  throw new Error(`no assistant turn found for requestId=${requestId}`)
}

// ── Slice A coverage (preserved, adapted for the turns array) ─────────────

test('startAsk: clears composer, appends user + assistant turns, records request id', () => {
  reset()
  const s = useAiStore.getState()
  s.setQuestion('what is foo?')

  s.startAsk('req-1', 'what is foo?')
  const after = useAiStore.getState()

  assert.equal(after.status, 'asking')
  assert.equal(after.currentRequestId, 'req-1')
  assert.equal(after.question, '', 'composer must clear optimistically (legacy ChatPanel.tsx:472)')
  assert.equal(after.turns.length, 2, 'one user turn + one streaming assistant turn')
  assert.equal(after.turns[0].kind, 'user')
  assert.equal(after.turns[1].kind, 'assistant')
  if (after.turns[0].kind === 'user') {
    assert.equal(after.turns[0].question, 'what is foo?')
  }
  if (after.turns[1].kind === 'assistant') {
    assert.equal(after.turns[1].requestId, 'req-1')
    assert.equal(after.turns[1].status, 'streaming')
    assert.equal(after.turns[1].streamedText, '')
    assert.equal(after.turns[1].finalText, null)
    assert.deepEqual(after.turns[1].sources, [])
  }
})

test('appendChunk: matching request_id appends to the assistant turn + flips streaming', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')

  s.appendChunk('req-1', 'hello ')
  s.appendChunk('req-1', 'world')

  const after = useAiStore.getState()
  assert.equal(after.status, 'streaming')
  const asst = findAssistant('req-1')
  assert.equal(asst.streamedText, 'hello world')
})

test('appendChunk: mismatched request_id is dropped silently', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'good ')

  // Stale chunk from a prior (cancelled) request: must be ignored.
  s.appendChunk('req-OLD', 'BAD ')
  // Future request that hasn't been started yet: also ignored.
  s.appendChunk('req-2', 'BAD ')

  const asst = findAssistant('req-1')
  assert.equal(asst.streamedText, 'good ', 'only matching chunks accumulate')
})

test('finishStream: matching id sets finalText, clears streamed buffer, idle', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'partial')

  s.finishStream('req-1', 'PARTIAL — but the final wins')

  const after = useAiStore.getState()
  assert.equal(after.status, 'idle')
  assert.equal(after.currentRequestId, null)
  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'done')
  assert.equal(asst.streamedText, '', 'streamed buffer cleared so render falls through to finalText')
  assert.equal(
    asst.finalText,
    'PARTIAL — but the final wins',
    'stream_done.text is authoritative, overwrites accumulated chunks (legacy ChatPanel.tsx:335)',
  )
})

test('finishStream: mismatched id ignored — does not clobber an in-flight stream', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-2', 'q')
  s.appendChunk('req-2', 'live data')

  // A stale done event from a prior request must NOT clear our buffer.
  s.finishStream('req-OLD', 'stale final')

  const after = useAiStore.getState()
  assert.equal(after.status, 'streaming')
  assert.equal(after.currentRequestId, 'req-2')
  const asst = findAssistant('req-2')
  assert.equal(asst.status, 'streaming')
  assert.equal(asst.streamedText, 'live data')
  assert.equal(asst.finalText, null)
})

test('cancel mid-stream: assistant flips to done with finalText = streamedText', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'half-formed thought')

  s.cancel()

  const after = useAiStore.getState()
  assert.equal(after.status, 'idle')
  assert.equal(after.currentRequestId, null)
  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'done', 'cancelled assistant turn is "done", not "streaming"')
  assert.equal(asst.streamedText, '', 'streamed buffer drains into finalText')
  assert.equal(asst.finalText, 'half-formed thought', 'partial preserved so the conversation stays coherent')
})

test('cancel makes subsequent chunks/done events no-ops (no streaming turn matches)', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'first')
  s.cancel()

  // Kernel may keep producing chunks / a final done. They must
  // bounce off the matching-turn guard now that the assistant turn
  // has status='done'.
  s.appendChunk('req-1', ' second')
  s.finishStream('req-1', 'final from cancelled request')

  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'done', 'no state change from late events')
  assert.equal(asst.finalText, 'first', 'cancelled finalText preserved, not overwritten')
})

test('setError: marks the in-flight assistant turn errored, preserves partial', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'why is the sky blue?')
  s.appendChunk('req-1', 'because')

  const err = new Error('kernel timed out')
  s.setError(err)

  const after = useAiStore.getState()
  assert.equal(after.status, 'error')
  assert.equal(after.currentRequestId, null)
  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'error')
  assert.equal(asst.error, err)
  assert.equal(asst.finalText, 'because', 'partial response preserved alongside the error')
})

test('a fresh startAsk after error appends new turns and clears global error status', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q1')
  s.setError(new Error('boom'))

  s.startAsk('req-2', 'q2')

  const after = useAiStore.getState()
  assert.equal(after.status, 'asking')
  assert.equal(after.currentRequestId, 'req-2')
  assert.equal(after.turns.length, 4, 'two user + two assistant turns total')
  // Prior errored turn untouched.
  const first = findAssistant('req-1')
  assert.equal(first.status, 'error')
})

// ── Slice B coverage ──────────────────────────────────────────────────────

test('Slice B: each submit appends two turns; conversation grows linearly', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q1')
  s.finishStream('req-1', 'a1')
  s.startAsk('req-2', 'q2')
  s.finishStream('req-2', 'a2')
  s.startAsk('req-3', 'q3')
  s.finishStream('req-3', 'a3')

  const turns = useAiStore.getState().turns
  assert.equal(turns.length, 6)
  // Pattern: user, assistant, user, assistant, ...
  assert.equal(turns[0].kind, 'user')
  assert.equal(turns[1].kind, 'assistant')
  assert.equal(turns[2].kind, 'user')
  assert.equal(turns[3].kind, 'assistant')
  if (turns[0].kind === 'user') assert.equal(turns[0].question, 'q1')
  if (turns[5].kind === 'assistant') assert.equal(turns[5].finalText, 'a3')
})

test('Slice B: chunks route to the correct assistant turn by requestId across multiple in-flight requests', () => {
  reset()
  const s = useAiStore.getState()
  // Start req-A, then req-B without finishing req-A. The runtime
  // single-flights submits, but the store must still route chunks
  // correctly if events interleave.
  s.startAsk('req-A', 'qA')
  s.startAsk('req-B', 'qB')

  s.appendChunk('req-A', 'A-data ')
  s.appendChunk('req-B', 'B-data ')
  s.appendChunk('req-A', 'more A')

  const a = findAssistant('req-A')
  const b = findAssistant('req-B')
  assert.equal(a.streamedText, 'A-data more A')
  assert.equal(b.streamedText, 'B-data ')
})

test('Slice B: finishStream attaches sources to the matching assistant turn', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'what is rust?')
  s.appendChunk('req-1', 'Rust is')

  s.finishStream('req-1', 'Rust is a systems language.', [
    { path: 'notes/rust.md', excerpt: 'Rust is a systems programming language.', score: 0.95, blockId: 1 },
    { path: 'notes/lang.md', score: 0.42 },
  ])

  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'done')
  assert.equal(asst.sources.length, 2)
  assert.equal(asst.sources[0].path, 'notes/rust.md')
  assert.equal(asst.sources[0].score, 0.95)
  assert.equal(asst.sources[1].excerpt, undefined)
})

test('Slice B: finishStream with undefined sources keeps the prior sources untouched', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  // Manually pre-seed sources on the assistant turn (would happen if
  // we ever start using `stream_start.sources` for pre-render).
  const turns = useAiStore.getState().turns.slice()
  if (turns[1].kind === 'assistant') {
    turns[1] = { ...turns[1], sources: [{ path: 'pre.md' }] }
  }
  useAiStore.setState({ turns })

  s.finishStream('req-1', 'final')

  const asst = findAssistant('req-1')
  assert.deepEqual(asst.sources, [{ path: 'pre.md' }])
})

test('Slice B: clearTurns wipes turns but preserves config + composer + in-flight status', () => {
  reset()
  const s = useAiStore.getState()
  s.setConfig({
    ai: { provider: 'anthropic', model: 'claude-3', base_url: null, has_api_key: true },
    embedding: null,
  })
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'streaming...')
  s.setQuestion('drafting next')

  s.clearTurns()

  const after = useAiStore.getState()
  assert.equal(after.turns.length, 0)
  const cfg = after.config
  if (!cfg) throw new Error('config must survive clearTurns')
  assert.equal(cfg.ai?.provider, 'anthropic')
  assert.equal(after.question, 'drafting next', 'composer text untouched')
  // In-flight stream's status is left as-is — clear is orthogonal to
  // cancel. The plugin's CLEAR command pairs the two together.
  assert.equal(after.status, 'streaming')
  assert.equal(after.currentRequestId, 'req-1')
})

test('Slice B: clearTurns then late chunks land harmlessly (no matching turn)', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'partial')

  // User clears chat WITHOUT cancelling — kernel still streaming.
  s.clearTurns()

  // Late chunks should bounce off the missing-turn guard.
  s.appendChunk('req-1', ' more')
  s.finishStream('req-1', 'final')

  assert.equal(useAiStore.getState().turns.length, 0, 'clearTurns left no rehydrated turn behind')
})

test('Slice B: cancel with no streamed content flips status to done with null finalText', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  // No appendChunk — cancel before any tokens arrived.
  s.cancel()

  const asst = findAssistant('req-1')
  assert.equal(asst.status, 'done')
  assert.equal(asst.finalText, null, 'no partial means finalText stays null; view shows "(no response)"')
})

// ── Slice C coverage ──────────────────────────────────────────────────────
//
// These tests cover the store-side bookkeeping. The aiRuntime IPC
// glue (loadSessions / saveCurrentSession / etc.) is exercised via
// stubbed `api.kernel.invoke` so we don't need a live kernel.

import {
  loadSessions,
  loadSession,
  saveCurrentSession,
  deleteSession,
  renameSession,
  flushAutosave,
} from './aiRuntime.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
}

/** Build a minimal PluginAPI-like stub. Only `kernel.invoke` is real
 *  — everything else is a placeholder that throws if accidentally
 *  touched. The runtime functions we test only ever call invoke. */
function stubApi(handler: (commandId: string, args: unknown) => unknown) {
  const calls: InvokeCall[] = []
  const api = {
    kernel: {
      invoke: async (pluginId: string, commandId: string, args: unknown) => {
        calls.push({ pluginId, commandId, args })
        return handler(commandId, args)
      },
    },
  } as unknown as Parameters<typeof loadSessions>[0]
  return { api, calls }
}

function fullReset(): void {
  flushAutosave()
  reset()
}

test('Slice C: loadSessions populates the sessions list, sorted newest-first', async () => {
  fullReset()
  const { api } = stubApi((cmd) => {
    if (cmd !== 'session_list') throw new Error(`unexpected ${cmd}`)
    return [
      { id: 'old', title: 'Old', updated_at: '2024-01-01T00:00:00Z', bytes: 100 },
      { id: 'new', title: 'New', updated_at: '2024-06-01T00:00:00Z', bytes: 200 },
      { id: 'mid', title: 'Mid', updated_at: '2024-03-01T00:00:00Z', bytes: 150 },
      { id: '', title: 'invalid', updated_at: null, bytes: 0 }, // dropped
    ]
  })

  await loadSessions(api)
  const after = useAiStore.getState()
  assert.equal(after.sessions.length, 3, 'invalid id dropped')
  assert.deepEqual(
    after.sessions.map((s) => s.id),
    ['new', 'mid', 'old'],
    'newest-first by updated_at',
  )
  assert.equal(after.sessionsLoading, false)
})

test('Slice C: loadSession replaces turns + sets activeSessionId', async () => {
  fullReset()
  // Pre-load some current turns to prove they get replaced.
  const s = useAiStore.getState()
  s.startAsk('old-req', 'discarded')
  s.finishStream('old-req', 'old answer')

  const { api } = stubApi((cmd, args) => {
    if (cmd !== 'session_load') throw new Error(`unexpected ${cmd}`)
    assert.deepEqual(args, { id: 'sess-A' })
    return {
      id: 'sess-A',
      title: 'Loaded session',
      turns: [
        { kind: 'user', id: 'u1', question: 'hello', askedAt: 1 },
        {
          kind: 'assistant',
          id: 'a1',
          requestId: 'r1',
          status: 'done',
          streamedText: '',
          finalText: 'hi there',
          sources: [{ path: 'doc.md' }],
          error: null,
        },
      ],
    }
  })

  await loadSession(api, 'sess-A')
  const after = useAiStore.getState()
  assert.equal(after.activeSessionId, 'sess-A')
  assert.equal(after.turns.length, 2)
  assert.equal(after.turns[0].kind, 'user')
  if (after.turns[0].kind === 'user') assert.equal(after.turns[0].question, 'hello')
  assert.equal(after.turns[1].kind, 'assistant')
  if (after.turns[1].kind === 'assistant') {
    assert.equal(after.turns[1].finalText, 'hi there')
    assert.equal(after.turns[1].sources[0]?.path, 'doc.md')
  }
})

test('Slice C: saveCurrentSession with no title auto-derives from first user turn (cap 48 + ellipsis)', async () => {
  fullReset()
  const s = useAiStore.getState()
  const longQ = 'a'.repeat(60) // longer than 48
  s.startAsk('req-1', longQ)
  s.finishStream('req-1', 'answer')

  let savedArgs: Record<string, unknown> | null = null
  const { api, calls } = stubApi((cmd, args) => {
    if (cmd === 'session_save') {
      savedArgs = args as Record<string, unknown>
      return { bytes: 100, id: (args as Record<string, unknown>).id }
    }
    if (cmd === 'session_list') return []
    throw new Error(`unexpected ${cmd}`)
  })

  const id = await saveCurrentSession(api)
  assert.ok(id, 'returns minted id')
  assert.ok(savedArgs, 'session_save was called')
  const args = savedArgs as unknown as Record<string, unknown>
  const title = args.title as string
  assert.equal(title.length, 49, '48 chars + ellipsis')
  assert.ok(title.endsWith('…'))
  // Refreshes the list right after.
  assert.ok(calls.some((c) => c.commandId === 'session_list'))
  assert.equal(useAiStore.getState().activeSessionId, id)
})

test('Slice C: saveCurrentSession with explicit title preserves it verbatim', async () => {
  fullReset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'something boring')
  s.finishStream('req-1', 'answer')

  let savedTitle: unknown = null
  const { api } = stubApi((cmd, args) => {
    if (cmd === 'session_save') {
      savedTitle = (args as Record<string, unknown>).title
      return { bytes: 1, id: (args as Record<string, unknown>).id }
    }
    if (cmd === 'session_list') return []
    throw new Error(`unexpected ${cmd}`)
  })

  await saveCurrentSession(api, 'Custom Title')
  assert.equal(savedTitle, 'Custom Title')
})

test('Slice C: saveCurrentSession on empty conversation is a no-op', async () => {
  fullReset()
  let saveCalled = false
  const { api } = stubApi((cmd) => {
    if (cmd === 'session_save') saveCalled = true
    return null
  })
  const id = await saveCurrentSession(api)
  assert.equal(id, null, 'returns null for empty conversation')
  assert.equal(saveCalled, false, 'no kernel call for empty turns')
})

test('Slice C: deleteSession removes the session from the list and clears activeSessionId if active', async () => {
  fullReset()
  // Seed an active session and a list snapshot.
  useAiStore.setState({
    activeSessionId: 'sess-A',
    sessions: [
      { id: 'sess-A', title: 'A', updatedAt: '2024-06-01T00:00:00Z', bytes: 1 },
      { id: 'sess-B', title: 'B', updatedAt: '2024-05-01T00:00:00Z', bytes: 1 },
    ],
  })
  const s = useAiStore.getState()
  s.startAsk('r-A', 'q')
  s.finishStream('r-A', 'a')

  let listResp: Array<Record<string, unknown>> = [
    { id: 'sess-B', title: 'B', updated_at: '2024-05-01T00:00:00Z', bytes: 1 },
  ]
  const { api } = stubApi((cmd, args) => {
    if (cmd === 'session_delete') {
      assert.deepEqual(args, { id: 'sess-A' })
      return { deleted: true, id: 'sess-A' }
    }
    if (cmd === 'session_list') return listResp
    throw new Error(`unexpected ${cmd}`)
  })

  await deleteSession(api, 'sess-A')
  const after = useAiStore.getState()
  assert.equal(after.activeSessionId, null, 'active id cleared because the deleted session WAS active')
  assert.equal(after.sessions.length, 1)
  assert.equal(after.sessions[0].id, 'sess-B')
  assert.equal(after.turns.length, 0, 'newSession() ran since deleted was active')
})

test('Slice C: deleteSession of a NON-active session leaves activeSessionId + turns alone', async () => {
  fullReset()
  useAiStore.setState({
    activeSessionId: 'sess-A',
    sessions: [
      { id: 'sess-A', title: 'A', updatedAt: '2024-06-01T00:00:00Z', bytes: 1 },
      { id: 'sess-B', title: 'B', updatedAt: '2024-05-01T00:00:00Z', bytes: 1 },
    ],
  })
  const s = useAiStore.getState()
  s.startAsk('r-A', 'keep me')
  s.finishStream('r-A', 'answer')

  const { api } = stubApi((cmd) => {
    if (cmd === 'session_delete') return { deleted: true, id: 'sess-B' }
    if (cmd === 'session_list') {
      return [{ id: 'sess-A', title: 'A', updated_at: '2024-06-01T00:00:00Z', bytes: 1 }]
    }
    throw new Error(`unexpected ${cmd}`)
  })
  await deleteSession(api, 'sess-B')
  const after = useAiStore.getState()
  assert.equal(after.activeSessionId, 'sess-A', 'untouched')
  assert.equal(after.turns.length, 2, 'turns survive')
})

test('Slice C: newSession clears turns + activeSessionId without cancelling in-flight stream', () => {
  fullReset()
  const s = useAiStore.getState()
  useAiStore.setState({ activeSessionId: 'sess-A' })
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'streaming...')

  // Pre-condition: in-flight stream alive.
  assert.equal(useAiStore.getState().status, 'streaming')
  assert.equal(useAiStore.getState().currentRequestId, 'req-1')

  s.newSession()

  const after = useAiStore.getState()
  assert.equal(after.turns.length, 0, 'turns cleared')
  assert.equal(after.activeSessionId, null, 'active id cleared')
  // Crucial: the store-level newSession is orthogonal to cancel. The
  // runtime's startNewChat() pairs cancelInFlight + saveCurrentSession
  // around it; the store action itself stays narrow so tests can
  // exercise the pieces independently.
  assert.equal(after.status, 'streaming', 'stream status untouched by store-level newSession')
  assert.equal(after.currentRequestId, 'req-1', 'requestId untouched by store-level newSession')
})

test('Slice C: renameSession on a non-active session round-trips through load + save with new title', async () => {
  fullReset()
  // List has B; A is active.
  useAiStore.setState({
    activeSessionId: 'sess-A',
    sessions: [
      { id: 'sess-A', title: 'A', updatedAt: '2024-06-01T00:00:00Z', bytes: 1 },
      { id: 'sess-B', title: 'B-old', updatedAt: '2024-05-01T00:00:00Z', bytes: 1 },
    ],
  })

  let savedTitle: unknown = null
  let savedTurns: unknown = null
  const { api, calls } = stubApi((cmd, args) => {
    if (cmd === 'session_load') {
      assert.deepEqual(args, { id: 'sess-B' })
      return {
        id: 'sess-B',
        title: 'B-old',
        turns: [{ kind: 'user', id: 'u1', question: 'hi from B', askedAt: 1 }],
      }
    }
    if (cmd === 'session_save') {
      const a = args as Record<string, unknown>
      assert.equal(a.id, 'sess-B')
      savedTitle = a.title
      savedTurns = a.turns
      return { bytes: 1, id: 'sess-B' }
    }
    if (cmd === 'session_list') return []
    throw new Error(`unexpected ${cmd}`)
  })

  await renameSession(api, 'sess-B', 'B-new')
  assert.equal(savedTitle, 'B-new')
  assert.ok(Array.isArray(savedTurns) && (savedTurns as unknown[]).length === 1, 'preserves loaded turns')
  // Order: load → save → list refresh.
  assert.deepEqual(
    calls.map((c) => c.commandId),
    ['session_load', 'session_save', 'session_list'],
  )
})

test('Slice C: renameSession on the ACTIVE session uses in-memory turns (no extra load round-trip)', async () => {
  fullReset()
  useAiStore.setState({ activeSessionId: 'sess-A' })
  const s = useAiStore.getState()
  s.startAsk('r-A', 'in memory')
  s.finishStream('r-A', 'answer')

  const { api, calls } = stubApi((cmd, args) => {
    if (cmd === 'session_save') {
      const a = args as Record<string, unknown>
      assert.equal(a.id, 'sess-A')
      assert.equal(a.title, 'A-new')
      return { bytes: 1, id: 'sess-A' }
    }
    if (cmd === 'session_list') return []
    throw new Error(`unexpected ${cmd}`)
  })

  await renameSession(api, 'sess-A', 'A-new')
  // No session_load — we used in-memory turns to skip the round-trip.
  assert.deepEqual(
    calls.map((c) => c.commandId),
    ['session_save', 'session_list'],
  )
})

test('Slice C: renameSession with empty / whitespace-only title is a no-op', async () => {
  fullReset()
  useAiStore.setState({ activeSessionId: 'sess-A' })
  const s = useAiStore.getState()
  s.startAsk('r-A', 'q')
  s.finishStream('r-A', 'a')

  let saveCalled = false
  const { api } = stubApi((cmd) => {
    if (cmd === 'session_save') saveCalled = true
    return null
  })
  await renameSession(api, 'sess-A', '   ')
  assert.equal(saveCalled, false, 'whitespace-only title rejected without IPC')
})
