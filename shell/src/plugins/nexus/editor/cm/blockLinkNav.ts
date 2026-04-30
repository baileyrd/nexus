// CodeMirror 6 navigation surface for inline `[[<file>#^<uuid>]]`
// block links (BL-049 phase 2). Two responsibilities:
//
//   * decorate each occurrence with a `cm-md-block-link` mark so
//     the user can see clickable ranges without staring at the
//     raw syntax;
//   * intercept left-button clicks within those ranges and call
//     `deps.onNavigate(link)` so the editor plugin can dispatch
//     `files:open` + a follow-up `reveal-block` event.
//
// The decoration source uses a `StateField` keyed on the doc
// content + selection so the marks rebuild when the user types
// (matches the BL-012 split-3 pattern exactly).
//
// Click handling lives in a `ViewPlugin` rather than inside the
// state field — pointer events arrive via `eventHandlers` and
// each call needs a synchronous ack (`return true`) to suppress
// the default text-selection drag.

import { type EditorState, type Extension, StateField } from '@codemirror/state'
import { Decoration, type DecorationSet, EditorView, ViewPlugin } from '@codemirror/view'

import { type ParsedBlockLink, parseBlockLinks } from '../blockLinks'

export interface BlockLinkNavDeps {
  /** Click handler invoked when the user activates a block link.
   *  Receives the parsed link (path / blockId / label / range);
   *  return value is unused. The plugin emits `files:open` here
   *  plus the follow-up `nexus.editor:reveal-block` event. */
  onNavigate: (link: ParsedBlockLink) => void
}

const BLOCK_LINK_MARK = Decoration.mark({ class: 'cm-md-block-link' })

/** Pure decoration builder — emits one `Decoration.mark` per
 *  `[[<file>#^<uuid>]]` occurrence. Exported so unit tests can
 *  pin the contents of the set without driving a `ViewPlugin`. */
export function buildBlockLinkDecorations(state: EditorState): DecorationSet {
  const text = state.doc.toString()
  const links = parseBlockLinks(text)
  const ranges = links.map((l) => BLOCK_LINK_MARK.range(l.from, l.to))
  return Decoration.set(ranges, true)
}

/** Search the document for the `<!-- ^<uuid> -->` stable-id
 *  marker and scroll its line into view, parking the selection
 *  at the marker's position. Returns `true` on success, `false`
 *  if the marker isn't present (the kernel resolver said the
 *  block exists but the on-disk source hasn't been re-saved
 *  with the marker yet — the caller can fall back to
 *  `root_index`-based scrolling).
 *
 *  Exported so the editor plugin can call it from a `files:open`
 *  follow-up handler once the target tab finishes loading. */
export function revealBlockInView(view: EditorView, blockId: string): boolean {
  const needle = `<!-- ^${blockId.toLowerCase()} -->`
  const text = view.state.doc.toString()
  const idx = text.toLowerCase().indexOf(needle)
  if (idx < 0) return false
  const line = view.state.doc.lineAt(idx)
  view.dispatch({
    selection: { anchor: line.from },
    effects: EditorView.scrollIntoView(line.from, { y: 'center' }),
  })
  return true
}

/** CM extension: state-field decorations + a ViewPlugin click
 *  handler that consults the same parser to map a click position
 *  to a `ParsedBlockLink`. */
export function blockLinkNavExt(deps: BlockLinkNavDeps): Extension {
  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildBlockLinkDecorations(state)
    },
    update(value, tr) {
      if (tr.docChanged) return buildBlockLinkDecorations(tr.state)
      return value
    },
    provide(f) {
      return EditorView.decorations.from(f)
    },
  })

  const handler = ViewPlugin.define(() => ({}), {
    eventHandlers: {
      mousedown(this: unknown, event: MouseEvent, view: EditorView) {
        const pos = view.posAtDOM(event.target as Node)
        return handleBlockLinkMousedown(view.state, pos, event, deps)
      },
    },
  })

  return [field, handler]
}

/** Pure click-routing logic — returns `true` when the event is a
 *  block-link activation (caller should `preventDefault` + dispatch),
 *  `false` to fall through to CM's default mousedown handling.
 *  Exported so unit tests can pin every chord / hit-test branch
 *  without driving CM's internal `posAtDOM` machinery. */
export function handleBlockLinkMousedown(
  state: EditorState,
  pos: number,
  event: { button: number; metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean; preventDefault?: () => void },
  deps: BlockLinkNavDeps,
): boolean {
  // Only intercept plain left-button clicks; chord+click
  // (Mod-click for "open in split", Shift-click for selection
  // extension) keeps default behaviour so power users don't lose
  // the affordance.
  if (event.button !== 0) return false
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return false
  const link = blockLinkAtPos(state, pos)
  if (!link) return false
  event.preventDefault?.()
  deps.onNavigate(link)
  return true
}

/** Internal: like `blockLinkAt` but takes a CM `EditorState` so
 *  callers don't have to materialise the full doc string each
 *  time. Equivalent semantics. */
function blockLinkAtPos(state: EditorState, pos: number): ParsedBlockLink | null {
  // Scan a small window around `pos` rather than the full doc —
  // block links are line-local. Two lines on either side covers
  // the case where a user clicks the closing `]]` near a line
  // boundary.
  const line = state.doc.lineAt(pos)
  const fromLine = Math.max(1, line.number - 1)
  const toLine = Math.min(state.doc.lines, line.number + 1)
  const start = state.doc.line(fromLine).from
  const end = state.doc.line(toLine).to
  const slice = state.doc.sliceString(start, end)
  for (const link of parseBlockLinks(slice, start)) {
    if (pos >= link.from && pos <= link.to) return link
  }
  return null
}
