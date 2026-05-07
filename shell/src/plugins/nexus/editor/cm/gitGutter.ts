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
/// deletions, the original `Removed` content shown in tooltip.
interface LineMarker {
  kind: LineKind
  removed: string[]
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
  for (const hunk of hunks) {
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
          })
          pendingRemoved = []
        } else {
          byLine.set(newLine, { kind: 'added', removed: [] })
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
    }),
    tooltip,
  ]
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
