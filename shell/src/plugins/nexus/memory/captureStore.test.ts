// BL-043 unit tests for the capture store + commit pipeline.
//
// The kernel-side `note_append` handler is covered in
// `crates/nexus-storage/src/core_plugin.rs::tests`. This file pins the
// shell-side contract:
//
//   1. captureCommit calls com.nexus.storage::note_append with the
//      configured inboxPath and a timestamped snippet.
//   2. error path keeps the overlay open and stores the kernel error
//      message.
//   3. clipboard pre-fill is best-effort — a clipboard read rejection
//      still opens an empty draft.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  buildSnippet,
  commitCapture,
  readClipboardBestEffort,
  useCaptureStore,
  type CaptureSourceMeta,
} from './captureStore.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
}

/** Build a minimal KernelAPI stub. Mirrors the AI store's pattern in
 *  `aiStore.test.ts` — only `invoke` is real; the rest is a placeholder
 *  the commit pipeline never touches. */
function stubKernel(handler: (commandId: string, args: unknown) => unknown) {
  const calls: InvokeCall[] = []
  const api = {
    invoke: async (pluginId: string, commandId: string, args: unknown) => {
      calls.push({ pluginId, commandId, args })
      return handler(commandId, args)
    },
    on: async () => () => {},
    available: async () => true,
  }
  return { api, calls }
}

function reset(): void {
  useCaptureStore.getState().close()
  useCaptureStore.getState().setError(null)
}

const SOURCE_META: CaptureSourceMeta = {
  app: 'Nexus',
  capturedAt: '2026-04-28T12:00:00.000Z',
}

test('captureCommit calls com.nexus.storage::note_append with the configured inboxPath and a timestamped snippet', async () => {
  reset()
  // Open the overlay so we can verify it gets closed on success.
  useCaptureStore.getState().openOverlay('hello world', SOURCE_META)

  const { api, calls } = stubKernel((cmd) => {
    assert.equal(cmd, 'note_append')
    return {
      path: 'Captures/Inbox.md',
      size_bytes: 42,
      modified_at: 1_700_000_000,
      content_hash: 'deadbeef',
    }
  })

  const result = await commitCapture({
    api,
    inboxPath: 'Captures/Inbox.md',
    draft: 'hello world',
    sourceMeta: SOURCE_META,
  })

  assert.equal(result.ok, true)
  assert.equal(calls.length, 1, 'exactly one kernel invocation')
  assert.equal(calls[0].pluginId, 'com.nexus.storage')
  assert.equal(calls[0].commandId, 'note_append')

  const args = calls[0].args as { path: string; snippet: string }
  assert.equal(args.path, 'Captures/Inbox.md', 'inboxPath flows through unchanged')
  // The snippet must contain the captured-at timestamp + the source app
  // label + the user draft, separated by blank lines as the BL-043 plan
  // §11 prescribes.
  assert.match(args.snippet, /^## Captured at 2026-04-28T12:00:00\.000Z/, 'starts with timestamped heading')
  assert.match(args.snippet, /Source: Nexus/, 'records source app')
  assert.match(args.snippet, /hello world/, 'includes the user draft')
  // Exactly one trailing newline so subsequent appends don't drift.
  assert.match(args.snippet, /\n$/)

  // On success the overlay is closed and any prior error is cleared.
  const after = useCaptureStore.getState()
  assert.equal(after.open, false, 'overlay closes on success')
  assert.equal(after.error, null, 'no error stored on success')
})

test('error path keeps the overlay open and stores the kernel error message', async () => {
  reset()
  useCaptureStore.getState().openOverlay('a draft', SOURCE_META)

  const { api } = stubKernel(() => {
    throw new Error('PluginNotFound: com.nexus.storage')
  })

  const result = await commitCapture({
    api,
    inboxPath: 'Inbox.md',
    draft: 'a draft',
    sourceMeta: SOURCE_META,
  })

  assert.equal(result.ok, false)
  if (result.ok === false) {
    assert.match(result.error, /PluginNotFound/)
  }

  const after = useCaptureStore.getState()
  assert.equal(after.open, true, 'overlay stays open so the user can retry')
  assert.equal(after.draft, 'a draft', 'draft is preserved on error')
  assert.match(after.error ?? '', /PluginNotFound/, 'error message is exposed via the store')
})

test('clipboard pre-fill is best-effort: a clipboard read rejection still opens an empty draft', async () => {
  reset()

  // Stub navigator.clipboard so readText rejects. The util must catch
  // the rejection and fall back to the empty string — never propagate.
  const originalNavigator = globalThis.navigator
  ;(globalThis as { navigator: unknown }).navigator = {
    clipboard: {
      readText: () => Promise.reject(new Error('NotAllowedError: permission denied')),
    },
  }

  try {
    const text = await readClipboardBestEffort()
    assert.equal(text, '', 'rejection collapses to empty string')

    // The plugin's captureOpen handler then calls openOverlay with that
    // empty string — we mirror that here so the assertion is end-to-end.
    useCaptureStore.getState().openOverlay(text, SOURCE_META)
    const state = useCaptureStore.getState()
    assert.equal(state.open, true, 'overlay opens despite clipboard failure')
    assert.equal(state.draft, '', 'draft is empty when the clipboard read failed')
    assert.equal(state.sourceMeta.app, 'Nexus', 'sourceMeta still flows through')
  } finally {
    ;(globalThis as { navigator: unknown }).navigator = originalNavigator
  }
})

test('readClipboardBestEffort returns empty string when navigator.clipboard is undefined', async () => {
  const originalNavigator = globalThis.navigator
  ;(globalThis as { navigator: unknown }).navigator = {}
  try {
    const text = await readClipboardBestEffort()
    assert.equal(text, '')
  } finally {
    ;(globalThis as { navigator: unknown }).navigator = originalNavigator
  }
})

test('buildSnippet shape: heading line, source line, draft, trailing newline', () => {
  const snippet = buildSnippet('multi\nline draft', SOURCE_META)
  // Reconstruct the expected exact string — pinned so a copy-paste of
  // the on-disk Inbox.md looks identical to what callers expect.
  assert.equal(
    snippet,
    [
      '## Captured at 2026-04-28T12:00:00.000Z',
      '',
      'Source: Nexus',
      '',
      'multi\nline draft',
      '',
    ].join('\n'),
  )
})
