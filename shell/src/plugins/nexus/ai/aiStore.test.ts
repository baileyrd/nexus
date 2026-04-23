// shell/src/plugins/nexus/ai/aiStore.test.ts
//
// WI-01 Slice A unit tests for the chat store. The streaming
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
import { useAiStore } from './aiStore.ts'

function reset(): void {
  useAiStore.getState().reset()
  useAiStore.setState({ config: null })
}

test('startAsk: clears prior answer + composer, records request id', () => {
  reset()
  const s = useAiStore.getState()
  s.setQuestion('what is foo?')

  s.startAsk('req-1', 'what is foo?')
  const after = useAiStore.getState()

  assert.equal(after.status, 'asking')
  assert.equal(after.currentRequestId, 'req-1')
  assert.equal(after.question, '', 'composer must clear optimistically (legacy ChatPanel.tsx:472)')
  assert.equal(after.lastQuestion, 'what is foo?', 'lastQuestion preserved for retry')
  assert.equal(after.streamedAnswer, '')
  assert.equal(after.finalAnswer, null)
  assert.equal(after.error, null)
})

test('appendChunk: matching request_id appends + flips to streaming', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')

  s.appendChunk('req-1', 'hello ')
  s.appendChunk('req-1', 'world')

  const after = useAiStore.getState()
  assert.equal(after.status, 'streaming')
  assert.equal(after.streamedAnswer, 'hello world')
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

  const after = useAiStore.getState()
  assert.equal(after.streamedAnswer, 'good ', 'only matching chunks accumulate')
})

test('finishStream: matching id sets finalAnswer, clears streamed buffer, idle', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'partial')

  s.finishStream('req-1', 'PARTIAL — but the final wins')

  const after = useAiStore.getState()
  assert.equal(after.status, 'idle')
  assert.equal(after.currentRequestId, null)
  assert.equal(after.streamedAnswer, '', 'streamed buffer cleared so render falls through to finalAnswer')
  assert.equal(
    after.finalAnswer,
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
  assert.equal(after.streamedAnswer, 'live data')
  assert.equal(after.finalAnswer, null)
  assert.equal(after.currentRequestId, 'req-2')
})

test('cancel mid-stream: clears streamed buffer + currentRequestId, idles', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'half-formed thought')

  s.cancel()

  const after = useAiStore.getState()
  assert.equal(after.status, 'idle')
  assert.equal(after.currentRequestId, null)
  assert.equal(after.streamedAnswer, '', 'cancel wipes the visible streaming text')
  assert.equal(after.finalAnswer, null)
})

test('cancel makes subsequent chunks/done events no-ops (request_id mismatch)', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q')
  s.appendChunk('req-1', 'first')
  s.cancel()

  // Kernel may keep producing chunks / a final done — there's no
  // server-side abort yet (see aiRuntime.ts comments). They must
  // bounce off the request_id check now that currentRequestId=null.
  s.appendChunk('req-1', ' second')
  s.finishStream('req-1', 'final from cancelled request')

  const after = useAiStore.getState()
  assert.equal(after.status, 'idle', 'no state change from late events')
  assert.equal(after.streamedAnswer, '')
  assert.equal(after.finalAnswer, null, 'cancelled request must not retroactively populate finalAnswer')
})

test('setError sets error + idles, preserving lastQuestion for retry', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'why is the sky blue?')
  s.appendChunk('req-1', 'because')

  const err = new Error('kernel timed out')
  s.setError(err)

  const after = useAiStore.getState()
  assert.equal(after.status, 'error')
  assert.equal(after.currentRequestId, null)
  assert.equal(after.error, err)
  assert.equal(after.lastQuestion, 'why is the sky blue?', 'retry button reads lastQuestion')
})

test('a fresh startAsk after error clears the prior error', () => {
  reset()
  const s = useAiStore.getState()
  s.startAsk('req-1', 'q1')
  s.setError(new Error('boom'))

  s.startAsk('req-2', 'q2')

  const after = useAiStore.getState()
  assert.equal(after.status, 'asking')
  assert.equal(after.error, null)
  assert.equal(after.currentRequestId, 'req-2')
})
