// Shared markdown → sanitized HTML helper. Extracted out of
// EditorView so other plugins (nexus.ai chat) can reuse the exact
// same pipeline + CSS (.nexus-markdown-body).
//
// marked.parse returns string when `async: false`. Sanitize before
// handing HTML to React's dangerouslySetInnerHTML — user notes
// aren't hostile, but DOMPurify is cheap insurance, and AI output
// is even less trustworthy.

import { marked } from 'marked'
import DOMPurify from 'dompurify'

export function renderMarkdown(content: string): string {
  const raw = marked.parse(content, { async: false }) as string
  return DOMPurify.sanitize(raw)
}
