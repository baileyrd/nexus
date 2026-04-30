// shell/src/plugins/nexus/editor/cm/marginSuggestTrigger.ts
//
// BL-036 phase 4 — idle-debounced trigger for the AMB margin-
// suggestions engine. The phase-1 engine + phase-2/3 rendering
// surfaces are already shipped; this is the thing that makes the
// feature actually run in production.
//
// Layout (mirrors `linkSuggest.ts` / `ghostCompletion.ts` so the
// three ambient surfaces feel uniform):
//
//   docChanged → reset idle timer
//        ↓
//   timer fires after `idleMs` of quiet
//        ↓
//   readSettings() — kill switch + length gates
//        ↓
//   getMarginApi() — short-circuit when AI plugin not yet active
//        ↓
//   requestPass(api, relpath, docText) — engine writes
//        suggestions to `useMarginSuggestStore`; the phase-2
//        `marginSuggestionsExt` CM extension reads them and paints.
//
// Settings (read lazily so live edits in Settings panel take effect
// without remounting the editor):
//
//   ai.marginSuggest.enabled       — master kill switch (default false; opt-in for v1)
//   ai.marginSuggest.idleMs        — quiet period after typing (default 5000)
//   ai.marginSuggest.minDocChars   — skip docs shorter than this (default 200)
//   ai.marginSuggest.maxDocChars   — skip docs longer than this (default 8000)
//
// Why opt-in by default: an idle pass costs a model call. Surfacing
// it without an explicit toggle would surprise users with first-time
// AI provider config dialogs. The phase-4 commit also turns on a
// settings UI row so flipping it is one click.
//
// Single-flight: the engine's store-side staleness guard already
// drops superseded results, so this layer doesn't re-implement it.
// Concurrent triggers across multiple open editors all funnel into
// one store; the most-recently-edited tab's pass wins.

import type { Extension } from '@codemirror/state'
import {
  ViewPlugin,
  type PluginValue,
  type ViewUpdate,
  EditorView,
} from '@codemirror/view'

import { configStore } from '../../../../stores/configStore'
import { getMarginApi } from '../../ai/marginApi'
import { requestPass } from '../../ai/marginSuggest'

/** Effective settings snapshot. Pure data so tests can construct
 *  one without standing up the configStore. */
export interface MarginSuggestSettings {
  enabled: boolean
  idleMs: number
  minDocChars: number
  maxDocChars: number
}

/** Defaults — exported so the manifest schema and the trigger agree
 *  by construction (no chance of a default drift between the two
 *  surfaces). */
export const MARGIN_SUGGEST_DEFAULTS: Readonly<MarginSuggestSettings> = {
  enabled: false,
  idleMs: 5000,
  minDocChars: 200,
  maxDocChars: 8000,
}

/** Configuration keys read by the trigger. Exported as constants
 *  so the AI plugin's `contributes.configuration.schema` can
 *  reference the same strings without copy-paste drift. */
export const MARGIN_SUGGEST_CONFIG_KEYS = {
  enabled: 'ai.marginSuggest.enabled',
  idleMs: 'ai.marginSuggest.idleMs',
  minDocChars: 'ai.marginSuggest.minDocChars',
  maxDocChars: 'ai.marginSuggest.maxDocChars',
} as const

/** Read the four settings off the configStore, falling back to the
 *  shared defaults. Pure read — does not mutate state. Exposed for
 *  tests; production callers go through `MarginSuggestTrigger`. */
export function readMarginSuggestSettings(): MarginSuggestSettings {
  return {
    enabled: configStore.get<boolean>(
      MARGIN_SUGGEST_CONFIG_KEYS.enabled,
      MARGIN_SUGGEST_DEFAULTS.enabled,
    ),
    idleMs: configStore.get<number>(
      MARGIN_SUGGEST_CONFIG_KEYS.idleMs,
      MARGIN_SUGGEST_DEFAULTS.idleMs,
    ),
    minDocChars: configStore.get<number>(
      MARGIN_SUGGEST_CONFIG_KEYS.minDocChars,
      MARGIN_SUGGEST_DEFAULTS.minDocChars,
    ),
    maxDocChars: configStore.get<number>(
      MARGIN_SUGGEST_CONFIG_KEYS.maxDocChars,
      MARGIN_SUGGEST_DEFAULTS.maxDocChars,
    ),
  }
}

