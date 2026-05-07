// shell/src/plugins/nexus/editor/cm/gitBlame.ts
//
// BL-079 — togglable inline-blame annotations.
//
// When activated, fetches `com.nexus.git::blame` for the current
// file and renders a muted end-of-line widget on each line showing
// `<author> · <short hash> · <relative date> · <commit summary>`.
// The annotation is purely decorative (no text in the buffer); the
// CM `Decoration.widget` API keeps the document content untouched.
//
// The extension is opt-in — wired from the editor view through a
// Compartment so a command can flip it on / off without remounting
// the whole CM stack. Untitled tabs are excluded because they have
// no committed history.

import {
  StateEffect,
  StateField,
  type Extension,
} from '@codemirror/state'
import {
  Decoration,
  DecorationSet,
  EditorView,
  WidgetType,
  ViewPlugin,
  type ViewUpdate,
} from '@codemirror/view'

const PLUGIN_ID = 'com.nexus.git'
const CMD_BLAME = 'blame'

/// Mirror of `crates/nexus-git/src/ipc.rs::GitBlameEntry`.
interface GitBlameEntry {
  commit_hash: string
  author: string
  date: string
  message: string
  start_line: number
  end_line: number
}

interface BlameLineRow {
  /// Pre-rendered annotation string. Computed once per blame
  /// fetch so the widget's `toDOM` doesn't reformat on every
  /// repaint.
  text: string
  /// Stable key for `eq()` — two widgets with the same `text` are
  /// `.eq()` so CM6's diffing keeps the existing DOM node.
  key: string
}

/// Decoration widget rendering one annotation. The CM6 widget
/// contract is `WidgetType` with `toDOM` + `eq`; both are minimal.
class BlameWidget extends WidgetType {
  constructor(private readonly row: BlameLineRow) {
    super()
  }
  toDOM() {
    const span = document.createElement('span')
    span.className = 'nexus-git-blame-line'
    span.textContent = `   ${this.row.text}`
    return span
  }
  eq(other: WidgetType): boolean {
    return other instanceof BlameWidget && other.row.key === this.row.key
  }
}

const setBlameRows = StateEffect.define<Map<number, BlameLineRow>>()

/// State field of `line → annotation`. The view-plugin computes
/// the decoration set lazily from this; keeping the decorations in
/// a derived view (not the field) avoids re-running the whole map
/// on every transaction.
const blameRowsField = StateField.define<Map<number, BlameLineRow>>({
  create: () => new Map(),
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setBlameRows)) return e.value
    }
    return value
  },
})

/// Compute the decoration set from the current blame map. Pure —
/// the view-plugin reuses this on every update without state.
function buildDecorations(
  view: EditorView,
  rows: Map<number, BlameLineRow>,
): DecorationSet {
  const builder: { from: number; widget: BlameWidget; side: number }[] = []
  const total = view.state.doc.lines
  for (const [line, row] of rows) {
    if (line < 1 || line > total) continue
    const lineObj = view.state.doc.line(line)
    builder.push({
      from: lineObj.to,
      widget: new BlameWidget(row),
      side: 1,
    })
  }
  // CM requires decoration ranges in ascending `from` order.
  builder.sort((a, b) => a.from - b.from)
  return Decoration.set(
    builder.map((b) =>
      Decoration.widget({ widget: b.widget, side: b.side }).range(b.from),
    ),
  )
}

interface GitBlameDeps {
  relpath: string
  kernel: {
    invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T>
    available?(): Promise<boolean>
  }
  /** Optional callback for blame-fetch errors. Defaults to silent. */
  onError?: (err: unknown) => void
}

/**
 * BL-079 — root extension. Mount in the CM stack to opt into the
 * inline blame annotations; unmount (e.g. via Compartment.reconfigure)
 * to clear them. Untitled tabs should not mount this — the blame
 * IPC will error on a path that doesn't exist in HEAD.
 */
export function gitBlameExt(deps: GitBlameDeps): Extension {
  const fetchBlame = async (view: EditorView) => {
    try {
      if (deps.kernel.available && !(await deps.kernel.available())) return
      const entries = await deps.kernel.invoke<GitBlameEntry[]>(
        PLUGIN_ID,
        CMD_BLAME,
        { path: deps.relpath },
      )
      const rows = new Map<number, BlameLineRow>()
      for (const e of entries ?? []) {
        const text = formatBlameRow(e)
        const key = `${e.commit_hash}:${e.start_line}-${e.end_line}`
        for (let line = e.start_line; line <= e.end_line; line += 1) {
          rows.set(line, { text, key: `${key}:${line}` })
        }
      }
      view.dispatch({ effects: setBlameRows.of(rows) })
    } catch (err) {
      deps.onError?.(err)
      view.dispatch({ effects: setBlameRows.of(new Map()) })
    }
  }

  const watcher = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet = Decoration.none
      constructor(view: EditorView) {
        // Compute decorations from whatever's in the field at mount
        // time, then trigger a fresh fetch.
        this.decorations = buildDecorations(
          view,
          view.state.field(blameRowsField, false) ?? new Map(),
        )
        void fetchBlame(view)
      }
      update(u: ViewUpdate) {
        const newRows = u.state.field(blameRowsField, false)
        const oldRows = u.startState.field(blameRowsField, false)
        if (newRows !== oldRows) {
          this.decorations = buildDecorations(u.view, newRows ?? new Map())
        } else if (u.docChanged) {
          // Doc edits shift line numbers — rebuild against the
          // existing map so the annotations float with their
          // lines. A fresh fetch will reset to truth on save.
          this.decorations = buildDecorations(u.view, newRows ?? new Map())
        }
      }
    },
    {
      decorations: (v) => v.decorations,
    },
  )

  return [blameRowsField, watcher]
}

/**
 * Format a single blame entry into the muted-line annotation.
 * Pure — exported for unit tests so the format isn't an integration-
 * test concern.
 */
export function formatBlameRow(entry: GitBlameEntry): string {
  const author = entry.author.split(' ')[0] || entry.author
  const hash = entry.commit_hash.slice(0, 7)
  const date = formatRelativeDate(entry.date)
  const summary = entry.message.length > 60
    ? `${entry.message.slice(0, 57)}…`
    : entry.message
  return `${author} · ${hash} · ${date} · ${summary}`
}

/**
 * Render `iso` (RFC-3339) as a coarse human duration. The blame
 * annotation cares about "recently vs. ages ago", not exact
 * timestamps.
 */
export function formatRelativeDate(iso: string): string {
  const ts = Date.parse(iso)
  if (Number.isNaN(ts)) return ''
  const deltaSec = Math.max(0, (Date.now() - ts) / 1000)
  if (deltaSec < 60) return 'just now'
  if (deltaSec < 3600) return `${Math.floor(deltaSec / 60)}m ago`
  if (deltaSec < 86400) return `${Math.floor(deltaSec / 3600)}h ago`
  if (deltaSec < 86400 * 30) return `${Math.floor(deltaSec / 86400)}d ago`
  if (deltaSec < 86400 * 365) {
    return `${Math.floor(deltaSec / (86400 * 30))}mo ago`
  }
  return `${Math.floor(deltaSec / (86400 * 365))}y ago`
}
