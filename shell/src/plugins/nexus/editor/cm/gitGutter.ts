// shell/src/plugins/nexus/editor/cm/gitGutter.ts
//
// BL-079 — git gutter for the active editor buffer.
//
// Calls `com.nexus.git::diff_file` for the currently-open file and
// renders per-line markers in CodeMirror's left gutter:
//   - **Added** (green vertical bar) — line exists in the working
//     copy but didn't in HEAD, with no nearby `Removed` lines.
//   - **Modified** (yellow vertical bar) — line replaces one or more
//     `Removed` lines in the same hunk (a `+` directly paired with a
//     `-`).
//   - **Deletion** (red triangle) — there's at least one `Removed`
//     line below the current line in the new file. Convention
//     borrowed from VS Code: deletions don't have a working-copy
//     line of their own, so we anchor the marker to the line
//     immediately above the gap.
//
// Hovering a marker shows a tooltip with the original lines (for
// modifications and deletions). Clicking opens an inline diff hunk
// with Stage / Revert buttons that route through
// `com.nexus.git::stage_hunks` / `unstage_hunks`.
//
// The fetch is debounced and re-issued on:
//   - file open (mount of a fresh editor),
//   - `files:saved` events for the buffer's relpath,
//   - the `nexus.git.refreshGutter` command.
//
// Lives in the CM extension stack; the extension self-installs the
// gutter slot, the state field, the view plugin (for the async
// fetch), and the click handler.

import {
  Compartment,
  StateEffect,
  StateField,
  type Extension,
} from '@codemirror/state'
import {
  EditorView,
  GutterMarker,
  ViewPlugin,
  type ViewUpdate,
  gutter,
  hoverTooltip,
} from '@codemirror/view'

const PLUGIN_ID = 'com.nexus.git'
const CMD_DIFF_FILE = 'diff_file'
const CMD_STAGE_HUNKS = 'stage_hunks'

/// Mirror of `crates/nexus-git/src/ipc.rs::GitDiffLine`.
interface GitDiffLine {
  kind: 'Context' | 'Added' | 'Removed'
  content: string
}

/// Mirror of `crates/nexus-git/src/ipc.rs::GitDiffHunk`.
interface GitDiffHunk {
  old_start: number
  old_count: number
  new_start: number
  new_count: number
  lines: GitDiffLine[]
}

/// What a single line in the *new* file looks like, after we walk
/// the hunks. Lines not in this map have no marker.
type LineKind = 'added' | 'modified' | 'deletion-above'

/// Annotation a line carries — kind plus, for modifications /
/// deletions, the original `Removed` content shown in tooltip,
/// plus the 0-based index of the hunk this line belongs to so the
/// click-to-stage path knows which hunk to send to
/// `com.nexus.git::stage_hunks`.
interface LineMarker {
  kind: LineKind
  removed: string[]
  hunkIndex: number
}

interface GutterState {
  /// 1-based line number → marker.
  byLine: Map<number, LineMarker>
}

const setGutterState = StateEffect.define<GutterState>()

/// State field storing the per-line marker map. Updated by the
/// view plugin's `dispatch(setGutterState(...))` after each fetch.
const gutterStateField = StateField.define<GutterState>({
  create: () => ({ byLine: new Map() }),
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setGutterState)) return e.value
    }
    return value
  },
})

class AddedMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'nexus-git-gutter-marker nexus-git-gutter-added'
    el.title = 'Added'
    return el
  }
}
class ModifiedMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'nexus-git-gutter-marker nexus-git-gutter-modified'
    el.title = 'Modified'
    return el
  }
}
class DeletionMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'nexus-git-gutter-marker nexus-git-gutter-deletion'
    el.title = 'Deletion below'
    return el
  }
}

const ADDED = new AddedMarker()
const MODIFIED = new ModifiedMarker()
const DELETION = new DeletionMarker()

/**
 * Walk a list of `GitDiffHunk[]` and produce a per-new-line marker
 * map. The walk tracks a `pendingRemoved` buffer so we can tell the
 * three cases apart:
 *
 *   - Added immediately after a Removed → modified (replacement).
 *   - Added with no pending Removed → pure addition.
 *   - Context with pending Removed in buffer → flush as
 *     "deletion-above" anchored to the *previous* new line.
 *   - Trailing Removed at hunk end → "deletion-above" on the last
 *     observed new line.
 *
 * Pure factor — exported for unit tests; no DOM, no IPC.
 */
