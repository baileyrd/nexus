/**
 * Live-preview markdown decorations for CodeMirror 6.
 *
 * Walks the Lezer markdown syntax tree in the visible viewport and emits
 * two kinds of decorations:
 *  - `Decoration.replace` to hide marker characters (e.g. `**`, `#`, `[`, `]`, `(url)`)
 *  - `Decoration.mark`    to style the surviving content (e.g. `.cm-nx-bold`)
 *
 * When the current selection overlaps a node (inclusive bounds), the
 * marker-hiding `replace` is skipped so the raw syntax reveals and the
 * user can edit it. The styling `mark` is still applied in that case.
 *
 * Implements the Live Preview pattern described by PRD 08 §4.3.
 */

import { syntaxTree } from "@codemirror/language";
import { type EditorSelection, type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";

const HEADING_LEVELS = new Set([
  "ATXHeading1",
  "ATXHeading2",
  "ATXHeading3",
  "ATXHeading4",
  "ATXHeading5",
  "ATXHeading6",
]);

/** Inclusive overlap: touching either edge counts as "in range". */
function cursorOverlaps(sel: EditorSelection, from: number, to: number): boolean {
  return sel.ranges.some((r) => r.from <= to && r.to >= from);
}

/** Live-preview options. `alwaysHideMarkers` corresponds to Reading
 *  mode (PRD-08 §15.3): syntax tokens are hidden even when the cursor
 *  overlaps, giving the user a fully rendered view. */
export interface LiveMarkdownOptions {
  alwaysHideMarkers?: boolean;
}

/** Push a `Decoration.replace` range covering every child of `node` whose
 *  name is in `names`. Used to hide marker children like `HeaderMark` and
 *  `EmphasisMark`. */
function hideChildren(
  node: SyntaxNode,
  names: ReadonlySet<string>,
  out: Array<Range<Decoration>>,
): void {
  for (let c = node.firstChild; c; c = c.nextSibling) {
    if (names.has(c.name)) {
      out.push(Decoration.replace({}).range(c.from, c.to));
    }
  }
}

const HEADER_MARK = new Set(["HeaderMark"]);
const EMPHASIS_MARK = new Set(["EmphasisMark"]);
const CODE_MARK = new Set(["CodeMark"]);
const LINK_MARK_AND_URL = new Set(["LinkMark", "URL"]);

function build(view: EditorView, opts: LiveMarkdownOptions): DecorationSet {
  const decos: Array<Range<Decoration>> = [];
  const sel = view.state.selection;
  const doc = view.state.doc;
  const alwaysHide = opts.alwaysHideMarkers === true;

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from,
      to,
      enter(nodeRef) {
        const name = nodeRef.name;
        const node = nodeRef.node;
        const nFrom = nodeRef.from;
        const nTo = nodeRef.to;
        const reveal = !alwaysHide && cursorOverlaps(sel, nFrom, nTo);

        if (HEADING_LEVELS.has(name)) {
          const level = Number(name.slice(-1));
          if (!reveal) hideChildren(node, HEADER_MARK, decos);
          decos.push(
            Decoration.mark({ class: `cm-nx-h${level}` }).range(nFrom, nTo),
          );
          return;
        }

        switch (name) {
          case "StrongEmphasis": {
            if (!reveal) hideChildren(node, EMPHASIS_MARK, decos);
            decos.push(
              Decoration.mark({ class: "cm-nx-bold" }).range(nFrom, nTo),
            );
            break;
          }
          case "Emphasis": {
            if (!reveal) hideChildren(node, EMPHASIS_MARK, decos);
            decos.push(
              Decoration.mark({ class: "cm-nx-italic" }).range(nFrom, nTo),
            );
            break;
          }
          case "InlineCode": {
            if (!reveal) hideChildren(node, CODE_MARK, decos);
            decos.push(
              Decoration.mark({ class: "cm-nx-inline-code" }).range(nFrom, nTo),
            );
            break;
          }
          case "Link": {
            // Lezer markdown: children are LinkMark "[", text, LinkMark "]",
            // LinkMark "(", URL, LinkMark ")". Hide all marks + the URL;
            // style only the display text in between the opening "[" and
            // closing "]".
            const first = node.firstChild;
            let displayFrom = nFrom;
            let displayTo = nTo;
            if (first?.name === "LinkMark") displayFrom = first.to;
            // The closing "]" is the second LinkMark; find it by walking.
            let bracketCloseFound = false;
            for (let c = first?.nextSibling ?? null; c; c = c.nextSibling) {
              if (!bracketCloseFound && c.name === "LinkMark") {
                displayTo = c.from;
                bracketCloseFound = true;
                break;
              }
            }
            if (!reveal) hideChildren(node, LINK_MARK_AND_URL, decos);
            if (displayTo > displayFrom) {
              decos.push(
                Decoration.mark({ class: "cm-nx-link" }).range(
                  displayFrom,
                  displayTo,
                ),
              );
            }
            break;
          }
          case "Blockquote": {
            // Line decoration per line of the blockquote. Quote marker
            // (`>`) stays visible; CSS styles the whole line.
            const startLine = doc.lineAt(nFrom).number;
            const endLine = doc.lineAt(nTo).number;
            for (let ln = startLine; ln <= endLine; ln++) {
              const lineStart = doc.line(ln).from;
              decos.push(
                Decoration.line({ class: "cm-nx-blockquote" }).range(lineStart),
              );
            }
            break;
          }
          default:
            break;
        }
      },
    });
  }

  return Decoration.set(decos, /* sort */ true);
}

/** CodeMirror extension factory: live-preview markdown decorations. */
export function liveMarkdown(opts: LiveMarkdownOptions = {}): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, opts);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.selectionSet || update.viewportChanged) {
          this.decorations = build(update.view, opts);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );
}
