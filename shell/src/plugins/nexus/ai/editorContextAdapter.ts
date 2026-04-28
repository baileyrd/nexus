// shell/src/plugins/nexus/ai/editorContextAdapter.ts
//
// BL-032 — first surface adapter for the Cmd+I overlay. Reports:
//
//   - the active editor's relative path + body (chip + prompt block)
//   - the current CodeMirror selection text (when non-empty)
//
// Lives in the AI plugin's directory rather than the editor plugin's
// because the editor plugin is loaded ahead of the AI plugin in the
// catalog (default-on vs default-off; see `shell/src/plugins/catalog.ts`)
// and we don't want a default-on plugin to depend on a default-off one.
//
// The adapter reads:
//   - `useEditorStore` for the active tab + its content snapshot.
//   - `getActiveCmView()` (exported from `editor/runtime.ts`) for the
//     current CodeMirror selection. The runtime registers/unregisters
//     the active view on EditorView mount/unmount, so this reads the
//     selection without taking a React dep on the editor module.
//
// Body cap: 8 KB. Past that we truncate + tag the chip so the model
// sees a deterministic prefix instead of a 200-line file silently
// blowing the prompt budget.

import { useEditorStore } from '../editor/editorStore'
import { getActiveCmView } from '../editor/runtime'
import {
  contextContributors,
  type ContextChip,
  type ContextContribution,
} from './contextContributors'

const FILE_BODY_BYTE_CAP = 8 * 1024

/** Truncate a body at the byte cap, preserving line boundaries when
 *  possible so the model never sees a half-fenced code block. */
function capBody(body: string): { body: string; truncated: boolean } {
  if (body.length <= FILE_BODY_BYTE_CAP) {
    return { body, truncated: false }
  }
  const slice = body.slice(0, FILE_BODY_BYTE_CAP)
  const lastNewline = slice.lastIndexOf('\n')
  const cut = lastNewline > FILE_BODY_BYTE_CAP / 2 ? lastNewline : slice.length
  return { body: body.slice(0, cut), truncated: true }
}

/** Derive the line range (1-indexed, inclusive) for a CM selection. */
function selectionLineRange(
  doc: { lineAt: (pos: number) => { number: number } },
  from: number,
  to: number,
): { fromLine: number; toLine: number } {
  return {
    fromLine: doc.lineAt(from).number,
    toLine: doc.lineAt(to).number,
  }
}

/** Build a contribution for the active editor tab. Returns null when
 *  no tab is active or the active tab has no body to surface. */
export function collectEditorContext(): ContextContribution | null {
  const state = useEditorStore.getState()
  const relpath = state.activeRelpath
  if (!relpath) return null

  const tab = state.tabs.find((t) => t.relpath === relpath)
  if (!tab) return null

  const chips: ContextChip[] = []
  // BL-033 — per-chip blocks so click-to-remove on Selection doesn't
  // drop the file chip's body (and vice versa). The combined
  // `promptBlock` is still emitted as a fallback for callers that
  // ignore `chipPromptBlocks`.
  const chipPromptBlocks: Record<string, string> = {}

  // ── current file ──
  const { body: cappedBody, truncated } = capBody(tab.content ?? '')
  const fileChipLabel = truncated ? `${relpath} · truncated` : relpath
  const fileChipId = `editor:file:${relpath}`
  chips.push({
    id: fileChipId,
    label: fileChipLabel,
    kind: 'file',
  })
  if (cappedBody.length > 0) {
    chipPromptBlocks[fileChipId] =
      `### Current file: \`${relpath}\`${truncated ? ' (truncated)' : ''}\n\n` +
      '```\n' +
      cappedBody +
      '\n```'
  }

  // ── current selection ──
  const cmView = getActiveCmView()
  if (cmView) {
    const sel = cmView.state.selection.main
    if (!sel.empty) {
      const text = cmView.state.sliceDoc(sel.from, sel.to)
      // CM lineAt is 1-indexed and inclusive on both ends.
      const { fromLine, toLine } = selectionLineRange(
        cmView.state.doc,
        sel.from,
        Math.max(sel.from, sel.to - 1),
      )
      const lines = toLine - fromLine + 1
      const selectionChipId = `editor:selection:${relpath}:${sel.from}-${sel.to}`
      chips.push({
        id: selectionChipId,
        label: `Selection · L${fromLine}–L${toLine} (${lines} ${lines === 1 ? 'line' : 'lines'})`,
        kind: 'selection',
      })
      chipPromptBlocks[selectionChipId] =
        `### Selection in \`${relpath}\` (lines ${fromLine}–${toLine})\n\n` +
        '```\n' +
        text +
        '\n```'
    }
  }

  if (chips.length === 0) return null

  const blocks = chips
    .map((c) => chipPromptBlocks[c.id])
    .filter((b): b is string => !!b)

  return {
    surfaceId: 'editor',
    chips,
    promptBlock: blocks.join('\n\n'),
    chipPromptBlocks,
  }
}

/**
 * Register the editor adapter against the shared registry. Returns
 * the disposer; the AI plugin's activate tracks it through
 * `PluginRegistry.trackSubscription` so unloads sweep it.
 */
export function registerEditorContextAdapter(): () => void {
  return contextContributors.register('editor', collectEditorContext)
}
