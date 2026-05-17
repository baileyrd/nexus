// BL-141 follow-up — diagnostics panel store.
//
// Mirrors the latest `com.nexus.lsp.textDocument.publishDiagnostics`
// payload per file. The LSP spec says each notification carries the
// *complete current set* for that URI, so the store replaces the entry
// rather than appending. Empty arrays are kept (vs deleted) only when
// the panel needs to render a "cleared" indicator; today we drop them
// to keep the file list tight.

import { create } from 'zustand'

import type { LspDiagnostic } from '../editor/cm/lspIpc.ts'
import { severityTag } from '../editor/cm/lspToExcerpts.ts'

export interface SeverityBuckets {
  error: number
  warn: number
  info: number
  hint: number
}

export const EMPTY_BUCKETS: Readonly<SeverityBuckets> = Object.freeze({
  error: 0,
  warn: 0,
  info: 0,
  hint: 0,
})

/** Count diagnostics by severity tag. Pure — exported for tests. */
export function bucketCounts(diags: readonly LspDiagnostic[]): SeverityBuckets {
  const out: SeverityBuckets = { error: 0, warn: 0, info: 0, hint: 0 }
  for (const d of diags) {
    const tag = severityTag(d?.severity) as keyof SeverityBuckets
    out[tag] += 1
  }
  return out
}

/** Sum buckets across every file. Pure. */
export function totalBuckets(
  byUri: ReadonlyMap<string, LspDiagnostic[]>,
): SeverityBuckets {
  const out: SeverityBuckets = { error: 0, warn: 0, info: 0, hint: 0 }
  for (const diags of byUri.values()) {
    const b = bucketCounts(diags)
    out.error += b.error
    out.warn += b.warn
    out.info += b.info
    out.hint += b.hint
  }
  return out
}

/** Render the panel header summary. Returns empty string when there
 *  are no diagnostics so the view can suppress the count line. */
export function composeHeader(totals: SeverityBuckets): string {
  const parts: string[] = []
  if (totals.error > 0) parts.push(`${totals.error} ${totals.error === 1 ? 'error' : 'errors'}`)
  if (totals.warn > 0) parts.push(`${totals.warn} ${totals.warn === 1 ? 'warning' : 'warnings'}`)
  if (totals.info > 0) parts.push(`${totals.info} info`)
  if (totals.hint > 0) parts.push(`${totals.hint} ${totals.hint === 1 ? 'hint' : 'hints'}`)
  return parts.join(' · ')
}

interface DiagnosticsState {
  /** Source of truth — last-seen diagnostics per URI. */
  byUri: Map<string, LspDiagnostic[]>
  /** Set a URI's diagnostics. Empty arrays drop the key. */
  setForUri(uri: string, diagnostics: LspDiagnostic[]): void
  /** Drop every entry — used on workspace close. */
  clear(): void
}

export const useDiagnosticsStore = create<DiagnosticsState>((set) => ({
  byUri: new Map(),
  setForUri(uri, diagnostics) {
    set((s) => {
      const next = new Map(s.byUri)
      if (!Array.isArray(diagnostics) || diagnostics.length === 0) {
        next.delete(uri)
      } else {
        next.set(uri, diagnostics)
      }
      return { byUri: next }
    })
  },
  clear() {
    set({ byUri: new Map() })
  },
}))
