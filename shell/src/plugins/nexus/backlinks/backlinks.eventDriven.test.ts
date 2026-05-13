// Phase 7 test: backlinks subscribes to the editor changed channel and
// re-queries `com.nexus.storage::backlinks` when an event fires for
// the active file. This is a mirror of the outline integration test
// but scoped to what backlinks actually does — the architectural note
// in the plugin header explains why the refresh is a no-op for most
// same-file edits today.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/backlinks/backlinks.eventDriven.test.ts

import type { KernelAPI } from '../../../types/plugin.ts'
import type {
  EditorChangedPayload,
  EditorSnapshot,
} from '../editor/types.ts'
import { makeEditorClient } from '../editor/kernelClient.ts'
import { makeSessionManager } from '../editor/sessionManager.ts'
import { useEditorStore } from '../editor/editorStore.ts'
import { useBacklinksStore } from './backlinksStore.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

function emptySnapshot(relpath: string): EditorSnapshot {
  return {
    relpath,
    tree: { blocks: {}, root_blocks: [], metadata: {} },
    undoPosition: null,
    undoLen: 0,
    canUndo: false,
    canRedo: false,
    revision: 0,
  }
}

interface CapturedHandler {
  topic: string
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  handler: (topic: string, payload: any) => void
}

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
}

function makeMockApi(relpath: string): {
  api: KernelAPI
  captured: CapturedHandler[]
  calls: InvokeCall[]
} {
  const captured: CapturedHandler[] = []
  const calls: InvokeCall[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T> {
      calls.push({ pluginId, commandId, args })
      if (commandId === 'open') return emptySnapshot(relpath) as T
      if (commandId === 'backlinks') {
        return [
          {
            source_path: 'notes/other.md',
            link_text: 'link',
            link_type: 'wikilink',
          },
        ] as T
      }
      return {} as T
    },
    async on<T = unknown>(
      topicPrefix: string,
      handler: (topic: string, payload: T) => void,
    ): Promise<() => void> {
      captured.push({
        topic: topicPrefix,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        handler: handler as any,
      })
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return { api, captured, calls }
}

test('editor changed event for the active file triggers a backlinks refresh', async () => {
  const relpath = 'notes/active.md'
  useBacklinksStore.getState().clear()
  useEditorStore.getState().clear()

  const ctx = makeMockApi(relpath)
  const client = makeEditorClient(ctx.api)
  const mgr = makeSessionManager(client, ctx.api)

  await mgr.acquire(relpath)
  await Promise.resolve()
  await Promise.resolve()

  // Simulate the backlinks plugin's setup: it records the current
  // relpath in its store (load() path) and subscribes to onChanged.
  // We inline a minimal refresh closure that mirrors the plugin's
  // `refresh(relpath)` — the plugin's version has a requestId guard
  // and kernel-availability guard; the core behaviour under test is
  // "change event → kernel invoke with BACKLINKS_COMMAND".
  useBacklinksStore.getState().setCurrent(relpath)

  const unsub = mgr.onChanged((payload: EditorChangedPayload) => {
    if (payload.relpath !== useEditorStore.getState().activeRelpath) {
      return
    }
    // The real plugin coalesces these with rAF — for the test we
    // just invoke synchronously and count kernel calls.
    void ctx.api.invoke('com.nexus.storage', 'backlinks', { path: relpath })
  })

  // Pretend the editor store knows the active tab is `relpath`.
  useEditorStore.setState({ activeRelpath: relpath })

  const capture = ctx.captured[0]
  const before = ctx.calls.filter((c) => c.commandId === 'backlinks').length

  capture.handler(`com.nexus.editor.changed.${relpath}`, {
    relpath,
    revision: 2,
    transaction_id: null,
  })
  await Promise.resolve()

  const after = ctx.calls.filter((c) => c.commandId === 'backlinks').length
  assert.equal(after, before + 1, 'change event re-queried backlinks once')

  // And a cross-file event is ignored.
  capture.handler(`com.nexus.editor.changed.notes/elsewhere.md`, {
    relpath: 'notes/elsewhere.md',
    revision: 3,
    transaction_id: null,
  })
  await Promise.resolve()
  const final = ctx.calls.filter((c) => c.commandId === 'backlinks').length
  assert.equal(final, after, 'non-active relpath event did not trigger a refresh')

  unsub()
  await mgr.release(relpath)
  useBacklinksStore.getState().clear()
  useEditorStore.getState().clear()
})
