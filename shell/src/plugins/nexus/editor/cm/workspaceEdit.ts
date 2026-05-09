// BL-077 follow-up — WorkspaceEdit applier.
//
// `LspIpc.rename` and `LspIpc.codeActions` return an LSP
// `WorkspaceEdit` (multi-file). The Mod-s format path uses
// `applyTextEdits` for single-file edits, but rename / code-actions
// span every file the symbol appears in — the applier here walks the
// shape and routes each per-file slice through either the live CM6
// view (active tab — preserves cursor + undo history) or
// `com.nexus.storage::write_file` (every other URI).
//
// We only consume the `changes` field today (the `{ uri: TextEdit[] }`
// form). `documentChanges` adds optional version checking + create /
// rename / delete file ops; deferred until a server actually relies
// on them — rust-analyzer and typescript-language-server both fall
// back to `changes` when the client doesn't advertise
// `documentChanges` support, which we don't (the LSP host stays
// version-agnostic on the wire).
//
// URI shape on the wire: LSP servers reply with absolute `file://`
// URIs. Our IPC layer accepts either that form or a bare relpath; on
// the way back we normalise to a forge-relative path before going to
// storage.

import type { EditorView } from '@codemirror/view'

import type { LspTextEdit } from './lspClient'
import { applyTextEdits, lspPositionToOffset } from './lspClient'
import type { LspRange } from './lspIpc'

/** Subset of `WorkspaceEdit` we consume — see module doc-comment. */
export interface LspWorkspaceEdit {
  changes?: Record<string, LspTextEdit[]>
  // documentChanges intentionally omitted — see module doc.
}

/**
 * One file's worth of edits already mapped to a forge-relative path.
 * The applier batches per-file work so a multi-file rename writes
 * each file once even if the LSP server returned interleaved edits.
 */
export interface PerFileEdit {
  relpath: string
  edits: LspTextEdit[]
}

export interface ApplyWorkspaceEditOptions {
  /** Forge root absolute path. URIs whose absolute path lives under
   *  this prefix get rewritten to relpaths; URIs that don't are
   *  surfaced through `onSkip`. */
  forgeRoot: string
  /** Active CM view, if any. Edits whose relpath matches
   *  `activeRelpath` go through this view to preserve cursor + undo. */
  activeView?: EditorView | null
  /** Forge-relative path of the active editor. */
  activeRelpath?: string | null
  /** Read a file via `com.nexus.storage::read_file`. Returns the
   *  current UTF-8 contents. */
  readFile: (relpath: string) => Promise<string>
  /** Write a file via `com.nexus.storage::write_file`. */
  writeFile: (relpath: string, content: string) => Promise<void>
  /** Notification sink for files outside the forge root that we
   *  refuse to touch. Defaults to a no-op. */
  onSkip?: (uri: string, reason: string) => void
}

/** Result of applying a `WorkspaceEdit`. */
export interface ApplyWorkspaceEditResult {
  /** Number of files mutated through the live CM view. 0 or 1. */
  liveViewFiles: number
  /** Number of files mutated through storage IPC. */
  storageFiles: number
  /** Files skipped because the URI didn't normalise to a forge-relative path. */
  skipped: string[]
}

/**
 * Convert an LSP URI (`file:///abs/path/x.rs`) or a bare absolute /
 * relative path to a forge-relative path. Returns `null` when the
 * URI's filesystem path doesn't live under `forgeRoot` — caller logs
 * and skips.
 *
 * Exported for tests; the host normalises consistently and we expect
 * exactly one of the two input shapes.
 */
export function uriToRelpath(uri: string, forgeRoot: string): string | null {
  let path = uri
  if (path.startsWith('file://')) {
    // Strip the scheme. On Linux/macOS this leaves an absolute path;
    // on Windows the host emits `file:///C:/...` which after stripping
    // gives `/C:/...` — drop the leading slash before drive letters.
    path = path.slice('file://'.length)
    if (/^\/[A-Za-z]:/.test(path)) path = path.slice(1)
  }
  // Decode percent-escapes (LSP servers escape spaces / unicode).
  try {
    path = decodeURI(path)
  } catch {
    // Malformed URI — fall through to absolute-path detection so a
    // best-effort match still lands. The caller skips on miss.
  }
  // If it's already relative (no leading separator), assume it's
  // already forge-relative — the host's IPC layer accepts that form.
  if (!path.startsWith('/') && !/^[A-Za-z]:[\\/]/.test(path)) {
    return path
  }
  // Absolute path — strip the forge-root prefix.
  const root = forgeRoot.endsWith('/') ? forgeRoot.slice(0, -1) : forgeRoot
  if (path === root) return ''
  if (path.startsWith(root + '/')) {
    return path.slice(root.length + 1)
  }
  // On Windows the absolute path may use backslashes — try the
  // forward-slash form as a fallback.
  const normalisedPath = path.replace(/\\/g, '/')
  const normalisedRoot = root.replace(/\\/g, '/')
  if (normalisedPath.startsWith(normalisedRoot + '/')) {
    return normalisedPath.slice(normalisedRoot.length + 1)
  }
  return null
}

