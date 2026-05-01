// CodeMirror 6 decoration source for inline `[[{db:…}]]` blocks
// (BL-012 split 3 — PRD-08 §8.1).
//
// Walks the doc text per line for the `[[{db:<spec>}]]` syntax and
// emits a `Decoration.replace` carrying a `DatabaseViewWidget` over
// the matched range, but only when the cursor is *not* on that
// line — same active-line-reveal rule the live-preview decoration
// builder uses for tables and fenced code (§ `livePreviewDecorations.ts`).
//
// The block-decoration constraint forces a `StateField` source
// rather than a `ViewPlugin` (`RangeError: Block decorations may
// not be specified via plugins`).
//
// Inline syntax (split 3 MVP — keeps the parser un-changed):
//
//   [[{db:Tasks.bases}]]                       (table view, no filters)
//   [[{db:Tasks.bases?view=kanban&group=status}]]
//   [[{db:Tasks.bases?filter=status%20=%20Done&sort=due_date%20asc}]]
//
// Filter / sort syntax matches the BL-012 split-1 parser (`field
// <op> value` / `field [asc|desc]`). Multiple `filter=` / `sort=`
// query params append to the lists in declaration order. Values are
// percent-decoded once.
//
// Markdown-parser-side recognition (so the editor's BlockType is
// `DatabaseView` instead of `Embed` on roundtrip) is split 4 territory.

import { type EditorState, type Extension, StateEffect, StateField } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
} from '@codemirror/view'

import {
  databaseViewCache,
  DatabaseViewCache,
  DatabaseViewWidget,
} from './databaseViewWidget'
import type {
  DatabaseViewConfig,
  DatabaseViewType,
  EditorKernelClient,
} from '../kernelClient'

/** Minimal subset of `KernelAPI` the watcher needs — kept narrow
 *  so unit tests can inject a fake without standing up the full
 *  kernel. Mirrors the shape of `KernelAPI.on` (Promise of an
 *  unsubscribe). */
export interface KernelEventSubscriber {
  on<T = unknown>(
    topicPrefix: string,
    handler: (topic: string, payload: T) => void,
  ): Promise<() => void>
}

/** Dependencies the decoration extension needs — passed from
 *  `EditorView.tsx` so unit tests can inject mocks. */
export interface DatabaseViewExtDeps {
  client: EditorKernelClient
  /** Optional cache override. Defaults to the shared singleton so
   *  results survive across tab re-mounts. */
  cache?: DatabaseViewCache
  /** Error sink threaded through to the widget. */
  onError?: (message: string, err: unknown) => void
  /** Optional kernel-event subscriber. When wired, the extension
   *  listens to `com.nexus.storage.file_modified` /
   *  `file_created` / `file_deleted` / `file_renamed` and calls
   *  `cache.invalidatePath(basePath)` followed by a
   *  `databaseViewInvalidate` dispatch when the changed path lives
   *  inside a `.bases/` directory. Without this, the cache only
   *  refreshes on doc edits. */
  events?: KernelEventSubscriber
}

/** A single parsed `[[{db:…}]]` occurrence in the document. Stays
 *  exported so split-4 / split-5 callers (decoration walker, undo
 *  bridge, command palette "insert db block") can reuse the parser. */
export interface ParsedDatabaseViewBlock {
  /** Document offset of the opening `[`. */
  from: number
  /** Document offset just past the closing `]]`. */
  to: number
  /** Forge-relative path to the `.bases` directory. */
  databasePath: string
  /** Resolved view config — never `null`; absent fields default to
   *  the table view with no filters / sorts / hidden columns. */
  config: DatabaseViewConfig
}

/** Errors surfaced inline when the syntax is malformed. Rendered
 *  alongside the source instead of replacing it so the user can
 *  fix the issue in place. */
export interface ParsedDatabaseViewError {
  from: number
  to: number
  message: string
}

const BLOCK_RE = /\[\[\{db:([^}]*)\}\]\]/g

/** Pure parser — scans `text` for `[[{db:<spec>}]]` occurrences,
 *  returning every match (even malformed ones, surfaced via
 *  `errors`). `offset` is added to each `from` / `to` so callers
 *  can scan a single line and translate back to doc coordinates.
 *
 *  Exported for unit tests and for split-4 wiring (the markdown
 *  parser will share this regex once the editor crate's
 *  `BlockType::DatabaseView` learns the syntax). */
