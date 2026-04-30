// Capture overlay state for BL-043 — quick-capture hotkey.
//
// The store owns the overlay open/closed flag, the in-flight draft, the
// last error from a kernel-side append, and the source-metadata snapshot
// taken at hotkey-press time. The plugin's command handlers (`captureOpen`
// / `captureCommit` in `index.ts`) drive transitions; the React overlay
// (`CaptureOverlay.tsx`) reads from this store and never touches the
// kernel directly.

import { create } from 'zustand'

import type { KernelAPI } from '../../../types/plugin.ts'
import { appendInbox } from './kernelClient.ts'
import {
  buildCodeSnippetSection,
  type CodeSourceMeta,
} from './codeCapture.ts'

/** Lightweight provenance recorded when the hotkey fires. */
export interface CaptureSourceMeta {
  /**
   * Best-effort source label. Defaults to `document.title` so the appended
   * snippet has at least the window name; bigger plugin layers can extend
   * this in v2 (system-frontmost-app via the platform API, browser tab
   * URL, …) without changing the on-disk shape.
   */
  app: string
  /** ISO-8601 capture timestamp, set when `captureOpen` runs. */
  capturedAt: string
  /** BL-046 — code-source provenance. Present when the capture
   *  came from an IDE selection / `captureCode` IPC; absent for
   *  the plain hotkey path so the on-disk format is unchanged. */
  code?: CodeSourceMeta
}

/** Default source metadata used until `captureOpen` overwrites it. */
function emptySourceMeta(): CaptureSourceMeta {
  return { app: '', capturedAt: '' }
}

interface CaptureStore {
  /** True while the modal is visible. */
  open: boolean
  /** Live textarea contents. */
  draft: string
  /** Last kernel-append error message; cleared at the start of a new commit. */
  error: string | null
  /** Source provenance recorded when the hotkey fired. */
  sourceMeta: CaptureSourceMeta

  /** Show the overlay with a (possibly clipboard-prefilled) draft. */
  openOverlay(draft: string, sourceMeta: CaptureSourceMeta): void
  /** Mutate the draft as the user types. */
  setDraft(text: string): void
  /** Hide the overlay and clear transient state. */
  close(): void
  /** Stash an error string and keep the overlay open. */
  setError(message: string | null): void
}

export const useCaptureStore = create<CaptureStore>((set) => ({
  open: false,
  draft: '',
  error: null,
  sourceMeta: emptySourceMeta(),

  openOverlay(draft, sourceMeta) {
    set({ open: true, draft, error: null, sourceMeta })
  },
  setDraft(text) {
    set({ draft: text })
  },
  close() {
    set({ open: false, draft: '', error: null, sourceMeta: emptySourceMeta() })
  },
  setError(message) {
    set({ error: message })
  },
}))

/**
 * Build the snippet body that gets appended to the inbox. Centralised so
 * the unit test pins the format and the React overlay doesn't have to
 * know about the on-disk shape.
 *
 * Format (matches BL-043 plan §11):
 *   ## Captured at {capturedAt}
 *
 *   Source: {sourceMeta.app}
 *
 *   {draft}
 *
 * The kernel-side dispatch already normalises the trailing-newline shape;
 * we still emit a trailing `\n` here for symmetry with how the engine
 * sees a hand-edited inbox.
 */
export function buildSnippet(draft: string, sourceMeta: CaptureSourceMeta): string {
  const lines: string[] = []
  lines.push(`## Captured at ${sourceMeta.capturedAt}`)
  lines.push('')
  lines.push(`Source: ${sourceMeta.app}`)
  lines.push('')
  if (sourceMeta.code) {
    // Code capture (BL-046): emit `File:` / `Lines:` headers, a
    // language-tagged fence, and a `#code/<language>` recall tag
    // in place of a bare draft. The fence body is the user's
    // draft so any narrative the user added in the textarea ends
    // up *outside* the fence is left to a future "annotation"
    // section — for now the textarea body is the code.
    for (const line of buildCodeSnippetSection(draft, sourceMeta.code)) {
      lines.push(line)
    }
    return lines.join('\n')
  }
  lines.push(draft)
  lines.push('')
  return lines.join('\n')
}

/**
 * Run the commit pipeline: read the inbox path from configuration, build
 * the snippet, fire the kernel-routed append, and update store state.
 *
 * Exported separately from the React overlay so the unit tests can drive
 * it through a stubbed `KernelAPI` without rendering anything.
 *
 * On success: closes the overlay and returns the kernel response.
 * On failure: stores the error message on the store and keeps the
 *   overlay open so the user can retry without retyping.
 */
export async function commitCapture(args: {
  api: KernelAPI
  inboxPath: string
  draft: string
  sourceMeta: CaptureSourceMeta
}): Promise<{ ok: true } | { ok: false; error: string }> {
  const { api, inboxPath, draft, sourceMeta } = args
  const snippet = buildSnippet(draft, sourceMeta)
  useCaptureStore.getState().setError(null)
  try {
    await appendInbox(api, inboxPath, snippet)
    useCaptureStore.getState().close()
    return { ok: true }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    useCaptureStore.getState().setError(message)
    return { ok: false, error: message }
  }
}

/**
 * Best-effort clipboard read. The Web Clipboard API can reject in three
 * ways we tolerate without showing an error to the user (BL-043 §10):
 *   1. `navigator.clipboard` undefined (older browsers / non-secure
 *      contexts).
 *   2. `readText()` rejects with `NotAllowedError` (permission denied).
 *   3. `readText()` rejects for any other reason (focused element type,
 *      …).
 *
 * Always resolves to a string. Returning the empty string is the
 * documented "no pre-fill" outcome.
 */
export async function readClipboardBestEffort(): Promise<string> {
  try {
    if (typeof navigator === 'undefined' || !navigator.clipboard?.readText) {
      return ''
    }
    const text = await navigator.clipboard.readText()
    return typeof text === 'string' ? text : ''
  } catch {
    return ''
  }
}
