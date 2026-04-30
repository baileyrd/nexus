// shell/src/plugins/nexus/ai/marginSuggest.test.ts
//
// BL-036 phase 1 — engine coverage for the AMB margin-suggestions
// pass. Stubs `api.kernel.invoke` so the flow doesn't reach Tauri /
// Rust, then asserts the store transitions and parser behaviour.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/marginSuggest.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  _resetForTests,
  buildSuggestionPrompt,
  isMarginSessionId,
  parseSuggestionsResponse,
  requestPass,
} from './marginSuggest.ts'
import { useMarginSuggestStore } from './marginSuggestStore.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: Record<string, unknown>
  timeoutMs: number | undefined
}

function reset(): void {
  _resetForTests()
  useMarginSuggestStore.getState().clear()
}

function stubApi(invokeImpl: (call: InvokeCall) => Promise<unknown>) {
  const calls: InvokeCall[] = []
  return {
    api: {
      kernel: {
        invoke: async (
          pluginId: string,
          commandId: string,
          args: unknown,
          timeoutMs?: number,
        ) => {
          const call: InvokeCall = {
            pluginId,
            commandId,
            args: args as Record<string, unknown>,
            timeoutMs,
          }
          calls.push(call)
          return invokeImpl(call)
        },
      },
    },
    calls,
  }
}

// ── Session id prefix ────────────────────────────────────────────────────

test('isMarginSessionId: only matches the margin- prefix', () => {
  assert.equal(isMarginSessionId('margin-abc'), true)
  assert.equal(isMarginSessionId('cmdi-abc'), false)
  assert.equal(isMarginSessionId('chat-abc'), false)
  assert.equal(isMarginSessionId(''), false)
})

// ── Prompt shape ─────────────────────────────────────────────────────────

test('buildSuggestionPrompt: embeds the doc and asks for JSON only', () => {
  const prompt = buildSuggestionPrompt('alpha bravo charlie')
  assert.match(prompt, /alpha bravo charlie/)
  assert.match(prompt, /JSON/)
  // Spec lists every kind so the model knows the closed set.
  for (const kind of ['rephrase', 'tighten', 'fact-check', 'spelling', 'grammar']) {
    assert.match(prompt, new RegExp(kind))
  }
})

// ── Parser ───────────────────────────────────────────────────────────────

