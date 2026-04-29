// shell/src/plugins/nexus/recall/insertFormat.ts
//
// BL-044 — pure formatter that turns a `RecallMatch` into the markdown
// snippet inserted at the editor caret (or copied to the clipboard).
// Kept separate from the runtime so the formatting can be unit-tested
// without mocking the editor or the kernel.
//
// Output shape:
//
//   > {chunk_text trimmed, line-wrapped with `> ` prefix}
//   >
//   > — [[basename]]
//
// Where `basename` is the file's basename minus the `.md` suffix, so
// the resulting wiki-link resolves to the source note in the existing
// link-resolution rules.

import type { RecallMatch } from './recallStore'

/** Forward-slash basename of a forge-relative path. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** Strip a single trailing `.md` (case-insensitive) from a basename
 *  so the produced `[[wiki-link]]` matches Nexus's existing link
 *  resolver, which keys on the bare note title. */
function stripMdSuffix(name: string): string {
  return name.replace(/\.md$/i, '')
}

/**
 * Format the match as a markdown blockquote with an attribution
 * footer. Internal newlines in `chunk_text` are preserved and prefixed
 * with `> ` so the entire snippet stays inside a single quote block.
 */
export function formatRecallSnippet(match: RecallMatch): string {
  const trimmed = match.chunk_text.trim()
  const quoted = trimmed.length === 0
    ? '>'
    : trimmed.split('\n').map((line) => `> ${line}`).join('\n')
  const title = stripMdSuffix(basename(match.file_path))
  return `${quoted}\n>\n> — [[${title}]]\n`
}
