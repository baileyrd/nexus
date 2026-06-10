// shell/src/plugins/nexus/editor/cm/marginSuggestTrigger.test.ts
//
// BL-036 phase 4 — coverage for the idle-debounced trigger.
//
// Two test surfaces:
//
//   1. `shouldFirePass` + `readMarginSuggestSettings` — pure logic,
//      runs anywhere. Covers the kill switch, doc-length gates, and
//      the untitled-tab carve-out.
//
//   2. End-to-end: mount a real EditorView (via happy-dom) with the
//      trigger extension, stub `getMarginApi` so we can intercept
//      `requestPass`'s call site, fire docChanged events, advance
//      `setTimeout` via the shared `MockTimers` from node:test, and
//      assert on whether / when a pass fires.
//
// Run:
//   node --import tsx --import ./tests/setup/happy-dom.ts --test \
//     shell/src/plugins/nexus/editor/cm/marginSuggestTrigger.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import {
  MARGIN_SUGGEST_CONFIG_KEYS,
  MARGIN_SUGGEST_DEFAULTS,
  marginSuggestTriggerExt,
  readMarginSuggestSettings,
  shouldFirePass,
  type MarginSuggestSettings,
} from './marginSuggestTrigger.ts'
import { useConfigStore } from '../../../../stores/configStore.ts'
import { stubPluginAPI } from '../../../../testing/typedStubs.ts'
import {
  _resetMarginApiForTests,
  setMarginApi,
} from '../../ai/marginApi.ts'
import { useMarginSuggestStore } from '../../ai/marginSuggestStore.ts'

function resetConfig(): void {
  useConfigStore.setState({ values: {} })
}

function makeSettings(overrides: Partial<MarginSuggestSettings> = {}): MarginSuggestSettings {
  return { ...MARGIN_SUGGEST_DEFAULTS, ...overrides }
}

// ── shouldFirePass ──────────────────────────────────────────────────────

test('shouldFirePass: returns "disabled" when the kill switch is off', () => {
  const reason = shouldFirePass(makeSettings({ enabled: false }), 'x'.repeat(500), 'note.md')
  assert.equal(reason, 'disabled')
})

test('shouldFirePass: returns "untitled" for an untitled-tab relpath', () => {
  const settings = makeSettings({ enabled: true })
  const reason = shouldFirePass(settings, 'x'.repeat(500), 'untitled:1')
  assert.equal(reason, 'untitled', 'untitled tabs have no kernel session — skip cleanly')
})

test('shouldFirePass: returns "doc-too-short" below minDocChars', () => {
  const settings = makeSettings({ enabled: true, minDocChars: 200 })
  const reason = shouldFirePass(settings, 'x'.repeat(199), 'note.md')
  assert.equal(reason, 'doc-too-short')
})

test('shouldFirePass: returns "doc-too-long" above maxDocChars', () => {
  const settings = makeSettings({ enabled: true, maxDocChars: 500 })
  const reason = shouldFirePass(settings, 'x'.repeat(501), 'note.md')
  assert.equal(reason, 'doc-too-long')
})

test('shouldFirePass: exact-length boundaries are inclusive (within bounds)', () => {
  const settings = makeSettings({ enabled: true, minDocChars: 200, maxDocChars: 500 })
  assert.equal(shouldFirePass(settings, 'x'.repeat(200), 'note.md'), null)
  assert.equal(shouldFirePass(settings, 'x'.repeat(500), 'note.md'), null)
})

test('shouldFirePass: returns null when everything checks out', () => {
  const settings = makeSettings({ enabled: true, minDocChars: 0, maxDocChars: 99999 })
  assert.equal(shouldFirePass(settings, 'hello world', 'note.md'), null)
})

// ── readMarginSuggestSettings ───────────────────────────────────────────

test('readMarginSuggestSettings: returns defaults when configStore is empty', () => {
  resetConfig()
  const s = readMarginSuggestSettings()
  assert.deepEqual(s, MARGIN_SUGGEST_DEFAULTS)
})

