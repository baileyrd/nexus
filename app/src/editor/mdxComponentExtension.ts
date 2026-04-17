/**
 * CodeMirror 6 extension that renders registered MDX components (PRD-08 §7)
 * as inline widgets.
 *
 * # Why a plain-DOM pipeline, not `@mdx-js/mdx`
 *
 * The canonical MDX runtime compiles JSX to JS and executes it via
 * `new Function(...)`, which requires a CSP `'unsafe-eval'` directive.
 * We shipped a strict CSP (UI F-5.1.2) that excludes `'unsafe-eval'`
 * in production, so plugin-supplied JSX can never be evaluated in the
 * shell origin. The pipeline here is instead:
 *
 *   1. Scan visible document text for self-closing `<Name prop="val" />`
 *      tags where `Name` is a registered component.
 *   2. Parse the attribute list into a plain `Record<string, unknown>`,
 *      coercing unquoted numbers and booleans.
 *   3. Call the component's `render(props)` to get a `PanelNode` tree.
 *   4. Walk the tree with a fixed host dispatcher that only emits the
 *      approved primitives (`vstack`, `hstack`, `text`, `heading`,
 *      `button`, `spacer`) as plain `HTMLElement`s.
 *   5. Wrap the result in a CM6 `WidgetType` and attach via a replace
 *      decoration at the tag's source range.
 *
 * No plugin-supplied code touches the DOM directly; every node kind is
 * host-reviewed. This matches the `registerPanelView` trade-off (UI
 * F-5.2.1) — safety over unrestricted HTML.
 *
 * # Scope of v1
 *
 * - Self-closing tags only (`<Card title="Hi" />`). Block-form tags
 *   with nested markdown children (`<Card>...</Card>`) require
 *   matching the close-tag and feeding the inner range back through
 *   the markdown parser; tracked as a follow-up.
 * - Attributes must be double-quoted strings, unquoted numbers, or the
 *   bareword `true`/`false`. JSX expression attributes (`{…}`) are not
 *   parsed — the scanner treats them as opaque and ignores the tag.
 * - Multiline tags are supported by the regex (`.` with `s` flag).
 */

import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  ViewPlugin,
  WidgetType,
  type ViewUpdate,
} from "@codemirror/view";
import type { MdxComponent, PanelNode } from "../contributions";

/**
 * Regex matching a self-closing JSX tag whose name begins with an
 * uppercase letter. Captures:
 *   1 — component name
 *   2 — attribute list (possibly multiline; may be empty)
 */
const SELF_CLOSING_RE = /<([A-Z][A-Za-z0-9]*)\b([^>]*?)\/>/g;

/**
 * Regex matching a single `name="value"` or `name={value}` or `name`
 * attribute within the captured attribute group. The `s` flag allows
 * values to span lines (rare but legal).
 */
