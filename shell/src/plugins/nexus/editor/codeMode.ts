// shell/src/plugins/nexus/editor/codeMode.ts
//
// BL-075 — dual-mode editor (document vs. code).
//
// The editor handles two file modes:
//
// - **Document mode**: `.md`, `.markdown` — opened through the
//   `com.nexus.editor` block tree (Phase 3+ session lifecycle).
//   Renders with the WYSIWYG / live-preview / source toggle and the
//   block-handle extensions.
// - **Code mode**: `.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`,
//   `.json`, `.yaml`, `.yml`, `.toml`, `.go` — opened directly via
//   `com.nexus.storage::read_file` and saved with `write_file`. The
//   editor mounts a bare CodeMirror with the right language
//   extension; no block tree, no slash menu, no live-preview
//   decorations.
//
// The mode is picked at file-open time by [`getEditorMode`]; the
// matching CodeMirror language extension is constructed by
// [`pickLanguageExtension`]. Both functions are pure — the editor
// store keeps no copy of the mode, instead deriving it from the
// tab's `name` whenever it's needed. That keeps tab persistence
// (Phase 5 layout snapshots) backward-compatible: an old shell
// state file without the field still works after a rebuild.
//
// Settings (`nexus.editor.codeFileExtensions`) lets the user widen
// the code-mode set; ASKING the user "which extensions" was
// preferred over a per-language setting because the LSP track
// (BL-076 / BL-077) will key off the same list.

import type { Extension } from '@codemirror/state'
import { json } from '@codemirror/lang-json'
import { javascript } from '@codemirror/lang-javascript'
import { python } from '@codemirror/lang-python'
import { rust } from '@codemirror/lang-rust'
import { yaml } from '@codemirror/lang-yaml'
import { StreamLanguage } from '@codemirror/language'
import { go } from '@codemirror/legacy-modes/mode/go'
import { toml } from '@codemirror/legacy-modes/mode/toml'

/** The two routing buckets BL-075 introduces. */
export type EditorMode = 'document' | 'code'

/**
 * Default extensions routed through code mode. Curated set — the
 * languages Nexus's own codebase uses, plus the web-tier classics.
 * Users widen the list via the `nexus.editor.codeFileExtensions`
 * config; that override is layered on top of this default at
 * runtime.
 *
 * Markdown is intentionally absent: `.md` files belong in document
 * mode where they get the block tree + live preview.
 */
export const DEFAULT_CODE_EXTENSIONS: readonly string[] = [
  'rs',
  'ts',
  'tsx',
  'js',
  'jsx',
  'mjs',
  'cjs',
  'py',
  'go',
  'json',
  'jsonc',
  'yaml',
  'yml',
  'toml',
] as const

/**
 * Extract the lowercased file extension from a name. Handles
 * `.tar.gz`-style multi-dot names by returning only the final
 * segment after the last dot — `getExtension('a.b.rs')` → `'rs'`.
 * Names with no dot return `''`.
 */
export function getExtension(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return ''
  const dot = trimmed.lastIndexOf('.')
  if (dot < 0 || dot === trimmed.length - 1) return ''
  return trimmed.slice(dot + 1).toLowerCase()
}

/**
 * Pick the routing bucket for `name`. Markdown wins regardless of
 * the code-extension list (a user-configured list can't accidentally
 * disable document mode). Anything in `codeExtensions` (default or
 * user override) gets `'code'`. Everything else falls back to
 * `'document'` — the legacy storage-read path, which renders as
 * `<pre>` in preview mode and a bare CM6 in source mode.
 *
 * The fall-through deliberately picks `'document'`, not `'code'`:
 * unrecognised extensions (or no extension at all) historically
 * opened in the markdown view; flipping that to code mode would
 * break behaviour for plain-text scratch files like `LICENSE`,
 * `CHANGELOG`, etc.
 */
export function getEditorMode(
  name: string,
  codeExtensions: readonly string[] = DEFAULT_CODE_EXTENSIONS,
): EditorMode {
  const ext = getExtension(name)
  if (ext === 'md' || ext === 'markdown' || ext === 'mdx') return 'document'
  if (codeExtensions.includes(ext)) return 'code'
  return 'document'
}

/**
 * Return the CodeMirror language extension for `name`, or `null`
 * when no built-in matches. Code mode mounts the returned extension
 * unchanged; passing `null` produces a bare-text CM6 buffer (still
 * useful for `.txt`, `.log`, etc.).
 *
 * Mapping notes:
 * - TypeScript shares the JavaScript package's grammar with the
 *   `typescript: true` flag. JSX / TSX share too.
 * - Go and TOML come from `@codemirror/legacy-modes` because the
 *   first-party packages don't cover them. `StreamLanguage.define`
 *   wraps the legacy mode so it slots into the modern extension
 *   system.
 * - YAML: the first-party `@codemirror/lang-yaml` package shipped
 *   in mid-2024; we depend on a recent enough version that it's
 *   stable.
 */
/**
 * BL-139 — language hint passed to the AI predict handler. Distinct
 * from `pickLanguageExtension` because the AI side wants a stable
 * name regardless of which CM language extension we mounted (so
 * `.tsx` reports `"typescript"`, not `"javascript-jsx"`). Markdown
 * is intentionally included — predictions in document-mode markdown
 * tabs are part of the BL-139 scope.
 */
export function languageHintFor(name: string): string {
  const ext = getExtension(name)
  switch (ext) {
    case 'rs':
      return 'rust'
    case 'ts':
    case 'tsx':
      return 'typescript'
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
      return 'javascript'
    case 'py':
      return 'python'
    case 'go':
      return 'go'
    case 'json':
    case 'jsonc':
      return 'json'
    case 'yaml':
    case 'yml':
      return 'yaml'
    case 'toml':
      return 'toml'
    case 'md':
    case 'markdown':
      return 'markdown'
    default:
      return ext || 'plaintext'
  }
}

export function pickLanguageExtension(name: string): Extension | null {
  const ext = getExtension(name)
  switch (ext) {
    case 'rs':
      return rust()
    case 'ts':
    case 'tsx':
      return javascript({ typescript: true, jsx: ext === 'tsx' })
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
      return javascript({ jsx: ext === 'jsx' })
    case 'py':
      return python()
    case 'json':
    case 'jsonc':
      return json()
    case 'yaml':
    case 'yml':
      return yaml()
    case 'toml':
      return StreamLanguage.define(toml)
    case 'go':
      return StreamLanguage.define(go)
    default:
      return null
  }
}
