// shell/src/plugins/nexus/audio/speechApi.test.ts
//
// BL-118 — unit tests for the Web Speech API wrappers. Stubs
// `webkitSpeechRecognition` / `speechSynthesis` on the global
// `window` so node:test can drive the recognition + synthesis flows
// without a real browser.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  createRecognition,
  listVoices,
  probeSpeechSupport,
  speak,
  startRecognition,
} from './speechApi.ts'

// ── Test harness ───────────────────────────────────────────────────────────

interface FakeRecogEvent {
  results: Array<Array<{ transcript: string; confidence: number }> & { isFinal: boolean }>
}

class FakeRecognition {
  continuous = false
  interimResults = false
  lang = ''
  onresult: ((e: FakeRecogEvent) => void) | null = null
  onerror: ((e: { error: string }) => void) | null = null
  onend: ((e: unknown) => void) | null = null
  onstart: ((e: unknown) => void) | null = null
  started = false
  stopped = false
  start() {
    this.started = true
  }
  stop() {
    this.stopped = true
  }
  abort() {
    this.stopped = true
  }
  addEventListener(): void {}
  removeEventListener(): void {}
  dispatchEvent(): boolean {
    return true
  }

  /** Test helper — push one final result + onend. */
  emitFinal(text: string) {
    const group = [{ transcript: text, confidence: 1 }] as unknown as Array<{
      transcript: string
      confidence: number
    }> & { isFinal: boolean }
    Object.assign(group, { isFinal: true })
    this.onresult?.({ results: [group] })
    this.onend?.({})
  }
  /** Test helper — push one interim result without ending. */
  emitInterim(text: string) {
    const group = [{ transcript: text, confidence: 0.5 }] as unknown as Array<{
      transcript: string
      confidence: number
    }> & { isFinal: boolean }
    Object.assign(group, { isFinal: false })
    this.onresult?.({ results: [group] })
  }
  emitError(code: string) {
    this.onerror?.({ error: code })
  }
}

function installFakeRecognition(): {
  instance: FakeRecognition
  cleanup: () => void
} {
  const instance = new FakeRecognition()
  const w = globalThis as unknown as { window?: { webkitSpeechRecognition?: unknown } }
  if (!w.window) w.window = {}
  const prevCtor = w.window.webkitSpeechRecognition
  w.window.webkitSpeechRecognition = function () {
    return instance
  } as unknown as () => FakeRecognition
  return {
    instance,
    cleanup: () => {
      if (prevCtor === undefined) delete w.window!.webkitSpeechRecognition
      else w.window!.webkitSpeechRecognition = prevCtor
    },
  }
}

function installFakeSynthesis(): { calls: SpeechSynthesisUtterance[]; cleanup: () => void } {
  const w = globalThis as unknown as { window?: Record<string, unknown> }
  if (!w.window) w.window = {}
  const calls: SpeechSynthesisUtterance[] = []
  // jsdom doesn't ship speechSynthesis; install our own.
  const prev = w.window.speechSynthesis
  ;(globalThis as unknown as { SpeechSynthesisUtterance?: unknown }).SpeechSynthesisUtterance =
    class {
      text: string
      lang = ''
      rate = 1
      pitch = 1
      volume = 1
      voice: SpeechSynthesisVoice | null = null
      onend: (() => void) | null = null
      onerror: ((e: { error: string }) => void) | null = null
      constructor(t: string) {
        this.text = t
      }
    }
  w.window.speechSynthesis = {
    speak(u: SpeechSynthesisUtterance) {
      calls.push(u)
      // Fire onend synchronously so the test promise resolves
      // without needing a tick.
      setTimeout(() => {
        ;(u.onend as unknown as () => void)?.()
      }, 0)
    },
    cancel() {},
    getVoices(): SpeechSynthesisVoice[] {
      return [
        {
          voiceURI: 'test-en',
          name: 'Test English',
          lang: 'en-US',
          default: true,
          localService: true,
        } as SpeechSynthesisVoice,
      ]
    },
  }
  return {
    calls,
    cleanup: () => {
      if (prev === undefined) delete w.window!.speechSynthesis
      else w.window!.speechSynthesis = prev
    },
  }
}

// ── probeSpeechSupport ──────────────────────────────────────────────────────

test('probeSpeechSupport detects both surfaces when stubs installed', () => {
  const recog = installFakeRecognition()
  const synth = installFakeSynthesis()
  try {
    const s = probeSpeechSupport()
    assert.equal(s.stt, true)
    assert.equal(s.tts, true)
  } finally {
    recog.cleanup()
    synth.cleanup()
  }
})