const ATTR_RE = /\s+([A-Za-z_][A-Za-z0-9_-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|\{([^}]*)\}|([^\s/>"'=]+)))?/g;

function parseAttributes(raw: string): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  let m: RegExpExecArray | null;
  // Reset global regex state between calls.
  ATTR_RE.lastIndex = 0;
  // eslint-disable-next-line no-cond-assign
  while ((m = ATTR_RE.exec(raw))) {
    const [, name, dq, sq, expr, bare] = m;
    if (dq !== undefined) {
      out[name] = dq;
    } else if (sq !== undefined) {
      out[name] = sq;
    } else if (expr !== undefined) {
      // JSX expression attributes (`count={3}`). We accept a small subset —
      // numbers and booleans — and bail on anything else. Bailing means the
      // attribute becomes `undefined` rather than a shell-interpreted string,
      // which is safer than pretending we understood.
      const trimmed = expr.trim();
      if (/^-?\d+(\.\d+)?$/.test(trimmed)) out[name] = Number(trimmed);
      else if (trimmed === "true") out[name] = true;
      else if (trimmed === "false") out[name] = false;
      // else: drop attribute — host will see `undefined` and render a default.
    } else if (bare !== undefined) {
      // Unquoted value, e.g. `level=info` — treat as string.
      out[name] = bare;
    } else {
      // Valueless attribute: JSX convention is "present = true".
      out[name] = true;
    }
  }
  return out;
}

/**
 * Walk a `PanelNode` tree, emitting plain HTMLElements. Mirrors the host
 * React dispatcher in `registry.ts` (`renderPanelNode`) but uses the DOM
 * directly so CM6 widgets can hand the node over without a React root.
 */
function panelNodeToDom(node: PanelNode): HTMLElement {
  switch (node.type) {
    case "vstack": {
      const el = document.createElement("div");
      el.className = "mdx-panel mdx-panel-vstack";
      el.style.display = "flex";
      el.style.flexDirection = "column";
      el.style.gap = `${node.gap ?? 8}px`;
      for (const child of node.children) {
        el.appendChild(panelNodeToDom(child));
      }
      return el;
    }
    case "hstack": {
      const el = document.createElement("div");
      el.className = "mdx-panel mdx-panel-hstack";
      el.style.display = "flex";
      el.style.flexDirection = "row";
      el.style.alignItems = "center";
      el.style.gap = `${node.gap ?? 8}px`;
      for (const child of node.children) {
        el.appendChild(panelNodeToDom(child));
      }
      return el;
    }
    case "text": {
      const el = document.createElement("span");
      el.className = "mdx-panel-text";
      el.textContent = node.value;
      if (node.muted) el.style.opacity = "0.7";
      if (node.strong) el.style.fontWeight = "600";
      return el;
    }
    case "heading": {
      const tag = `h${node.level ?? 3}`;
      const el = document.createElement(tag);
      el.className = "mdx-panel-heading";
      el.textContent = node.value;
      return el;
    }
    case "button": {
      const el = document.createElement("button");
      el.type = "button";
      el.className = "mdx-panel-button";
      el.textContent = node.label;
      if (node.disabled) el.disabled = true;
      // NOTE: Buttons inside CM6 widgets fire the `commandId` through the
      // host dispatcher — done at widget attach time in the plugin below
      // because `panelNodeToDom` has no access to `contributions` here
      // without creating an import cycle. See `MdxComponentWidget.toDOM`.
      el.dataset.mdxCommandId = node.commandId;
      return el;
    }
    case "spacer": {
      const el = document.createElement("div");
      el.className = "mdx-panel-spacer";
      el.style.height = `${node.size ?? 8}px`;
      return el;
    }
    default: {
      const el = document.createElement("div");
      el.className = "mdx-panel-unknown";
      el.setAttribute("role", "alert");
      el.textContent = `unknown panel node: ${JSON.stringify(node)}`;
      return el;
    }
  }
}

/**
 * CM6 widget wrapping one rendered MDX component. `eq` compares on the
 * tag source text so a docChange that didn't touch this widget keeps the
 * same DOM node (no flicker).
 */
class MdxComponentWidget extends WidgetType {
  constructor(
    readonly source: string,
    readonly dom: HTMLElement,
    readonly onCommand: (commandId: string) => void,
  ) {
    super();
  }

  override eq(other: WidgetType): boolean {
    return other instanceof MdxComponentWidget && other.source === this.source;
  }

  override toDOM(): HTMLElement {
    // Wire button clicks to the host command dispatcher. Plain-DOM
    // delegation beats re-doing tree-walk here because buttons can
    // live deeply nested in vstack/hstack containers.
    const wrapper = document.createElement("span");
    wrapper.className = "mdx-component-widget";
    wrapper.setAttribute("contenteditable", "false");
    wrapper.style.display = "inline-block";
    wrapper.style.verticalAlign = "top";
    wrapper.style.border = "1px dashed var(--nx-border, #888)";
    wrapper.style.borderRadius = "4px";
    wrapper.style.padding = "4px 6px";
    wrapper.style.margin = "0 1px";
    wrapper.style.background = "var(--nx-bg-muted, rgba(0,0,0,0.03))";
    wrapper.appendChild(this.dom.cloneNode(true));

    // Click delegation for any button produced by the renderer.
    wrapper.addEventListener("click", (event) => {
      const target = event.target as HTMLElement | null;
      const btn = target?.closest("button[data-mdx-command-id]") as
        | HTMLButtonElement
        | null;
      if (btn && !btn.disabled) {
        event.preventDefault();
        event.stopPropagation();
        this.onCommand(btn.dataset.mdxCommandId ?? "");
      }
    });
    return wrapper;
  }

  override ignoreEvent(): boolean {
    // Let clicks reach our listener; otherwise the editor treats widget
    // clicks as caret movement and swallows them.
    return false;
  }
}

/**
 * Build the CM6 extension. `components` is the live registry snapshot —
 * callers (EditorSurface) swap the compartment when the snapshot changes.
 * `onCommand` bridges widget button clicks to `contributions.invokeCommand`.
 */
export function buildMdxComponentExtension(
  components: readonly MdxComponent[],
  onCommand: (commandId: string) => void,
): Extension {
  if (components.length === 0) return [];

  const byName = new Map<string, MdxComponent>();
  for (const c of components) byName.set(c.name, c);

  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = this.build(view);
      }

      update(u: ViewUpdate): void {
        if (u.docChanged || u.viewportChanged) {
          this.decorations = this.build(u.view);
        }
      }

      build(view: EditorView): DecorationSet {
        const builder = new RangeSetBuilder<Decoration>();
        const hits: Range<Decoration>[] = [];

        // Only scan visible ranges so huge documents stay responsive.
        for (const { from, to } of view.visibleRanges) {
          const text = view.state.doc.sliceString(from, to);
          SELF_CLOSING_RE.lastIndex = 0;
          let m: RegExpExecArray | null;
          // eslint-disable-next-line no-cond-assign
          while ((m = SELF_CLOSING_RE.exec(text))) {
            const [full, name, attrs] = m;
            const component = byName.get(name);
            if (!component) continue;

            // Skip matches that fall inside a code block / inline code —
            // lean on the markdown syntax tree for this check so we don't
            // render widgets inside fenced code samples demonstrating MDX.
            const matchStart = from + m.index;
            const matchEnd = matchStart + full.length;
            if (isInsideCode(view, matchStart)) continue;

            let tree: PanelNode;
            try {
              tree = component.render(parseAttributes(attrs));
            } catch (err) {
              // Render the raw source as a failure marker so plugin
              // authors see their render threw without crashing the
              // editor. Skip the widget by continuing.
              // eslint-disable-next-line no-console
              console.error(
                `[mdx] component '${name}' render threw:`,
                err,
              );
              continue;
            }
            const dom = panelNodeToDom(tree);
            hits.push(
              Decoration.replace({
                widget: new MdxComponentWidget(full, dom, onCommand),
                inclusive: false,
              }).range(matchStart, matchEnd),
            );
          }
        }

        // CM6 requires builder.add() calls in position order; sort first.
        hits.sort((a, b) => a.from - b.from);
        for (const h of hits) builder.add(h.from, h.to, h.value);
        return builder.finish();
      }
    },
    { decorations: (v) => v.decorations },
  );
}

/**
 * Best-effort "is this position inside a code block or inline code?"
 * lookup against the Lezer syntax tree. Returns false if the tree isn't
 * ready or the document isn't markdown — callers fall through to
 * rendering, which is the right default.
 */
function isInsideCode(view: EditorView, pos: number): boolean {
  try {
    const tree = syntaxTree(view.state);
    const node = tree.resolveInner(pos, 1);
    for (let cur: typeof node | null = node; cur; cur = cur.parent) {
      const name = cur.name;
      if (
        name === "CodeBlock" ||
        name === "FencedCode" ||
        name === "InlineCode"
      ) {
        return true;
      }
    }
  } catch {
    // Tree not ready (e.g. during initial parse) — safe to render.
  }
  return false;
}