export function parseDatabaseViewBlocks(
  text: string,
  offset = 0,
): { blocks: ParsedDatabaseViewBlock[]; errors: ParsedDatabaseViewError[] } {
  const blocks: ParsedDatabaseViewBlock[] = []
  const errors: ParsedDatabaseViewError[] = []
  // Reset the regex state — `BLOCK_RE` is module-scoped + global so
  // a previous scan would leave `lastIndex` non-zero.
  BLOCK_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = BLOCK_RE.exec(text)) !== null) {
    const from = offset + match.index
    const to = from + match[0].length
    const spec = match[1].trim()
    if (!spec) {
      errors.push({ from, to, message: 'empty `db:` spec' })
      continue
    }
    try {
      const parsed = parseSpec(spec)
      blocks.push({ from, to, ...parsed })
    } catch (err) {
      errors.push({
        from,
        to,
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }
  return { blocks, errors }
}

interface ParsedSpec {
  databasePath: string
  config: DatabaseViewConfig
}

function parseSpec(spec: string): ParsedSpec {
  const queryStart = spec.indexOf('?')
  const databasePath = (queryStart < 0 ? spec : spec.slice(0, queryStart)).trim()
  if (!databasePath) {
    throw new Error('missing database path')
  }
  if (databasePath.includes('..')) {
    // Reject path-traversal attempts up front; the storage layer
    // would also catch this but failing early surfaces a clear
    // inline error.
    throw new Error('invalid database path')
  }
  const config: DatabaseViewConfig = {
    view_type: { kind: 'table' },
    filters: [],
    sorts: [],
    group_by: null,
    hidden_columns: [],
  }
  if (queryStart < 0) return { databasePath, config }

  const params = new URLSearchParams(spec.slice(queryStart + 1))
  const view = params.get('view')?.toLowerCase()
  const group = params.get('group') ?? undefined
  const dateField = params.get('date') ?? params.get('date_field') ?? undefined
  const titleField = params.get('title') ?? params.get('title_field') ?? undefined

  config.view_type = resolveViewType(view, { group, dateField, titleField })
  // The structured layout-specific group (`column_by` / `date_field`)
  // wins; `group_by` is the generic fallback for layouts that don't
  // pin one (e.g. List view, once it learns this syntax).
  if (group && view !== 'kanban') config.group_by = group

  for (const f of params.getAll('filter')) {
    if (f.trim()) config.filters.push(f.trim())
  }
  for (const s of params.getAll('sort')) {
    if (s.trim()) config.sorts.push(s.trim())
  }
  for (const h of params.getAll('hide')) {
    if (h.trim()) config.hidden_columns.push(h.trim())
  }
  return { databasePath, config }
}

function resolveViewType(
  view: string | undefined,
  fields: { group?: string; dateField?: string; titleField?: string },
): DatabaseViewType {
  switch (view) {
    case undefined:
    case '':
    case 'table':
      return { kind: 'table' }
    case 'kanban':
      return { kind: 'kanban', column_by: fields.group ?? 'status' }
    case 'calendar':
      return { kind: 'calendar', date_field: fields.dateField ?? 'date' }
    case 'gallery':
      return { kind: 'gallery', title_field: fields.titleField ?? 'title' }
    default:
      throw new Error(`unknown view kind '${view}'`)
  }
}

/** Inverse of [`parseDatabaseViewBlocks`] — serialise a
 *  `(databasePath, config)` pair back to the inline
 *  `[[{db:<spec>}]]` form. Only emits query-string params when they
 *  carry information so the bare-table case round-trips to
 *  `[[{db:<path>}]]` instead of `[[{db:<path>?view=table}]]`.
 *  Filter / sort / hide values are percent-encoded so spaces and
 *  `=` / `&` survive a round-trip through the parser.
 *
 *  Used by the BL-012 split-5 filter/sort UX to write user
 *  edits back to the source — the markdown stays the truth and
 *  the next decoration rebuild parses the new spec the same way
 *  any other tool would. */
export function serializeDatabaseViewSpec(
  databasePath: string,
  config: DatabaseViewConfig,
): string {
  if (!databasePath) throw new Error('database path required')
  const params: string[] = []
  const view = config.view_type
  switch (view.kind) {
    case 'table':
      // Default — omit `view=` to keep the source minimal.
      break
    case 'kanban':
      params.push('view=kanban')
      params.push(`group=${encodeURIComponent(view.column_by)}`)
      break
    case 'calendar':
      params.push('view=calendar')
      params.push(`date=${encodeURIComponent(view.date_field)}`)
      break
    case 'gallery':
      params.push('view=gallery')
      params.push(`title=${encodeURIComponent(view.title_field)}`)
      break
    case 'custom':
      // Plugin-provided types fall through to the parser as
      // `unknown view kind` — preserve the raw value so the
      // round-trip surfaces an explicit error to the user instead
      // of silently dropping it.
      params.push(`view=${encodeURIComponent((view as { 0: string })[0] ?? 'custom')}`)
      break
  }
  if (config.group_by && view.kind !== 'kanban') {
    params.push(`group=${encodeURIComponent(config.group_by)}`)
  }
  for (const f of config.filters) {
    params.push(`filter=${encodeURIComponent(f)}`)
  }
  for (const s of config.sorts) {
    params.push(`sort=${encodeURIComponent(s)}`)
  }
  for (const h of config.hidden_columns) {
    params.push(`hide=${encodeURIComponent(h)}`)
  }
  return params.length === 0
    ? `[[{db:${databasePath}}]]`
    : `[[{db:${databasePath}?${params.join('&')}}]]`
}

// ── Decoration builder ──────────────────────────────────────────────────────

/** Pure decoration builder — emits one `Decoration.replace` per
 *  off-active-line `[[{db:…}]]` block, plus error marks for the
 *  malformed ones. Exported for unit testing.
 *
 *  Active-line reveal mirrors `livePreviewDecorations` so the user
 *  can position the cursor on the line and edit the spec; off-line,
 *  the source range is replaced by the rendered grid widget.
 *
 *  When `view` is provided the widgets receive an `onUpdateConfig`
 *  callback that rewrites the inline source via a CM transaction
 *  (split-5 filter / sort UX). The state-field path passes
 *  `view` from its `provide(field)` to keep the source the truth;
 *  the test harness omits it and exercises the read-only path. */
export function buildDatabaseViewDecorations(
  state: EditorState,
  deps: DatabaseViewExtDeps,
  view?: EditorView,
): DecorationSet {
  const builder: { from: number; to: number; deco: Decoration }[] = []
  const text = state.doc.toString()
  const { blocks, errors } = parseDatabaseViewBlocks(text)
  const activeLines = computeActiveLines(state)

  for (const block of blocks) {
    const line = state.doc.lineAt(block.from)
    if (activeLines.has(line.number)) continue
    const onUpdateConfig = view
      ? (next: DatabaseViewConfig) => {
          // Re-locate the block by its current source range —
          // `block.from` / `block.to` were captured at scan time and
          // a concurrent edit elsewhere could have shifted them. The
          // simplest correct strategy is to re-scan and replace the
          // first block at `databasePath` whose range still contains
          // the original `from`; if not found, no-op.
          const live = view.state
          const fresh = parseDatabaseViewBlocks(live.doc.toString())
          const target = fresh.blocks.find(
            (b) =>
              b.databasePath === block.databasePath &&
              b.from <= block.from &&
              b.to >= block.to - (block.to - block.from) / 2,
          )
          if (!target) return
          const insert = serializeDatabaseViewSpec(target.databasePath, next)
          view.dispatch({
            changes: { from: target.from, to: target.to, insert },
            // Bump the field via the explicit invalidate effect so the
            // rebuild path runs even if the change happens to be a
            // no-op rewrite (e.g. removing an already-empty filter
            // wouldn't mutate the doc but the cache should still flush).
            effects: databaseViewInvalidate.of(null),
          })
        }
      : undefined

    const widget = new DatabaseViewWidget(block.databasePath, block.config, {
      client: deps.client,
      cache: deps.cache ?? databaseViewCache,
      onError: deps.onError,
      onUpdateConfig,
    })
    builder.push({
      from: block.from,
      to: block.to,
      deco: Decoration.replace({ widget, block: true, inclusive: false }),
    })
  }
  for (const err of errors) {
    builder.push({
      from: err.from,
      to: err.to,
      deco: Decoration.mark({
        class: 'cm-md-dbview-syntax-error',
        attributes: { title: err.message },
      }),
    })
  }
  builder.sort((a, b) => a.from - b.from || a.to - b.to)
  const set = Decoration.set(
    builder.map((b) => b.deco.range(b.from, b.to)),
    true,
  )
  return set
}

function computeActiveLines(state: EditorState): Set<number> {
  const lines = new Set<number>()
  for (const range of state.selection.ranges) {
    const fromLine = state.doc.lineAt(range.from).number
    const toLine = state.doc.lineAt(range.to).number
    for (let i = fromLine; i <= toLine; i++) lines.add(i)
    lines.add(state.doc.lineAt(range.anchor).number)
    lines.add(state.doc.lineAt(range.head).number)
  }
  return lines
}

/** Effect that requests a decoration recompute. Fired from the
 *  storage-event watcher (split 4) after a `.bases` directory
 *  changes externally, so the inline grid flushes the stale cached
 *  layout without waiting for a doc edit. */
export const databaseViewInvalidate = StateEffect.define<null>()

/** Resolve a forge-relative path to the `.bases` directory it
 *  belongs to, or `null` if the path doesn't live under one.
 *  Two cases:
 *    * the path *is* the directory — `Tasks.bases` → `Tasks.bases`
 *    * the path is inside it — `Tasks.bases/records.json` →
 *      `Tasks.bases`, also `nested/Board.bases/views.json` →
 *      `nested/Board.bases`.
 *  Anything else returns `null`. Exported so unit tests can pin
 *  the regex; split-5 will likely reuse it. */
export function pathToBasePath(relpath: string): string | null {
  if (!relpath) return null
  if (relpath.endsWith('.bases')) return relpath
  // First `.bases/` segment wins — nested bases would be invalid in
  // the storage layer anyway.
  const idx = relpath.indexOf('.bases/')
  if (idx < 0) return null
  return relpath.slice(0, idx + '.bases'.length)
}

/** Storage-event watcher: subscribe to file mutation topics, find
 *  the affected base path (if any), invalidate cached views for
 *  that path, and dispatch `databaseViewInvalidate` on the editor
 *  view so the field rebuilds.
 *
 *  Exported for unit tests; production wiring goes through
 *  `databaseViewExt(deps)`. */
export function makeBasesChangeWatcher(
  view: EditorView,
  deps: DatabaseViewExtDeps,
): { destroy: () => void } {
  const cache = deps.cache ?? databaseViewCache
  if (!deps.events) return { destroy() {} }

  let disposed = false
  let unsubscribe: (() => void) | null = null
  const subscribed = deps.events.on<{ path?: string; from?: string; to?: string }>(
    'com.nexus.storage.file_',
    (_topic, payload) => {
      // The four topic ids — file_created / file_modified /
      // file_deleted / file_renamed — all carry a path field.
      // file_renamed carries `from` + `to`; both candidates are
      // mapped to base paths so we cover the rename-into and
      // rename-out cases.
      const candidates = [payload.path, payload.from, payload.to].filter(
        (p): p is string => typeof p === 'string' && p.length > 0,
      )
      let touched = 0
      for (const p of candidates) {
        const basePath = pathToBasePath(p)
        if (!basePath) continue
        touched += cache.invalidatePath(basePath)
      }
      if (touched > 0) {
        view.dispatch({ effects: databaseViewInvalidate.of(null) })
      }
    },
  )

  void subscribed.then(
    (unsub) => {
      if (disposed) {
        unsub()
        return
      }
      unsubscribe = unsub
    },
    (err) => {
      const onError =
        deps.onError ??
        ((m, e) => {
          console.error(`[nexus.editor] ${m}:`, e)
        })
      onError('database-view watcher: subscribe failed', err)
    },
  )

  return {
    destroy() {
      disposed = true
      if (unsubscribe) {
        unsubscribe()
        unsubscribe = null
      }
    },
  }
}

/** CM extension: state field carrying the decoration set + a
 *  matching atomic-ranges provider so the cursor doesn't park inside
 *  a hidden block. The view-bound ViewPlugin owns both the storage-
 *  event subscription (split 4) and a mutable view reference the
 *  state field reads through to wire write-back transactions on the
 *  widget's filter / sort UX (split 5). */
export function databaseViewExt(deps: DatabaseViewExtDeps): Extension {
  // Mutable closure holding the active EditorView so the field's
  // `update(tr)` (which only receives the transaction, not the
  // view) can pass it through to `buildDatabaseViewDecorations`.
  // Set on ViewPlugin create, cleared on destroy.
  const viewRef: { current: EditorView | null } = { current: null }

  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildDatabaseViewDecorations(state, deps, viewRef.current ?? undefined)
    },
    update(value, tr) {
      if (
        tr.docChanged ||
        tr.selection ||
        tr.effects.some((e) => e.is(databaseViewInvalidate))
      ) {
        return buildDatabaseViewDecorations(
          tr.state,
          deps,
          viewRef.current ?? undefined,
        )
      }
      return value
    },
    provide(f) {
      return [
        EditorView.decorations.from(f),
        EditorView.atomicRanges.of((view) => view.state.field(f) ?? Decoration.none),
      ]
    },
  })

  // ViewPlugin owns (a) the storage-event subscription and (b)
  // the mutable view reference the state field reads through. When
  // `deps.events` is absent the watcher half is a no-op; the view
  // ref is still captured so write-back transactions work even on
  // event-less mounts.
  const watcher = ViewPlugin.define((view) => {
    viewRef.current = view
    const handle = makeBasesChangeWatcher(view, deps)
    // Trigger a rebuild so the freshly-installed view reference flows
    // into the widgets' `onUpdateConfig` closures. Deferred via
    // queueMicrotask so we don't dispatch during CM's construction
    // update (dispatching while CM's updateState != 0 throws).
    queueMicrotask(() => {
      if (viewRef.current === view) {
        view.dispatch({ effects: databaseViewInvalidate.of(null) })
      }
    })
    return {
      destroy() {
        if (viewRef.current === view) viewRef.current = null
        handle.destroy()
      },
    }
  })

  return [field, watcher]
}
