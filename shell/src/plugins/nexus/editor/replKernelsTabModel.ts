// BL-142 Phase 3 — pure factor for the Settings → REPL Kernels tab.
//
// The persisted shape stays unchanged: the setting at
// `nexus.editor.replKernels` is a JSON-encoded map from language tag
// to a kernel command string. The Phase 3 tab presents that map as
// an editable list of `(lang, command)` rows; serialising back is
// the inverse of `parseReplKernelsConfig`.
//
// Two pure factors live here so the tab component is mostly a thin
// shell over them:
//
//   1. `rowsFromJson(json)` — JSON → array of rows preserving the
//      stored order so existing kernels don't shuffle on every
//      open. Malformed JSON resolves to `[]` (same fail-safe
//      posture `parseReplKernelsConfig` already has).
//   2. `jsonFromRows(rows)` — array of rows → JSON, dropping blank
//      rows (a partly-typed Add or a row the user blanked out).
//      Duplicate lang keys: last one wins, mirroring how JSON
//      objects collapse repeated keys.
//
// The tab also needs a per-row validity check so the Save button
// can disable while the user is mid-typing — see `rowIsValid` /
// `rowsAreSavable` at the bottom.

/** One editable row in the Settings tab. `lang` and `command` are
 *  the trimmed-on-save shape; while editing either may be empty
 *  or whitespace. */
export interface KernelRow {
  /** Stable identity for React's key prop. Generated client-side;
   *  not persisted. */
  id: string
  /** Language tag (the JSON object key, e.g. "python"). */
  lang: string
  /** Kernel command string (the JSON object value, e.g. "python3 -i"). */
  command: string
}

/**
 * Parse the persisted JSON blob into editable rows. Order is the
 * `Object.entries` order — V8 / SpiderMonkey preserve insertion
 * order for string keys, so editing a kernel in the tab and saving
 * round-trips its position. Malformed input yields `[]`.
 */
export function rowsFromJson(json: string): KernelRow[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(json)
  } catch {
    return []
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return []
  }
  const out: KernelRow[] = []
  let counter = 0
  for (const [lang, cmd] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof lang !== 'string' || lang.length === 0) continue
    if (typeof cmd !== 'string') continue
    out.push({
      id: `row-${counter++}`,
      lang,
      command: cmd,
    })
  }
  return out
}

/**
 * Serialise the editable rows back into the persisted JSON shape.
 * Rules that match `parseReplKernelsConfig`'s acceptance set:
 *   - Trim both `lang` and `command` before validation.
 *   - Drop rows whose `lang` is empty after trimming (a half-typed
 *     Add).
 *   - Drop rows whose `command` is empty after trimming (the user
 *     hit Remove via blank-out).
 *   - Later occurrences of the same `lang` overwrite earlier ones
 *     (mirrors `JSON.parse` of an object literal with repeated keys).
 *
 * The result is pretty-printed with `2`-space indent so a curious
 * user inspecting `app.toml` sees readable JSON; `parseReplKernelsConfig`
 * accepts either form.
 */
export function jsonFromRows(rows: ReadonlyArray<KernelRow>): string {
  const map: Record<string, string> = {}
  for (const row of rows) {
    const lang = row.lang.trim()
    const command = row.command.trim()
    if (lang.length === 0 || command.length === 0) continue
    map[lang] = command
  }
  return JSON.stringify(map, null, 2)
}

/**
 * True when a row has both fields populated after trimming. Used by
 * the tab to render a per-row validity hint and to gate the Save
 * button on at least one fully-filled row.
 */
export function rowIsValid(row: KernelRow): boolean {
  return row.lang.trim().length > 0 && row.command.trim().length > 0
}

/**
 * True when the row set has at least one valid row and no row with
 * a duplicate trimmed `lang` (after dropping blanks). Duplicates
 * would silently overwrite on save — surfacing this in the tab UI
 * is friendlier than letting Save eat a user's input.
 */
export function rowsAreSavable(rows: ReadonlyArray<KernelRow>): boolean {
  const seen = new Set<string>()
  let valid = 0
  for (const row of rows) {
    if (!rowIsValid(row)) continue
    const lang = row.lang.trim()
    if (seen.has(lang)) return false
    seen.add(lang)
    valid++
  }
  return valid > 0
}

/** Build a blank row with a fresh id. */
let __idCounter = 0
export function blankRow(): KernelRow {
  return { id: `new-${++__idCounter}`, lang: '', command: '' }
}

/** Test-only — reset the id counter so snapshot-style assertions
 *  on `blankRow().id` are stable across test cases. */
export function _resetBlankRowIdCounterForTests(): void {
  __idCounter = 0
}