test('parseSuggestionsResponse: anchors a bare-JSON array against the doc', () => {
  const doc = 'The quick brown fox jumps over the lazy dog.'
  const json = JSON.stringify([
    { kind: 'tighten', original: 'quick brown', replacement: 'fast', message: 'shorter' },
    { kind: 'spelling', original: 'lazy', replacement: 'lazy', message: 'no fix' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'req-X')
  assert.equal(out.length, 2)
  assert.equal(out[0].id, 'req-X-0')
  assert.equal(out[0].kind, 'tighten')
  assert.equal(out[0].rangeFrom, doc.indexOf('quick brown'))
  assert.equal(out[0].rangeTo, doc.indexOf('quick brown') + 'quick brown'.length)
  assert.equal(out[0].original, 'quick brown')
  assert.equal(out[0].replacement, 'fast')
  assert.equal(out[0].line, 1)
  assert.equal(out[0].generatedFor, 1)
  assert.equal(out[1].id, 'req-X-1')
  assert.equal(out[1].kind, 'spelling')
  assert.equal(out[1].rangeFrom, doc.indexOf('lazy'))
})

test('parseSuggestionsResponse: tolerates a ```json fenced block', () => {
  const doc = 'hello world'
  const fenced = '```json\n[{"kind":"rephrase","original":"hello","replacement":"hi","message":"shorter"}]\n```'
  const out = parseSuggestionsResponse(fenced, doc, 1, 'req-Y')
  assert.equal(out.length, 1)
  assert.equal(out[0].original, 'hello')
})

test('parseSuggestionsResponse: drops entries whose original is not in the doc', () => {
  const doc = 'alpha bravo'
  const json = JSON.stringify([
    { kind: 'rephrase', original: 'alpha', replacement: 'a', message: 'ok' },
    { kind: 'rephrase', original: 'NOT_IN_DOC', replacement: 'x', message: 'hallucinated' },
    { kind: 'rephrase', original: 'bravo', replacement: 'b', message: 'ok' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'req-Z')
  assert.equal(out.length, 2, 'hallucinated entry must be filtered')
  assert.deepEqual(out.map((s) => s.original), ['alpha', 'bravo'])
})

test('parseSuggestionsResponse: drops unknown kinds', () => {
  const doc = 'hello world'
  const json = JSON.stringify([
    { kind: 'paraphrase', original: 'hello', replacement: 'hi', message: 'unknown kind' },
    { kind: 'tighten', original: 'world', replacement: 'w', message: 'ok' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'req-K')
  assert.equal(out.length, 1)
  assert.equal(out[0].original, 'world')
})

test('parseSuggestionsResponse: dedupes by kind|original', () => {
  const doc = 'foo foo foo'
  const json = JSON.stringify([
    { kind: 'tighten', original: 'foo', replacement: 'f', message: 'first' },
    { kind: 'tighten', original: 'foo', replacement: 'f', message: 'duplicate' },
    { kind: 'rephrase', original: 'foo', replacement: 'f', message: 'different kind, kept' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'req-D')
  assert.equal(out.length, 2)
  assert.equal(out[0].kind, 'tighten')
  assert.equal(out[1].kind, 'rephrase')
})

test('parseSuggestionsResponse: caps at MAX_SUGGESTIONS_PER_PASS (6)', () => {
  // Doc with 8 distinct words; emit 8 suggestions and assert cap.
  const words = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight']
  const doc = words.join(' ')
  const json = JSON.stringify(
    words.map((w) => ({ kind: 'tighten', original: w, replacement: w[0], message: 'cap' })),
  )
  const out = parseSuggestionsResponse(json, doc, 1, 'req-C')
  assert.equal(out.length, 6)
  assert.deepEqual(out.map((s) => s.original), words.slice(0, 6))
})

test('parseSuggestionsResponse: anchors out-of-order entries by falling back to a from-zero scan', () => {
  // Model emits 'world' before 'hello' — the cursor advance would
  // miss it; the parser falls back to scanning from 0 so we don't
  // drop a valid suggestion just because of emission order.
  const doc = 'hello world'
  const json = JSON.stringify([
    { kind: 'rephrase', original: 'world', replacement: 'w', message: 'ok' },
    { kind: 'rephrase', original: 'hello', replacement: 'h', message: 'ok' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'req-O')
  assert.equal(out.length, 2)
  assert.deepEqual(out.map((s) => s.original), ['world', 'hello'])
})

test('parseSuggestionsResponse: returns [] for non-JSON / non-array input', () => {
  const doc = 'hello world'
  assert.deepEqual(parseSuggestionsResponse('not json at all', doc, 1, 'r'), [])
  assert.deepEqual(parseSuggestionsResponse('{"not":"an array"}', doc, 1, 'r'), [])
  assert.deepEqual(parseSuggestionsResponse('', doc, 1, 'r'), [])
})

test('parseSuggestionsResponse: caps message length at 120 chars', () => {
  const doc = 'hello world'
  const longMsg = 'x'.repeat(500)
  const json = JSON.stringify([
    { kind: 'tighten', original: 'hello', replacement: 'h', message: longMsg },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'r')
  assert.equal(out.length, 1)
  assert.equal(out[0].message.length, 120)
})

test('parseSuggestionsResponse: replacement="" yields null (annotation-only)', () => {
  // fact-check is the canonical use: model flags a span without
  // proposing a rewrite.
  const doc = 'the moon orbits earth'
  const json = JSON.stringify([
    { kind: 'fact-check', original: 'orbits', replacement: '', message: 'verify' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'r')
  assert.equal(out.length, 1)
  assert.equal(out[0].replacement, null)
})

test('parseSuggestionsResponse: line is 1-based and tracks newlines', () => {
  const doc = 'first line\nsecond line\nthird line'
  const json = JSON.stringify([
    { kind: 'rephrase', original: 'third', replacement: 't', message: 'ok' },
  ])
  const out = parseSuggestionsResponse(json, doc, 1, 'r')
  assert.equal(out.length, 1)
  assert.equal(out[0].line, 3)
})

// ── requestPass: wires invoke → store ────────────────────────────────────

test('requestPass: invokes com.nexus.ai::stream_chat with margin- session id', async () => {
  reset()
  const { api, calls } = stubApi(async () => ({ text: '[]' }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await requestPass(api as any, 'note.md', 'hello world')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.ai')
  assert.equal(calls[0].commandId, 'stream_chat')
  const sessionId = calls[0].args.session_id as string
  assert.equal(isMarginSessionId(sessionId), true)
  // Prompt is forwarded as the single user message.
  const messages = calls[0].args.messages as Array<{ role: string; content: string }>
  assert.equal(messages.length, 1)
  assert.equal(messages[0].role, 'user')
  assert.match(messages[0].content, /hello world/)
  // 30s pass timeout (tighter than chat's 60s).
  assert.equal(calls[0].timeoutMs, 30_000)
})

test('requestPass: parses success and writes the store', async () => {
  reset()
  const doc = 'The quick brown fox.'
  const json = JSON.stringify([
    { kind: 'tighten', original: 'quick brown', replacement: 'fast', message: 'shorter' },
  ])
  const { api } = stubApi(async () => ({ text: json }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await requestPass(api as any, 'note.md', doc)
  assert.equal(result.length, 1)
  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'done')
  assert.equal(s.suggestions.length, 1)
  assert.equal(s.suggestions[0].original, 'quick brown')
  assert.equal(s.currentDocPath, 'note.md')
  assert.equal(s.currentGeneration, 1, 'first pass after reset bumps the generation to 1')
  assert.equal(s.currentRequestId, null, 'request id clears on done')
})

test('requestPass: writes setError on transport failure and resolves []', async () => {
  reset()
  const { api } = stubApi(async () => {
    throw new Error('Transport: kernel down')
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await requestPass(api as any, 'note.md', 'hello')
  assert.deepEqual(result, [], 'engine swallows errors so background callers can fire-and-forget')
  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'error')
  assert.ok(s.lastError instanceof Error)
  assert.match(s.lastError!.message, /Transport: kernel down/)
})

test('requestPass: bumps generation per call', async () => {
  reset()
  const { api } = stubApi(async () => ({ text: '[]' }))
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await requestPass(api as any, 'note.md', 'a')
  assert.equal(useMarginSuggestStore.getState().currentGeneration, 1)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await requestPass(api as any, 'note.md', 'b')
  assert.equal(useMarginSuggestStore.getState().currentGeneration, 2)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await requestPass(api as any, 'note.md', 'c')
  assert.equal(useMarginSuggestStore.getState().currentGeneration, 3)
})

test('requestPass: stale result from a superseded pass is dropped by the store', async () => {
  reset()
  // First pass: a deferred resolver we hold open until the second
  // pass starts, so we can interleave the writes.
  let resolveFirst: ((v: { text: string }) => void) | null = null
  const firstPromise = new Promise<{ text: string }>((res) => {
    resolveFirst = res
  })
  const stubInvoke = async () => {
    if (!resolveFirst) {
      // Second pass — return immediately with empty result.
      return { text: '[]' }
    }
    return firstPromise
  }
  const { api } = stubApi(stubInvoke)

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const firstP = requestPass(api as any, 'note.md', 'foo bar')
  // Detach the resolver so the second pass takes the immediate path.
  const captured = resolveFirst
  resolveFirst = null
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  await requestPass(api as any, 'note.md', 'foo bar')
  // Now resolve the first pass with a payload that, were it not
  // stale, WOULD populate suggestions.
  captured!({
    text: JSON.stringify([
      { kind: 'tighten', original: 'foo', replacement: 'f', message: 'stale' },
    ]),
  })
  await firstP

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'done')
  assert.deepEqual(s.suggestions, [], 'first pass resolved AFTER it was superseded; staleness guard must drop the result')
  assert.equal(s.currentGeneration, 2)
})

test('requestPass: missing/invalid result.text yields zero suggestions but status=done', async () => {
  reset()
  const { api } = stubApi(async () => ({})) // no text field at all
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = await requestPass(api as any, 'note.md', 'doc body')
  assert.deepEqual(result, [])
  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'done', 'empty/bad text is not a transport error — it just means zero suggestions')
})