export function buildLineMarkers(hunks: GitDiffHunk[]): Map<number, LineMarker> {
  const byLine = new Map<number, LineMarker>()
  for (let hunkIndex = 0; hunkIndex < hunks.length; hunkIndex++) {
    const hunk = hunks[hunkIndex]!
    let newLine = hunk.new_start
    let pendingRemoved: string[] = []
    let lastNewLine = newLine - 1
    for (const line of hunk.lines) {
      if (line.kind === 'Context') {
        if (pendingRemoved.length > 0) {
          // The Removed lines have no Added counterpart — mark the
          // line *just above* the gap (the most recently observed
          // new line) as "deletion below it".
          const anchor = Math.max(lastNewLine, hunk.new_start)
          byLine.set(anchor, {
            kind: 'deletion-above',
            removed: pendingRemoved,
            hunkIndex,
          })
          pendingRemoved = []
        }
        lastNewLine = newLine
        newLine += 1
      } else if (line.kind === 'Added') {
        if (pendingRemoved.length > 0) {
          byLine.set(newLine, {
            kind: 'modified',
            removed: pendingRemoved,
            hunkIndex,
          })
          pendingRemoved = []
        } else {
          byLine.set(newLine, { kind: 'added', removed: [], hunkIndex })
        }
        lastNewLine = newLine
        newLine += 1
      } else {
        // Removed — no advance in the new file; buffer the content
        // for later attribution to a modification or deletion.
        pendingRemoved.push(line.content)
      }
    }
    // Hunk ended with unpaired Removed lines: attribute to the
    // last new-file line we saw.
    if (pendingRemoved.length > 0) {
      const anchor = Math.max(lastNewLine, hunk.new_start)
      byLine.set(anchor, {
        kind: 'deletion-above',
        removed: pendingRemoved,
        hunkIndex,
      })
    }
  }
  return byLine
}

interface GitGutterDeps {
  /** Forge-relative path of the buffer the editor is showing. */
  relpath: string
  /** Kernel handle. The extension calls
   *  `kernel.invoke('com.nexus.git', 'diff_file', { path })`. */
  kernel: {
    invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T>
    available?(): Promise<boolean>
  }
  /** Event subscriber — the extension hooks `files:saved` so the
   *  gutter refreshes whenever the buffer is written. */
  events?: {
    on<T = unknown>(name: string, fn: (payload: T) => void): () => void
  }
  /** Optional callback for diff failures. The extension swallows
   *  errors silently by default; callers wire to a notification UI
   *  for visibility. */
  onError?: (err: unknown) => void
}

/**
 * BL-079 — root extension that wires the per-line gutter markers
 * for `deps.relpath` to the live `com.nexus.git::diff_file`
 * response.
 *
 * Returns a CM6 extension; mount once per editor view. Multiple
 * views with different relpaths share the same extension but each
 * runs its own fetch — the state field is per-view.
 */