test('probeSpeechSupport reports false when nothing installed', () => {
  // Wipe any prior stubs from sibling tests.
  const w = globalThis as unknown as { window?: Record<string, unknown> }
  const prevR = w.window?.webkitSpeechRecognition
  const prevS = w.window?.speechSynthesis
  if (w.window) {
    delete w.window.webkitSpeechRecognition
    delete w.window.speechSynthesis
  }
  try {
    const s = probeSpeechSupport()
    assert.equal(s.stt, false)
    assert.equal(s.tts, false)
  } finally {
    if (w.window) {
      if (prevR !== undefined) w.window.webkitSpeechRecognition = prevR
      if (prevS !== undefined) w.window.speechSynthesis = prevS
    }
  }
})

// ── createRecognition ──────────────────────────────────────────────────────

test('createRecognition returns null without a backing API', () => {
  const w = globalThis as unknown as { window?: Record<string, unknown> }
  const prev = w.window?.webkitSpeechRecognition
  if (w.window) delete w.window.webkitSpeechRecognition
  try {
    assert.equal(createRecognition(), null)
  } finally {
    if (w.window && prev !== undefined) w.window.webkitSpeechRecognition = prev
  }
})

// ── startRecognition ───────────────────────────────────────────────────────

test('startRecognition resolves with the final transcript', async () => {
  const fake = installFakeRecognition()
  try {
    const handle = startRecognition()
    assert.ok(handle, 'expected a handle')
    fake.instance.emitFinal('hello world')
    const text = await handle.done
    assert.equal(text, 'hello world')
    assert.equal(fake.instance.started, true)
  } finally {
    fake.cleanup()
  }
})

test('startRecognition forwards lang when provided', () => {
  const fake = installFakeRecognition()
  try {
    startRecognition({ lang: 'fr-FR' })
    assert.equal(fake.instance.lang, 'fr-FR')
    // continuous defaults to false for one-shot capture
    assert.equal(fake.instance.continuous, false)
  } finally {
    fake.cleanup()
  }
})

test('startRecognition emits interim chunks when configured', async () => {
  const fake = installFakeRecognition()
  try {
    const interims: string[] = []
    const handle = startRecognition({
      interim: true,
      onInterim: (t) => interims.push(t),
    })
    assert.ok(handle)
    fake.instance.emitInterim('hello')
    fake.instance.emitInterim('hello world')
    fake.instance.emitFinal('hello world!')
    await handle.done
    assert.deepEqual(interims, ['hello', 'hello world'])
  } finally {
    fake.cleanup()
  }
})

test('startRecognition treats no-speech as a clean empty result', async () => {
  const fake = installFakeRecognition()
  try {
    const handle = startRecognition()
    assert.ok(handle)
    fake.instance.emitError('no-speech')
    const text = await handle.done
    assert.equal(text, '')
  } finally {
    fake.cleanup()
  }
})

test('startRecognition rejects with the permission error code', async () => {
  const fake = installFakeRecognition()
  try {
    const handle = startRecognition()
    assert.ok(handle)
    fake.instance.emitError('not-allowed')
    await assert.rejects(handle.done, /not-allowed/)
  } finally {
    fake.cleanup()
  }
})

test('startRecognition cancel() resolves with partial text and stops the recogniser', async () => {
  const fake = installFakeRecognition()
  try {
    const handle = startRecognition()
    assert.ok(handle)
    fake.instance.emitInterim('partial')
    handle.cancel()
    fake.instance.emitError('aborted')
    const text = await handle.done
    assert.equal(text, '')
    assert.equal(fake.instance.stopped, true)
  } finally {
    fake.cleanup()
  }
})

// ── speak / listVoices ────────────────────────────────────────────────────

test('listVoices reflects platform voices', () => {
  const synth = installFakeSynthesis()
  try {
    const v = listVoices()
    assert.equal(v.length, 1)
    assert.equal(v[0]!.voiceURI, 'test-en')
    assert.equal(v[0]!.default, true)
  } finally {
    synth.cleanup()
  }
})

test('speak resolves when the utterance ends and applies options', async () => {
  const synth = installFakeSynthesis()
  try {
    const handle = speak('hello', { rate: 1.5, voice: 'test-en', lang: 'en-US' })
    assert.ok(handle, 'expected a handle')
    await handle.done
    assert.equal(synth.calls.length, 1)
    const u = synth.calls[0]!
    assert.equal(u.text, 'hello')
    assert.equal(u.lang, 'en-US')
    assert.equal(u.rate, 1.5)
  } finally {
    synth.cleanup()
  }
})

test('speak clamps rate to the legal range', () => {
  const synth = installFakeSynthesis()
  try {
    speak('boom', { rate: 99 })
    assert.equal(synth.calls[0]!.rate, 10)
    speak('whisper', { rate: 0.0001 })
    assert.equal(synth.calls[1]!.rate, 0.1)
  } finally {
    synth.cleanup()
  }
})

test('speak returns null when speechSynthesis is unavailable', () => {
  const w = globalThis as unknown as { window?: Record<string, unknown> }
  const prev = w.window?.speechSynthesis
  if (w.window) delete w.window.speechSynthesis
  try {
    assert.equal(speak('nope'), null)
  } finally {
    if (w.window && prev !== undefined) w.window.speechSynthesis = prev
  }
})