test('readMarginSuggestSettings: reflects configStore overrides', () => {
  resetConfig()
  useConfigStore.setState({
    values: {
      [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true,
      [MARGIN_SUGGEST_CONFIG_KEYS.idleMs]: 2000,
      [MARGIN_SUGGEST_CONFIG_KEYS.minDocChars]: 50,
      [MARGIN_SUGGEST_CONFIG_KEYS.maxDocChars]: 12000,
    },
  })
  const s = readMarginSuggestSettings()
  assert.deepEqual(s, {
    enabled: true,
    idleMs: 2000,
    minDocChars: 50,
    maxDocChars: 12000,
  })
})

// ── End-to-end (mount EditorView, fire docChanged, intercept api.kernel.invoke) ──

interface CapturedInvoke {
  pluginId: string
  commandId: string
  args: Record<string, unknown>
}

/** Build a stub `PluginAPI` whose only purpose is to capture the
 *  `kernel.invoke` calls made by `requestPass`. Returns the captured
 *  list AND the api so the test can pass it to `setMarginApi`. */
function stubMarginApi(
  responseText = '[]',
): { calls: CapturedInvoke[]; install: () => void } {
  const calls: CapturedInvoke[] = []
  const api = stubPluginAPI({
    kernel: {
      invoke: async <T>(
        pluginId: string,
        commandId: string,
        args?: unknown,
      ): Promise<T> => {
        calls.push({
          pluginId,
          commandId,
          args: args as Record<string, unknown>,
        })
        // T is the production call site's expectation (StreamChatResult);
        // the test owns the payload shape.
        return { text: responseText } as T
      },
    },
  })
  return {
    calls,
    install: () => {
      setMarginApi(api)
    },
  }
}

function makeView(doc: string, relpath: string): EditorView {
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  return new EditorView({
    state: EditorState.create({
      doc,
      extensions: [marginSuggestTriggerExt({ relpath })],
    }),
    parent,
  })
}

function fullReset(): void {
  resetConfig()
  _resetMarginApiForTests()
  useMarginSuggestStore.getState().clear()
}

/** Flush queued microtasks WITHOUT going through `setTimeout` —
 *  the mocked timers freeze setTimeout, so the usual
 *  `await new Promise(r => setTimeout(r, 0))` hangs the test.
 *
 *  The trigger's `runFetch` is `async`, and `requestPass` awaits
 *  `api.kernel.invoke` (one `await`) then writes the store. Three
 *  `Promise.resolve()` ticks reliably drain that chain — measured
 *  against the actual call graph. If you add more awaits to the
 *  fire path, bump this. */
async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve()
  }
}

test('trigger: dormant when ai.marginSuggest.enabled is false (default)', async (t) => {
  fullReset()
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('long enough doc body '.repeat(50), 'note.md')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    t.mock.timers.tick(120_000) // way past any plausible idleMs
    // Settle promise microtasks (await of in-flight requestPass).
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'disabled trigger never reaches the kernel')
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: fires once after idleMs of quiet when enabled', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: { [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('long enough doc body '.repeat(50), 'note.md')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    // Just before idle threshold — must NOT have fired yet.
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs - 1)
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'pre-idle: timer hasn\'t expired yet')
    // Cross the threshold.
    t.mock.timers.tick(2)
    await flushMicrotasks()
    assert.equal(calls.length, 1, 'idle threshold crossed → one pass fires')
    assert.equal(calls[0].pluginId, 'com.nexus.ai')
    assert.equal(calls[0].commandId, 'stream_chat')
    const sessionId = calls[0].args.session_id as string
    assert.match(sessionId, /^margin-/)
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: each docChanged resets the timer (debounce coalesces)', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: { [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('long enough doc body '.repeat(50), 'note.md')
  try {
    // Type three times in quick succession. Each one must reset the
    // idle timer; only the final quiet period should produce a pass.
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs - 1)
    view.dispatch({ changes: { from: 0, to: 0, insert: 'b' } })
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs - 1)
    view.dispatch({ changes: { from: 0, to: 0, insert: 'c' } })
    // At this point cumulative time ≈ 2*(idleMs-1) but no FULL idle
    // window elapsed since the last edit — pass must NOT have fired.
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'sequential edits coalesce — only one pass after final idle')
    // Now go quiet for a full idleMs.
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs + 1)
    await flushMicrotasks()
    assert.equal(calls.length, 1)
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: skips when doc is shorter than minDocChars', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: {
      [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true,
      [MARGIN_SUGGEST_CONFIG_KEYS.minDocChars]: 1000,
    },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('short', 'note.md')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs + 10)
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'doc-too-short gate fires before kernel.invoke')
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: skips untitled-tab relpaths', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: { [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('long enough doc body '.repeat(50), 'untitled:1')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs + 10)
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'untitled tabs are skipped')
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: short-circuits silently when getMarginApi returns null', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: { [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  // No `setMarginApi(...)` call — handle is null.
  const view = makeView('long enough doc body '.repeat(50), 'note.md')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs + 10)
    await flushMicrotasks()
    // No throw, no pass — store stays idle.
    assert.equal(useMarginSuggestStore.getState().status, 'idle')
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})

test('trigger: re-reads settings at fire time (live edit takes effect mid-debounce)', async (t) => {
  fullReset()
  useConfigStore.setState({
    values: { [MARGIN_SUGGEST_CONFIG_KEYS.enabled]: true },
  })
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { calls, install } = stubMarginApi()
  install()
  const view = makeView('long enough doc body '.repeat(50), 'note.md')
  try {
    view.dispatch({ changes: { from: 0, to: 0, insert: 'a' } })
    // Halfway through the idle window the user flips the kill switch.
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs / 2)
    useConfigStore.getState().set(MARGIN_SUGGEST_CONFIG_KEYS.enabled, false)
    t.mock.timers.tick(MARGIN_SUGGEST_DEFAULTS.idleMs)
    await flushMicrotasks()
    assert.equal(calls.length, 0, 'flip-to-disabled mid-debounce must take effect')
  } finally {
    view.destroy()
    t.mock.timers.reset()
  }
})
