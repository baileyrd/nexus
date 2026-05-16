// BL-142 Phase 2a — pure helpers for the `nexus.editor.replKernels`
// configuration. The setting is stored as a JSON-encoded map from
// language tag (`"python"`, `"node"`, …) to a kernel command string
// (`"python3 -i"`, `"node --interactive"`). Two transformations
// live here, both pure so they're trivially testable:
//
// 1. `parseReplKernelsConfig(jsonStr)` — parse the JSON blob, drop
//    malformed entries, return a typed `Record<string, string>`.
// 2. `splitKernelCommand(cmd)` — tokenize a kernel command string
//    into `{ program, args[] }` so the editor REPL plugin can pass
//    it to `replClient.start(...)`. Handles double-quoted args so
//    `python3 -c "print(2)"` doesn't split the print arg.
//
// The default value is `{}` (opt-in) — the REPL gutter / Shift-Enter
// surfaces remain inert until the user configures at least one
// kernel. This matches the BL-142 DoD's "opt-in" posture.

/** JSON-encoded form stored under the `nexus.editor.replKernels` key. */
export const CONFIG_REPL_KERNELS = 'nexus.editor.replKernels'

/** Default JSON value — opt-in: no kernels until the user adds one. */
export const REPL_KERNELS_DEFAULT_JSON = '{}'

/** Resolved kernel-command shape ready for `ReplClient.start`. */
export interface KernelCommand {
  /** Program name (first token in the command string). */
  program: string
  /** Args after the program. */
  args: string[]
}

/**
 * Parse the JSON-encoded `replKernels` config value. Malformed JSON
 * resolves to an empty map (matches the "opt-in / fail-safe"
 * posture — a broken config shouldn't surface as a confusing
 * runtime error every time the user opens a REPL-flagged code block).
 * Non-string values inside the map are dropped silently for the same
 * reason.
 */
export function parseReplKernelsConfig(json: string): Record<string, string> {
  let parsed: unknown
  try {
    parsed = JSON.parse(json)
  } catch {
    return {}
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return {}
  }
  const out: Record<string, string> = {}
  for (const [lang, cmd] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof lang === 'string' && lang.length > 0 && typeof cmd === 'string' && cmd.trim().length > 0) {
      out[lang] = cmd
    }
  }
  return out
}

/**
 * Tokenize a kernel command string into `{ program, args }`.
 *
 * Recognizes double-quoted segments as single tokens so callers can
 * supply `python3 -c "print(2)"` without the print arg being split.
 * Single quotes are NOT special — they get treated as part of the
 * token, matching how `bash -c` would see them after shell parsing.
 *
 * Returns `null` when the input is empty / whitespace-only. The
 * caller can treat `null` as "no kernel configured for this lang"
 * and surface a clear "configure a REPL kernel for `<lang>` in
 * Settings → Editor" message rather than spawning an empty command.
 */
export function splitKernelCommand(raw: string): KernelCommand | null {
  const trimmed = raw.trim()
  if (trimmed.length === 0) return null
  const tokens: string[] = []
  let buf = ''
  let inQuotes = false
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i]
    if (ch === '"') {
      inQuotes = !inQuotes
      continue
    }
    if (!inQuotes && (ch === ' ' || ch === '\t')) {
      if (buf.length > 0) {
        tokens.push(buf)
        buf = ''
      }
      continue
    }
    buf += ch
  }
  if (buf.length > 0) tokens.push(buf)
  if (tokens.length === 0) return null
  const [program, ...args] = tokens
  return { program, args }
}

/**
 * One-shot resolver: look up `lang` in the parsed kernels map and
 * return the tokenized command, or `null` if either the lookup or
 * the tokenization fails. Exists so callers (the editor REPL
 * plugin's Run action) don't have to chain `parseReplKernelsConfig`
 * → lookup → `splitKernelCommand` themselves on every dispatch.
 */
export function resolveKernelForLang(
  kernelsJson: string,
  lang: string,
): KernelCommand | null {
  const kernels = parseReplKernelsConfig(kernelsJson)
  const cmd = kernels[lang]
  if (!cmd) return null
  return splitKernelCommand(cmd)
}
