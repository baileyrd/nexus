// shell/src/plugins/nexus/ai/aiStore.test.ts
//
// WI-01 Slice A + B unit tests for the chat store. The streaming
// contract is the load-bearing thing here; everything else in the
// chat plugin is wiring on top.
//
// Run with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/aiStore.test.ts

// `node:test` and `node:assert/strict` aren't in the shell tsconfig's
// `lib` set (no `@types/node`), so importing them directly would fail
// `pnpm typecheck`. The other shell sibling tests (editorStore.test.ts)
// dodge this with `(await import(...))`, but top-level await trips
// esbuild's CJS transform when the file is loaded via the
// `tests/*.test.ts` runner glob.
//
// `// @ts-expect-error` keeps tsc quiet without changing the runtime
// shape — node:test resolves at run time under `node --import tsx`.
//
// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
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
