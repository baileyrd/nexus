// BL-141 Phase 3 — pure converters that turn LSP `Location[]`
// responses (`textDocument/references`) and `WorkspaceEdit` responses
// (`textDocument/rename`) into `ExcerptRequest[]` payloads suitable
// for `editor.openExcerpts(...)`.
//
// Both converters:
//
// - Resolve `file://` URIs against `forgeRoot` so the resulting
//   `relpath` matches what the storage IPC expects. URIs outside
//   the forge root are skipped silently — the caller logs / surfaces
//   them through a different channel if needed.
// - Expand each match range by `contextLines` lines in both
//   directions so the user sees the surrounding context, not just
//   the bare match line. The expansion is symmetric and clamps at
//   line 1 (no underflow); the caller of `openExcerpts` already
//   merges overlapping ranges within the same file.
// - Build per-match labels carrying the original match-line range
//   (`L42` / `L42-L48`) so the multibuffer's per-excerpt header
//   tells the user where each match came from.

import type { LspRange } from './lspIpc.ts'

/** LSP `Location` shape — minimal subset we use. */
export interface LspLocation {
  uri: string
  range: LspRange
}

/** LSP `WorkspaceEdit` shape — minimal subset we use (the same
 *  one `applyWorkspaceEdit` already handles). */
export interface LspWorkspaceEditChanges {
  changes?: Record<string, Array<{ range: LspRange; newText: string }>>
}

/** Per-item input shape for `editor.openExcerpts`. Mirrors
 *  `ExcerptRequest` in `kernelClient.ts`. */
export interface ExcerptRequest {
  relpath: string
  line_start: number
  line_end: number
  label?: string
}

/**
 * Pure factor — convert a `file://` URI to a forge-relative path.
 * Returns `null` when the URI lives outside `forgeRoot` (the
 * caller surfaces those through `onSkip` or equivalent).
 *
 * Mirrors the URI-handling shape in `cm/workspaceEdit.ts`'s
 * `uriToRelpath` (intentionally re-implemented rather than imported
 * to keep this module dependency-light and trivially testable —
 * the algorithm is small).
 */
export function uriToRelpath(uri: string, forgeRoot: string): string | null {
  if (!uri.startsWith('file://')) return null
  const absPath = uri.slice('file://'.length)
  // Normalize forge root: no trailing slash so the prefix match
  // doesn't accidentally let `/srv/forge2` look like a child of
  // `/srv/forge`.
  const normalizedRoot = forgeRoot.replace(/\/+$/, '')
  if (!absPath.startsWith(`${normalizedRoot}/`)) {
    // Also accept the exact match (forge-root file itself, e.g.
    // a hypothetical README at root level).
    if (absPath === normalizedRoot) return ''
    return null
  }
  return absPath.slice(normalizedRoot.length + 1)
}

/** Options shared by both converters. */
export interface ToExcerptsOptions {
  /** Absolute path to the forge root. URIs whose absolute path
   *  lives under this prefix get rewritten to relpaths; others are
   *  skipped silently. */
  forgeRoot: string
  /** Extra lines to include on either side of each match. Default
   *  3 — same as `grep --before-context=3 --after-context=3`. */
  contextLines?: number
}

const DEFAULT_CONTEXT_LINES = 3

/** Clamp + expand a single range to a `(line_start, line_end)`
 *  pair with surrounding context. 1-based, inclusive — matches
 *  `ExcerptRequest`'s contract.
 *
 *  LSP positions are 0-based, so `line + 1` converts. The expansion
 *  clamps `line_start` at 1; clamping the upper bound at EOF is
 *  the kernel's job (it sees the source bytes; we don't). */
export function rangeToExcerptLines(
  range: LspRange,
  contextLines: number,
): { line_start: number; line_end: number } {
  const startLine1 = range.start.line + 1
  const endLine1 = range.end.line + 1
  return {
    line_start: Math.max(1, startLine1 - contextLines),
    line_end: endLine1 + contextLines,
  }
}

/** Convenience helper that builds the per-excerpt label. Used by
 *  both converters. Renders as `L42` for a single-line match,
 *  `L42-L45` for a multi-line one. */
function rangeLabel(range: LspRange, suffix?: string): string {
  const a = range.start.line + 1
  const b = range.end.line + 1
  const head = a === b ? `L${a}` : `L${a}-L${b}`
  return suffix ? `${head} — ${suffix}` : head
}

/**
 * Convert `Location[]` (the response from `textDocument/references`)
 * into `ExcerptRequest[]`. Preserves response order; URIs outside
 * the forge root are silently dropped. Returns an empty array when
 * every location is filtered out — the caller surfaces "no
 * references in forge" through a notification.
 */
export function locationsToExcerptRequests(
  locations: LspLocation[],
  opts: ToExcerptsOptions,
): ExcerptRequest[] {
  const contextLines = opts.contextLines ?? DEFAULT_CONTEXT_LINES
  const out: ExcerptRequest[] = []
  for (const loc of locations) {
    const relpath = uriToRelpath(loc.uri, opts.forgeRoot)
    if (relpath === null || relpath === '') continue
    const lines = rangeToExcerptLines(loc.range, contextLines)
    out.push({
      relpath,
      line_start: lines.line_start,
      line_end: lines.line_end,
      label: rangeLabel(loc.range),
    })
  }
  return out
}

/**
 * Convert a `WorkspaceEdit` (the response from
 * `textDocument/rename`) into `ExcerptRequest[]`. Each per-file
 * `TextEdit[]` group expands into one excerpt per text-edit range
 * — the multibuffer's open-excerpt merger then collapses
 * overlapping ranges within a file.
 *
 * `documentChanges` is intentionally unhandled — the existing
 * `cm/workspaceEdit.ts` consumer also restricts to the `changes`
 * map, so the preview surface stays consistent with what the
 * apply path supports.
 */
export function workspaceEditToExcerptRequests(
  edit: LspWorkspaceEditChanges,
  opts: ToExcerptsOptions,
): ExcerptRequest[] {
  if (!edit?.changes) return []
  const contextLines = opts.contextLines ?? DEFAULT_CONTEXT_LINES
  const out: ExcerptRequest[] = []
  for (const [uri, textEdits] of Object.entries(edit.changes)) {
    const relpath = uriToRelpath(uri, opts.forgeRoot)
    if (relpath === null || relpath === '') continue
    for (const te of textEdits) {
      const lines = rangeToExcerptLines(te.range, contextLines)
      out.push({
        relpath,
        line_start: lines.line_start,
        line_end: lines.line_end,
        label: rangeLabel(te.range, `→ "${te.newText}"`),
      })
    }
  }
  return out
}