/**
 * Group a `WorkspaceEdit`'s `changes` map into a flat list of
 * per-file slices keyed by forge-relative path. Order is
 * URI-iteration order from the input; LSP doesn't define an order
 * across files, so neither do we.
 *
 * Skipped URIs are reported through `onSkip`; valid edits are
 * returned in the array.
 */
export function groupEditsByRelpath(
  edit: LspWorkspaceEdit,
  forgeRoot: string,
  onSkip?: (uri: string, reason: string) => void,
): PerFileEdit[] {
  const out: PerFileEdit[] = []
  if (!edit.changes) return out
  for (const [uri, edits] of Object.entries(edit.changes)) {
    const relpath = uriToRelpath(uri, forgeRoot)
    if (relpath == null) {
      onSkip?.(uri, 'outside forge root')
      continue
    }
    if (relpath === '') {
      onSkip?.(uri, 'resolved to forge root itself')
      continue
    }
    if (edits.length === 0) continue
    out.push({ relpath, edits })
  }
  return out
}

/**
 * Apply LSP `TextEdit[]` to a string buffer (the storage path). We
 * intentionally don't reuse `applyTextEdits` here because that
 * mutates a CM `EditorView`; for off-screen files we work on raw
 * strings instead, computing offsets the same way (line + character
 * → absolute).
 *
 * Edits are sorted bottom-up like the CM6 path so earlier edits
 * don't invalidate later positions.
 */
export function applyTextEditsToString(content: string, edits: LspTextEdit[]): string {
  if (edits.length === 0) return content
  const lineStarts = computeLineStarts(content)
  const sorted = edits
    .map((e) => ({
      from: positionToOffset(lineStarts, content.length, e.range.start),
      to: positionToOffset(lineStarts, content.length, e.range.end),
      insert: e.newText,
    }))
    .sort((a, b) => b.from - a.from)
  let out = content
  for (const e of sorted) {
    out = out.slice(0, e.from) + e.insert + out.slice(e.to)
  }
  return out
}

function computeLineStarts(content: string): number[] {
  const out = [0]
  for (let i = 0; i < content.length; i += 1) {
    if (content[i] === '\n') out.push(i + 1)
  }
  return out
}

function positionToOffset(
  lineStarts: number[],
  total: number,
  pos: { line: number; character: number },
): number {
  if (pos.line < 0) return 0
  if (pos.line >= lineStarts.length) return total
  const lineStart = lineStarts[pos.line]
  const lineEnd = pos.line + 1 < lineStarts.length ? lineStarts[pos.line + 1] - 1 : total
  const lineLength = Math.max(0, lineEnd - lineStart)
  const character = Math.min(Math.max(0, pos.character), lineLength)
  return lineStart + character
}

/**
 * BL-077 follow-up — apply a `WorkspaceEdit` across every file it
 * touches. Returns a summary the caller can render as a toast.
 *
 * Strategy:
 * 1. Group edits by URI → per-file slices.
 * 2. For the slice whose relpath matches the active editor, apply
 *    via the live CM6 view so cursor + undo survive. The save path
 *    will write it on the next `nexus.editor.save`; rename does not
 *    auto-save (matches VS Code).
 * 3. For every other slice, read-modify-write through storage IPC.
 *
 * The applier is intentionally not transactional — a partial failure
 * in the middle of a multi-file rename leaves earlier files mutated.
 * Practical alternatives (rolled-back via undo, two-phase commit) are
 * out of scope; the caller logs and surfaces a notification, and the
 * user resolves manually if needed.
 */
export async function applyWorkspaceEdit(
  edit: LspWorkspaceEdit,
  opts: ApplyWorkspaceEditOptions,
): Promise<ApplyWorkspaceEditResult> {
  const skipped: string[] = []
  const slices = groupEditsByRelpath(edit, opts.forgeRoot, (uri, reason) => {
    skipped.push(uri)
    opts.onSkip?.(uri, reason)
  })
  let liveViewFiles = 0
  let storageFiles = 0
  for (const slice of slices) {
    if (
      opts.activeView != null &&
      opts.activeRelpath != null &&
      slice.relpath === opts.activeRelpath
    ) {
      applyTextEdits(opts.activeView, slice.edits)
      liveViewFiles += 1
      continue
    }
    const before = await opts.readFile(slice.relpath)
    const after = applyTextEditsToString(before, slice.edits)
    if (after !== before) {
      await opts.writeFile(slice.relpath, after)
      storageFiles += 1
    }
  }
  return { liveViewFiles, storageFiles, skipped }
}

// Re-export so callers can build edits from CM positions without
// importing both modules.
export { lspPositionToOffset }
export type { LspRange }
