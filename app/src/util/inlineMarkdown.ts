// Compact inline-markdown → typed AST parser used by MDX block-form
// components. Scope is deliberately small — just enough to make Card /
// Callout / Alert bodies feel like markdown without pulling a full
// parser dependency into the widget path:
//
//   - Headings: `#` / `##` / `###` at line start.
//   - Bullet lists: `- ` / `* ` at line start; blank line closes the list.
//   - Paragraphs: groups of non-empty lines separated by blank lines.
//   - Inline: `**bold**`, `*italic*` / `_italic_`, `` `code` ``,
//     `[[wikilink]]`, `[text](url)`.
//
// Unknown syntax falls through as literal text so the user's source
// is never lost, just not stylized.

export type MdInline =
  | { type: "text"; value: string }
  | { type: "bold"; children: MdInline[] }
  | { type: "italic"; children: MdInline[] }
  | { type: "code"; value: string }
  | { type: "link"; href: string; children: MdInline[] }
  | { type: "wikilink"; target: string };

export type MdBlock =
  | { type: "heading"; level: 1 | 2 | 3; children: MdInline[] }
  | { type: "paragraph"; children: MdInline[] }
  | { type: "list"; items: MdInline[][] };

/** Parse a block of markdown text into a typed AST. */
export function parseInlineMarkdown(source: string): MdBlock[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: MdBlock[] = [];

  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    const trimmed = line.trimStart();

    // Blank line: separator only.
    if (trimmed === "") {
      i += 1;
      continue;
    }

    // Heading
    const headingMatch = /^(#{1,3})\s+(.*)$/.exec(trimmed);
    if (headingMatch) {
      const level = headingMatch[1]!.length as 1 | 2 | 3;
      blocks.push({
        type: "heading",
        level,
        children: parseInline(headingMatch[2] ?? ""),
      });
      i += 1;
      continue;
    }

    // Bullet list — consecutive lines starting with `- ` or `* ` merge
    // into a single list block.
    if (/^[-*]\s+/.test(trimmed)) {
      const items: MdInline[][] = [];
      while (i < lines.length) {
        const l = (lines[i] ?? "").trimStart();
        const m = /^[-*]\s+(.*)$/.exec(l);
        if (!m) break;
        items.push(parseInline(m[1] ?? ""));
        i += 1;
      }
      blocks.push({ type: "list", items });
      continue;
    }

    // Paragraph — accumulate non-blank, non-special lines.
    const paraLines: string[] = [];
    while (i < lines.length) {
      const l = lines[i] ?? "";
      const t = l.trimStart();
      if (t === "") break;
      if (/^(#{1,3})\s+/.test(t)) break;
      if (/^[-*]\s+/.test(t)) break;
      paraLines.push(l);
      i += 1;
    }
    if (paraLines.length > 0) {
      blocks.push({
        type: "paragraph",
        children: parseInline(paraLines.join(" ")),
      });
    }
  }

  return blocks;
}

/** Parse a single line/phrase into inline nodes. */
export function parseInline(text: string): MdInline[] {
  const out: MdInline[] = [];
  let rest = text;
  let buffer = "";
  const flush = () => {
    if (buffer.length > 0) {
      out.push({ type: "text", value: buffer });
      buffer = "";
    }
  };

  const matchers: Array<
    (s: string) => { node: MdInline; consumed: number } | null
  > = [
    (s) => {
      const m = /^`([^`]+)`/.exec(s);
      return m ? { node: { type: "code", value: m[1]! }, consumed: m[0].length } : null;
    },
    (s) => {
      const m = /^\*\*([^*]+)\*\*/.exec(s);
      return m
        ? { node: { type: "bold", children: parseInline(m[1]!) }, consumed: m[0].length }
        : null;
    },
    (s) => {
      const m = /^__([^_]+)__/.exec(s);
      return m
        ? { node: { type: "bold", children: parseInline(m[1]!) }, consumed: m[0].length }
        : null;
    },
    (s) => {
      const m = /^\*([^*\s][^*]*?)\*/.exec(s);
      return m
        ? { node: { type: "italic", children: parseInline(m[1]!) }, consumed: m[0].length }
        : null;
    },
    (s) => {
      const m = /^_([^_\s][^_]*?)_/.exec(s);
      return m
        ? { node: { type: "italic", children: parseInline(m[1]!) }, consumed: m[0].length }
        : null;
    },
    (s) => {
      const m = /^\[\[([^\]]+)\]\]/.exec(s);
      return m
        ? { node: { type: "wikilink", target: m[1]! }, consumed: m[0].length }
        : null;
    },
    (s) => {
      const m = /^\[([^\]]+)\]\(([^)\s]+)\)/.exec(s);
      return m
        ? {
            node: { type: "link", href: m[2]!, children: parseInline(m[1]!) },
            consumed: m[0].length,
          }
        : null;
    },
  ];

  while (rest.length > 0) {
    let matched: { node: MdInline; consumed: number } | null = null;
    for (const fn of matchers) {
      matched = fn(rest);
      if (matched) break;
    }
    if (matched) {
      flush();
      out.push(matched.node);
      rest = rest.slice(matched.consumed);
    } else {
      buffer += rest[0]!;
      rest = rest.slice(1);
    }
  }
  flush();
  return out;
}
