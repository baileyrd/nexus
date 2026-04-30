// BL-046 — code-aware capture support layered on top of BL-043.
//
// When the capture source is detectably code (file path with a code
// extension, IDE-supplied selection, syntax-highlightable content),
// the appended snippet preserves a fenced code block + path + line
// range so the captured note can be re-rendered with the same
// fidelity in the editor and so BL-044 recall can filter for
// project-scoped captures via the `#code/<language>` tag.
//
// All helpers here are pure — the React overlay and the kernel-IPC
// command handler both consume them, and the unit tests drive them
// without standing up the store / plugin runtime.

/** Inclusive 1-indexed line range copied from the source file. */
export interface CodeLineRange {
  start: number
  end: number
}

/** Provenance captured for a code source. Lives alongside the
 *  existing `CaptureSourceMeta` (zero-overhead when absent — the
 *  hotkey-driven text-capture path leaves this `undefined`). */
export interface CodeSourceMeta {
  /** Forge-relative or absolute path of the source file. Stored
   *  verbatim — no normalisation — so an IDE plugin can pass
   *  whatever the IDE reports without round-trip risk. */
  file: string
  /** Markdown fence info-string (e.g. `'rust'`, `'typescript'`,
   *  `'tsx'`). Resolved from the file extension via
   *  [`detectCodeLanguage`]; the caller may override when the
   *  source provides a more specific hint. */
  language: string
  /** Optional 1-indexed inclusive line range. Absent when the
   *  caller couldn't determine it (e.g. clipboard paste with no
   *  IDE context). */
  lineRange?: CodeLineRange
}

/** File-extension → language-tag map. The right-hand side is the
 *  fence info-string we emit; readers can be more lenient. Tags
 *  match the GitHub-flavoured Markdown convention so the captured
 *  note round-trips through any standard markdown renderer. Order
 *  inside the table is irrelevant — this is just a lookup. */
const LANGUAGE_BY_EXTENSION: Readonly<Record<string, string>> = Object.freeze({
  rs: 'rust',
  ts: 'typescript',
  tsx: 'tsx',
  js: 'javascript',
  jsx: 'jsx',
  mjs: 'javascript',
  cjs: 'javascript',
  py: 'python',
  rb: 'ruby',
  go: 'go',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  cc: 'cpp',
  cs: 'csharp',
  java: 'java',
  kt: 'kotlin',
  swift: 'swift',
  php: 'php',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  fish: 'fish',
  ps1: 'powershell',
  sql: 'sql',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'scss',
  less: 'less',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'toml',
  json: 'json',
  md: 'markdown',
  lua: 'lua',
  scala: 'scala',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  clj: 'clojure',
  hs: 'haskell',
  ml: 'ocaml',
  r: 'r',
  dart: 'dart',
  zig: 'zig',
  nix: 'nix',
  proto: 'protobuf',
  graphql: 'graphql',
  vue: 'vue',
  svelte: 'svelte',
})

/** Resolve a source file's language tag via its extension.
 *  Returns `null` for unknown / extensionless paths so callers can
 *  fall through to a plain (non-fenced) capture. The match is
 *  case-insensitive and ignores the leading dot. */
export function detectCodeLanguage(filePath: string | undefined | null): string | null {
  if (!filePath) return null
  // Strip query / fragment that some IDE protocols append.
  const clean = filePath.split(/[?#]/, 1)[0]
  const dot = clean.lastIndexOf('.')
  if (dot < 0 || dot === clean.length - 1) return null
  const ext = clean.slice(dot + 1).toLowerCase()
  return LANGUAGE_BY_EXTENSION[ext] ?? null
}

/** Build the body lines specific to a code capture — slotted into
 *  `buildSnippet` between the existing `Source: <app>` line and
 *  the user's draft. Emits, in order:
 *
 *    1. `File: <path>` line
 *    2. `Lines: L<start>-L<end>` line (when `lineRange` is set)
 *    3. blank line
 *    4. fenced code block with the language tag and the draft body
 *
 *  Plus a trailing `#code/<language>` tag so BL-044 recall can
 *  filter "from project" by matching the tag prefix. The tag is
 *  added on its own line after the fence so a downstream parser
 *  treats it as a tag mention rather than fence body. */
export function buildCodeSnippetSection(
  draft: string,
  code: CodeSourceMeta,
): string[] {
  const lines: string[] = []
  lines.push(`File: ${code.file}`)
  if (code.lineRange) {
    lines.push(`Lines: L${code.lineRange.start}-L${code.lineRange.end}`)
  }
  lines.push('')
  // Defensively fence with extra backticks if the draft itself
  // contains a triple-backtick run, matching the GFM convention.
  const fence = draft.includes('```') ? '````' : '```'
  lines.push(`${fence}${code.language}`)
  lines.push(draft.replace(/\n+$/u, ''))
  lines.push(fence)
  lines.push('')
  lines.push(`#code/${code.language}`)
  return lines
}