export function gitGutterExt(deps: GitGutterDeps): Extension {
  const fetchAndDispatch = async (view: EditorView) => {
    try {
      if (deps.kernel.available && !(await deps.kernel.available())) return
      const hunks = await deps.kernel.invoke<GitDiffHunk[]>(
        PLUGIN_ID,
        CMD_DIFF_FILE,
        { path: deps.relpath },
      )
      const byLine = buildLineMarkers(hunks ?? [])
      view.dispatch({ effects: setGutterState.of({ byLine }) })
    } catch (err) {
      deps.onError?.(err)
      // Reset to empty so a stale gutter doesn't linger after an
      // error.
      view.dispatch({
        effects: setGutterState.of({ byLine: new Map() }),
      })
    }
  }

  const watcher = ViewPlugin.fromClass(
    class {
      private readonly view: EditorView
      private unsub: (() => void) | null = null
      private pending: number | null = null

      constructor(view: EditorView) {
        this.view = view
        // Initial fetch — debounce it slightly so a view that
        // remounts rapidly (settings flip → key change) only
        // triggers one IPC.
        this.schedule()
        if (deps.events) {
          this.unsub = deps.events.on<{ relpath: string }>(
            'files:saved',
            (payload) => {
              if (payload.relpath === deps.relpath) {
                this.schedule()
              }
            },
          )
        }
      }

      schedule() {
        if (this.pending !== null) return
        this.pending = window.setTimeout(() => {
          this.pending = null
          void fetchAndDispatch(this.view)
        }, 50)
      }

      update(_u: ViewUpdate) {
        // We don't refetch on every transaction — that would mean
        // an IPC per keystroke. The save event covers the
        // common case; the user can manually refresh through the
        // command palette for ad-hoc cases.
      }

      destroy() {
        if (this.pending !== null) {
          clearTimeout(this.pending)
          this.pending = null
        }
        this.unsub?.()
        this.unsub = null
      }
    },
  )

  // Hover tooltip surfaces the original lines for modifications /
  // deletions. Pure additions don't need a tooltip — the line is
  // visible in place.
  const tooltip = hoverTooltip((view, pos) => {
    const line = view.state.doc.lineAt(pos)
    const state = view.state.field(gutterStateField, false)
    if (!state) return null
    const marker = state.byLine.get(line.number)
    if (!marker || marker.removed.length === 0) return null
    return {
      pos: line.from,
      end: line.to,
      above: true,
      create: () => {
        const dom = document.createElement('div')
        dom.className = 'nexus-git-gutter-tooltip'
        const heading = document.createElement('div')
        heading.className = 'nexus-git-gutter-tooltip-heading'
        heading.textContent =
          marker.kind === 'modified'
            ? 'Modified — original'
            : 'Deleted lines'
        dom.appendChild(heading)
        const body = document.createElement('pre')
        body.className = 'nexus-git-gutter-tooltip-body'
        body.textContent = marker.removed.join('\n')
        dom.appendChild(body)
        return { dom }
      },
    }
  })

  // BL-079 follow-up — click a gutter marker to stage the hunk it
  // belongs to. Routes through `com.nexus.git::stage_hunks` with the
  // hunk index threaded onto the line marker by `buildLineMarkers`.
  // After a successful stage, schedule a re-fetch so the gutter
  // reflects the post-stage diff (the staged hunk drops out of
  // working-tree → HEAD-via-index). On failure the gutter stays put
  // and the error routes through the same `onError` path the diff
  // fetch uses.
  //
  // Revert / discard-hunks intentionally not wired here — `nexus-git`
  // doesn't expose a "discard hunks" verb yet, and silently falling
  // back to `unstage_hunks` (which doesn't restore working-tree
  // bytes) would be misleading. Tracked as a deferred follow-up.
  const stageHunkAt = async (view: EditorView, lineNumber: number): Promise<boolean> => {
    const state = view.state.field(gutterStateField, false)
    if (!state) return false
    const marker = state.byLine.get(lineNumber)
    if (!marker) return false
    try {
      await deps.kernel.invoke(PLUGIN_ID, CMD_STAGE_HUNKS, {
        path: deps.relpath,
        hunk_indices: [marker.hunkIndex],
      })
    } catch (err) {
      deps.onError?.(err)
      return false
    }
    void fetchAndDispatch(view)
    return true
  }

  return [
    gutterStateField,
    watcher,
    gutter({
      class: 'nexus-git-gutter',
      lineMarker(view, blockInfo) {
        const state = view.state.field(gutterStateField, false)
        if (!state) return null
        const lineObj = view.state.doc.lineAt(blockInfo.from)
        const marker = state.byLine.get(lineObj.number)
        if (!marker) return null
        switch (marker.kind) {
          case 'added':
            return ADDED
          case 'modified':
            return MODIFIED
          case 'deletion-above':
            return DELETION
        }
      },
      initialSpacer: () => ADDED,
      domEventHandlers: {
        click(view, blockInfo) {
          const lineObj = view.state.doc.lineAt(blockInfo.from)
          const state = view.state.field(gutterStateField, false)
          if (!state) return false
          if (!state.byLine.has(lineObj.number)) return false
          // Fire-and-forget; resolved value tracked for tests via
          // the deps surface.
          void stageHunkAt(view, lineObj.number)
          return true
        },
      },
    }),
    tooltip,
  ]
}

/** BL-079 follow-up — exported for unit tests so the suite can drive
 *  the click path without standing up a full CM6 view + DOM event.
 *  Thin wrapper over the same IPC + refresh sequence the gutter's
 *  click handler runs. Returns `true` on success (IPC + refresh
 *  scheduled), `false` when the line has no marker or the IPC
 *  rejected. */
export async function stageHunkForLine(
  deps: GitGutterDeps,
  marker: { hunkIndex: number } | undefined,
  refresh: () => void,
): Promise<boolean> {
  if (!marker) return false
  try {
    await deps.kernel.invoke(PLUGIN_ID, CMD_STAGE_HUNKS, {
      path: deps.relpath,
      hunk_indices: [marker.hunkIndex],
    })
  } catch (err) {
    deps.onError?.(err)
    return false
  }
  refresh()
  return true
}

/// BL-079 — public re-export so the editor view can install its
/// own keymap effects against the underlying state field (e.g. a
/// "next change" / "previous change" hop). Kept as an export
/// rather than a private impl so consumers don't take a deep
/// import path on this file.
export { setGutterState, gutterStateField }

/// BL-079 — convenience for callers who want to swap the gutter
/// extension on/off via a Compartment. Not currently used by the
/// editor host (the gutter is unconditional in code mode), but
/// kept here so tests and future toggles can share one shape.
export function gitGutterCompartment(): Compartment {
  return new Compartment()
}
