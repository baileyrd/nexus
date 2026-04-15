/** A markdown heading with its source line index. */
export interface Heading {
  /** 1-6, from `#` to `######`. */
  level: number;
  /** Heading text with `#` markers stripped. */
  text: string;
  /** Zero-based line index in the source content. */
  line: number;
}

/**
 * Parse ATX-style markdown headings out of `content`. Lines inside
 * fenced code blocks (``` / ~~~) are skipped so code-fence
 * pseudo-headings don't appear in the outline. Setext headings
 * (underlined with = or -) are not supported yet.
 */
export function parseHeadings(content: string): Heading[] {
  const lines = content.split("\n");
  const headings: Heading[] = [];
  let fence: string | null = null;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trimStart();

    // Track fenced code blocks. Fence opens with ``` or ~~~ (any number
    // of additional chars) and closes on a matching bare fence.
    const fenceMatch = /^(```+|~~~+)/.exec(trimmed);
    if (fenceMatch) {
      if (fence === null) {
        fence = fenceMatch[1][0];
      } else if (trimmed.startsWith(fence)) {
        fence = null;
      }
      continue;
    }
    if (fence !== null) continue;

    const heading = /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(trimmed);
    if (heading) {
      headings.push({
        level: heading[1].length,
        text: heading[2],
        line: i,
      });
    }
  }

  return headings;
}
