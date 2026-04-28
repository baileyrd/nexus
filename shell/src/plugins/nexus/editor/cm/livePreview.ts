import { StateEffect, StateField, type Extension } from '@codemirror/state'
import { Decoration, EditorView, ViewPlugin, type DecorationSet } from '@codemirror/view'
import { buildLivePreviewDecorations } from './livePreviewDecorations'
import { fencedCodeRegistry } from './fencedCodeRegistry'

const fencedRegistryChanged = StateEffect.define<null>()

/**
 * Decoration source must be a `StateField` rather than a `ViewPlugin` —
 * the HR widget is a block decoration, and CM6 disallows block decorations
 * from plugin-provided sources (`RangeError: Block decorations may not be
 * specified via plugins`).
 *
 * The field recomputes whenever the doc changes or the selection moves so
 * syntax marks fade in/out as the cursor crosses lines. Atomic ranges keep
 * the cursor from parking inside a hidden replace.
 *
 * The companion `ViewPlugin` subscribes to `fencedCodeRegistry.onChange`
 * so a renderer registered after the editor mounts (e.g. when the user
 * enables the mermaid plugin via Settings) triggers a recompute via the
 * `fencedRegistryChanged` effect — without this, existing fenced blocks
 * stay raw until the next doc edit or selection move.
 */
export function livePreviewExt(): Extension {
  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildLivePreviewDecorations(state)
    },
    update(value, tr) {
      if (
        tr.docChanged ||
        tr.selection ||
        tr.effects.some((e) => e.is(fencedRegistryChanged))
      ) {
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

  const watcher = ViewPlugin.define((view) => {
    const unsub = fencedCodeRegistry.onChange(() => {
      view.dispatch({ effects: fencedRegistryChanged.of(null) })
    })
    return {
      destroy() {
        unsub()
      },
    }
  })

  return [field, watcher]
}
