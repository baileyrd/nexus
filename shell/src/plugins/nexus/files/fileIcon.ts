import type { IconName } from '../../../icons'

/**
 * BL-080: pick a file-tree glyph from a filename.
 *
 * The DoD calls out `.md`, `.rs`, `.ts`, `.py`, `.toml`, `.json`, and a
 * generic fallback. The implementation extends that with the cluster
 * of related extensions a user typically mixes in alongside (`.tsx`,
 * `.jsx`, `.go`, `.yaml`, …) so the icon set doesn't look randomly
 * sparse. Anything outside the table falls back to `doc`.
 *
 * Pure: no React, no globals — easy to unit-test against a list of
 * shapes.
 */
export function getFileIcon(name: string): IconName {
  const ext = extensionOf(name)
  switch (ext) {
    case 'md':
    case 'markdown':
      return 'book'

    case 'rs':
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
    case 'py':
    case 'go':
    case 'rb':
    case 'java':
    case 'kt':
    case 'swift':
    case 'cpp':
    case 'cc':
    case 'c':
    case 'h':
    case 'hpp':
    case 'cs':
      return 'fileCode'

    case 'json':
    case 'jsonc':
    case 'json5':
    case 'toml':
    case 'yaml':
    case 'yml':
      return 'fileJson'

    default:
      return 'doc'
  }
}

/**
 * Lower-cased extension (without the leading dot), or `''` for files
 * with no recognisable extension. Strips trailing query / hash
 * fragments defensively even though the file tree never sees them —
 * keeps the helper safe to reuse from other surfaces.
 */
function extensionOf(name: string): string {
  const trimmed = name.split(/[?#]/, 1)[0]
  const dot = trimmed.lastIndexOf('.')
  if (dot <= 0 || dot === trimmed.length - 1) return ''
  return trimmed.slice(dot + 1).toLowerCase()
}
