import type { Extension } from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view'
import { buildLivePreviewDecorations } from './livePreviewDecorations'

/**
 * Live-preview ViewPlugin. Recomputes the decoration set on every doc
 * change, selection change, and viewport change so syntax marks fade
 * in/out as the cursor moves between lines.
 *
 * The atomic-ranges provider guarantees cursor motion can't park
 * inside a hidden replace range — left/right arrow steps over the
 * whole `**` rather than landing inside it.
 */
export function livePreviewExt(): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = buildLivePreviewDecorations(view.state)
      }
      update(u: ViewUpdate): void {
        if (u.docChanged || u.selectionSet || u.viewportChanged) {
          this.decorations = buildLivePreviewDecorations(u.state)
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      provide: (p) =>
        EditorView.atomicRanges.of((view) => view.plugin(p)?.decorations ?? Decoration.none),
    },
  )
  return [plugin]
}