/** Reason a pass should NOT fire, or null if the trigger is clear
 *  to call `requestPass`. Surfacing the reason as a string (rather
 *  than a bare boolean) lets the tests assert on WHY a pass was
 *  skipped — and gives us a hook for future telemetry / debug log
 *  without changing the call site. */
export type SkipReason =
  | 'disabled'
  | 'doc-too-short'
  | 'doc-too-long'
  | 'untitled'
  | null

/** Pure gate — given an effective settings snapshot, the doc text,
 *  and the relpath, decide whether the trigger should fire. */
export function shouldFirePass(
  settings: MarginSuggestSettings,
  docText: string,
  relpath: string,
): SkipReason {
  if (!settings.enabled) return 'disabled'
  // Untitled tabs have no kernel session so the suggestion couldn't
  // be persisted as a citation anchor; engineering decision is to
  // skip them rather than complicate the engine for a transient
  // surface. Mirrors the predicate in `editor/sessionManager.ts`.
  if (relpath.startsWith('untitled:')) return 'untitled'
  const len = docText.length
  if (len < settings.minDocChars) return 'doc-too-short'
  if (len > settings.maxDocChars) return 'doc-too-long'
  return null
}

class MarginSuggestTrigger implements PluginValue {
  private timer: ReturnType<typeof setTimeout> | null = null
  private readonly view: EditorView
  private readonly relpath: string

  constructor(view: EditorView, relpath: string) {
    this.view = view
    this.relpath = relpath
  }

  update(u: ViewUpdate): void {
    // Only doc edits reset the idle timer — selection changes and
    // viewport scrolls don't count as "typing". This is a lighter
    // contract than `linkSuggest` (which also resets on
    // selectionSet) because suggestions don't track caret position.
    if (u.docChanged) this.scheduleFetch()
  }

  destroy(): void {
    this.cancel()
  }

  private cancel(): void {
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
  }

  private scheduleFetch(): void {
    const settings = readMarginSuggestSettings()
    if (!settings.enabled) {
      // Don't even arm the timer when disabled — flipping the kill
      // switch should leave no in-flight surprise pass.
      this.cancel()
      return
    }
    // Reset on every keystroke so the user has to actually go idle
    // before a pass fires.
    this.cancel()
    this.timer = setTimeout(() => {
      this.timer = null
      void this.runFetch()
    }, settings.idleMs)
  }

  private async runFetch(): Promise<void> {
    // Re-read settings at fire time — the user may have flipped
    // them since the timer was armed.
    const settings = readMarginSuggestSettings()
    const docText = this.view.state.doc.toString()
    if (shouldFirePass(settings, docText, this.relpath) !== null) return
    const api = getMarginApi()
    if (!api) return // AI plugin not yet active; next edit will retry.
    await requestPass(api, this.relpath, docText)
  }
}

export interface MarginSuggestTriggerOptions {
  /** Forge-relative path of the doc this editor instance is
   *  showing. Threaded through to `requestPass` so the store's
   *  `currentDocPath` matches what the rendering extension filters
   *  on. */
  relpath: string
}

/** Build the BL-036 phase-4 idle-debounced trigger extension.
 *  Mounted alongside `marginSuggestionsExt` in the editor's live-
 *  mode extension list (see `EditorView.tsx`). Source-mode tabs
 *  don't mount it — the AMB pass over a raw markdown doc is a
 *  live-mode UX. */
export function marginSuggestTriggerExt(
  opts: MarginSuggestTriggerOptions,
): Extension {
  return ViewPlugin.define((view) => new MarginSuggestTrigger(view, opts.relpath))
}
