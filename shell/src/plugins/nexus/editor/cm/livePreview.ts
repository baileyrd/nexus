import { syntaxTree } from '@codemirror/language'
import { StateEffect, StateField, type Extension, type Range } from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'
import {
  buildLivePreviewBlockDecorations,
  buildLivePreviewInlineDecorations,
  forgeImageContext,
  type ForgeImageContext,
} from './livePreviewDecorations'
import { fencedCodeRegistry } from './fencedCodeRegistry'

const fencedRegistryChanged = StateEffect.define<null>()

/**
 * Live-preview rendering for the markdown CM6 editor.
 *
 * BL-125 splits the decoration source in two:
 *
 *   1. **Block decorations** — HR widget, table widget, fenced-code
 *      widget — come from a `StateField`. CM6 requires block widgets
 *      to be StateField-sourced (a `RangeError: Block decorations may
 *      not be specified via plugins` fires otherwise). Block
 *      constructs are rare per doc, so the full-tree walk is cheap.
 *
 *   2. **Inline decorations** — emphasis / strong / inline code /
 *      links / list markers / headings / blockquotes / non-rendered
 *      fenced-code line decorations — come from a `ViewPlugin` that
 *      walks only `view.visibleRanges`. Per-frame cost becomes
 *      O(visible nodes) instead of O(document nodes), which is the
 *      core BL-125 typing-latency win on large docs.
 *
 * Both sources contribute to `EditorView.decorations` (combined
 * automatically by CM6's facet) and to `EditorView.atomicRanges` so
 * the cursor doesn't park inside hidden marks.
 *
 * The companion `ViewPlugin` subscribes to `fencedCodeRegistry.onChange`
 * so a renderer registered after the editor mounts (e.g. when the
 * user enables the mermaid plugin via Settings) triggers a recompute
 * via the `fencedRegistryChanged` effect — without this, existing
 * fenced blocks stay raw until the next doc edit or selection move.
 */
export interface LivePreviewOptions {
  /** C1 (#354) — when present, whole-line `![](…)` images render as
   *  block widgets (see `forgeImageContext`). Absent → images keep
   *  the v1 mark-only styling; every existing caller/test that passes
   *  no options is unchanged. */
  forgeImages?: ForgeImageContext
}

export function livePreviewExt(options: LivePreviewOptions = {}): Extension {
  const blockField = StateField.define<DecorationSet>({
    create(state) {
      return buildLivePreviewBlockDecorations(state)
    },
    update(value, tr) {
      if (
        tr.docChanged ||
        tr.selection ||
        tr.effects.some((e) => e.is(fencedRegistryChanged)) ||
        // Lezer parses incrementally and asynchronously. When the parser
        // catches up after initial load the tree object is replaced — detect
        // that so tables/headings render without requiring a cursor move.
        syntaxTree(tr.state) !== syntaxTree(tr.startState)
      ) {
        return buildLivePreviewBlockDecorations(tr.state)
      }
      return value
    },
    provide(f) {
      return [
        EditorView.decorations.from(f),
        // Block widgets (and the atomic range that prevents the cursor
        // parking inside their hidden source) live here.
        EditorView.atomicRanges.of((view) => view.state.field(f) ?? Decoration.none),
      ]
    },
  })

  const inlinePlugin = ViewPlugin.fromClass(
    class implements PluginValue {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = buildLivePreviewInlineDecorations(
          view.state,
          view.visibleRanges,
        )
      }
      update(update: ViewUpdate): void {
        // Recompute on doc / selection / viewport change, or when the
        // lezer parser catches up (same identity-flip check as the
        // block field). Scrolling fires `viewportChanged` so the
        // newly-visible region picks up its decorations on the next
        // frame.
        if (
          update.docChanged ||
          update.selectionSet ||
          update.viewportChanged ||
          update.transactions.some((tr) =>
            tr.effects.some((e) => e.is(fencedRegistryChanged)),
          ) ||
          syntaxTree(update.state) !== syntaxTree(update.startState)
        ) {
          this.decorations = buildLivePreviewInlineDecorations(
            update.state,
            update.view.visibleRanges,
          )
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      // Atomic ranges for the inline replaces so left-arrow doesn't
      // park inside an invisible `**`. Cursor motion that crosses the
      // viewport boundary lands in the next viewport's freshly-
      // recomputed decoration set on the following frame.
      provide: (plugin) =>
        EditorView.atomicRanges.of((view) => {
          const inst = view.plugin(plugin)
          return inst?.decorations ?? Decoration.none
        }),
    },
  )

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

  const exts: Extension[] = [blockField, inlinePlugin, watcher]
  if (options.forgeImages) {
    exts.push(forgeImageContext.of(options.forgeImages))
  }
  return exts
}

// Reference to silence "unused" warnings for the legacy `Range` type
// import path that older CM6 versions exposed but the 6.x line keeps
// available via the state package.
void (null as unknown as Range<Decoration>)
