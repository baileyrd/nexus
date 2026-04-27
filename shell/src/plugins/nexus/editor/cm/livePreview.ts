import { StateField, type Extension } from '@codemirror/state'
import { Decoration, EditorView, type DecorationSet } from '@codemirror/view'
import { buildLivePreviewDecorations } from './livePreviewDecorations'

/**
 * Decoration source must be a `StateField` rather than a `ViewPlugin` —
 * the HR widget is a block decoration, and CM6 disallows block decorations
 * from plugin-provided sources (`RangeError: Block decorations may not be
 * specified via plugins`).
 *
 * The field recomputes whenever the doc changes or the selection moves so
 * syntax marks fade in/out as the cursor crosses lines. Atomic ranges keep
 * the cursor from parking inside a hidden replace.
 */
export function livePreviewExt(): Extension {
  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildLivePreviewDecorations(state)
    },
    update(value, tr) {
      if (tr.docChanged || tr.selection) {
        return buildLivePreviewDecorations(tr.state)
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
  return [field]
}
