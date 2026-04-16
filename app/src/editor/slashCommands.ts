/**
 * Slash command catalog for the markdown editor.
 *
 * Each entry describes a block-insert command surfaced by the `/` trigger
 * overlay in `EditorSurface`. Templates use a `|` character to mark where
 * the cursor should land after insertion.
 */

export interface SlashCommand {
  /** Stable identifier. */
  id: string;
  /** Primary display label. */
  label: string;
  /** Additional keywords for fuzzy search. */
  aliases?: string[];
  /** Short dimmed description. */
  description: string;
  /**
   * Markdown template to insert, replacing the `/query` trigger text.
   * The first `\0` (NUL) in the string marks the final cursor position.
   * NUL is used instead of a printable marker so templates containing
   * pipes, hashes, or other markdown syntax aren't misinterpreted.
   */
  template: string;
  /** Short text badge rendered in the popup (e.g. "H1", "#"). */
  badge: string;
}

export const SLASH_COMMANDS: SlashCommand[] = [
  {
    id: "heading-1",
    label: "Heading 1",
    aliases: ["h1", "title"],
    description: "Largest section heading",
    template: "# \0",
    badge: "H1",
  },
  {
    id: "heading-2",
    label: "Heading 2",
    aliases: ["h2"],
    description: "Medium section heading",
    template: "## \0",
    badge: "H2",
  },
  {
    id: "heading-3",
    label: "Heading 3",
    aliases: ["h3"],
    description: "Small section heading",
    template: "### \0",
    badge: "H3",
  },
  {
    id: "bullet-list",
    label: "Bullet list",
    aliases: ["ul", "unordered"],
    description: "Create an unordered list",
    template: "- \0",
    badge: "•",
  },
  {
    id: "numbered-list",
    label: "Numbered list",
    aliases: ["ol", "ordered"],
    description: "Create an ordered list",
    template: "1. \0",
    badge: "1.",
  },
  {
    id: "task-list",
    label: "Task list",
    aliases: ["todo", "checklist", "check"],
    description: "Create a checkbox task list",
    template: "- [ ] \0",
    badge: "☐",
  },
  {
    id: "code-block",
    label: "Code block",
    aliases: ["pre", "fence"],
    description: "Fenced code block",
    template: "```\n\0\n```",
    badge: "</>",
  },
  {
    id: "blockquote",
    label: "Blockquote",
    aliases: ["quote"],
    description: "Indented quote block",
    template: "> \0",
    badge: "❝",
  },
  {
    id: "horizontal-rule",
    label: "Horizontal rule",
    aliases: ["hr", "divider", "separator"],
    description: "Horizontal divider line",
    template: "---\n\0",
    badge: "—",
  },
  {
    id: "table",
    label: "Table",
    aliases: ["grid"],
    description: "3×3 markdown table",
    template:
      "| \0 | Column 2 | Column 3 |\n| --- | --- | --- |\n|     |          |          |\n|     |          |          |",
    badge: "⊞",
  },
];
