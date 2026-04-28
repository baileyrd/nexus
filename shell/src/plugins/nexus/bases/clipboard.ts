// BL-031 — clipboard helpers for the bases table.
//
// Two payload shapes ride alongside `text/plain` TSV so paste can
// preserve typing when the source and destination are both Nexus
// bases. External-app paste falls through to the TSV.
//
//   `application/x-nexus-bases-cells` — rectangular cell range
//   `application/x-nexus-bases-rows`  — whole records (with their id)
//
// The functions here are intentionally pure; the table component owns
// selection state and the kernel calls. That keeps this module easy to
// unit-test (no React, no PluginAPI) and lets the same helpers feed
// future surfaces (e.g. a board / list paste path).
//
// Cross-base coercion: the wire format records the source field
// `type` per column, so the destination side can call `coerceValue`
// on a per-cell basis. Anything that can't coerce becomes `null` and
// trips the `coerced > 0` flag — the table surfaces that as a
// `bases:paste-coerced` notification so the user knows some cells
// dropped values.

import type { BaseRecord } from './kernelClient'
import type { FieldDefinition, FieldKind } from './fieldTypes'

export const CELLS_MIME = 'application/x-nexus-bases-cells'
export const ROWS_MIME = 'application/x-nexus-bases-rows'

/** A normalized rectangular range over a table view. The four
 *  indices are inclusive and 0-based; `r1 <= r2` and `c1 <= c2`. */
export interface CellRange {
  r1: number
  c1: number
  r2: number
  c2: number
}

export interface CellsPayload {
  kind: 'cells'
  /** Source field name + type per column. The destination side
   *  matches by name first, then falls back to position so a paste
   *  into a column with the same display position still works. */
  fields: { name: string; type: FieldKind }[]
  /** rows[r][c] — the value the source cell held at copy time. */
  rows: unknown[][]
}

export interface RowsPayload {
  kind: 'rows'
  /** Field name + type per source column. */
  fields: { name: string; type: FieldKind }[]
  /** Per-record map of `{ field → value }`. The source `id` is
   *  preserved so tests can assert on it but isn't honoured by the
   *  paste path — the destination kernel mints fresh ids. */
  records: BaseRecord[]
}

export type ClipPayload = CellsPayload | RowsPayload

/** Normalize an anchor + focus pair (which can be in any direction)
 *  into a `r1 <= r2, c1 <= c2` rectangle. */
export function normalizeRange(
  anchor: { row: number; col: number },
  focus: { row: number; col: number },
): CellRange {
  return {
    r1: Math.min(anchor.row, focus.row),
    c1: Math.min(anchor.col, focus.col),
    r2: Math.max(anchor.row, focus.row),
    c2: Math.max(anchor.col, focus.col),
  }
}

export function rangeContains(range: CellRange, row: number, col: number): boolean {
  return row >= range.r1 && row <= range.r2 && col >= range.c1 && col <= range.c2
}

/** Serialize a value to its TSV cell representation. Tabs and
 *  newlines inside the value would corrupt the row-major TSV grid;
 *  they're swapped for spaces. Multi-select is joined on `, ` so the
 *  external app sees a stable string. */
export function cellToTsv(kind: FieldKind, value: unknown): string {
  if (value == null) return ''
  if (kind === 'multi-select' && Array.isArray(value)) {
    return value.map((v) => String(v)).join(', ').replace(/[\t\r\n]/g, ' ')
  }
  if (kind === 'checkbox') {
    return value ? 'true' : 'false'
  }
  if (typeof value === 'object') {
    return JSON.stringify(value).replace(/[\t\r\n]/g, ' ')
  }
  return String(value).replace(/[\t\r\n]/g, ' ')
}

/** Render a `cells[][]` matrix to TSV. No header row — cell-range
 *  copy is positional, not column-named. */
export function cellsToTsv(
  cells: unknown[][],
  fields: { name: string; type: FieldKind }[],
): string {
  return cells
    .map((row) =>
      row.map((v, c) => cellToTsv(fields[c]?.type ?? 'text', v)).join('\t'),
    )
    .join('\n')
}

/** Render a record list to TSV with a header row — used for whole-row
 *  copy and the no-selection paste path. */
export function rowsToTsv(
  records: BaseRecord[],
  fields: { name: string; type: FieldKind }[],
): string {
  const header = fields.map((f) => f.name).join('\t')
  const body = records
    .map((r) =>
      fields.map((f) => cellToTsv(f.type, r[f.name])).join('\t'),
    )
    .join('\n')
  return body ? `${header}\n${body}` : header
}

/** Parse TSV from an external app. Returns a `cells[][]` matrix —
 *  the caller decides whether to treat it as cells or rows based on
 *  selection state. Empty trailing line is dropped. */
export function parseTsv(text: string): string[][] {
  const trimmed = text.replace(/\r\n/g, '\n').replace(/\n$/, '')
  if (!trimmed) return []
  return trimmed.split('\n').map((line) => line.split('\t'))
}

/** Best-effort coercion of a raw value into the destination column's
 *  type. Returns `[value, coerced]` — `coerced` is true when the
 *  source value had to change shape (e.g. string → number, number →
 *  string). Truly incompatible inputs return `[null, true]`.
 *
 *  Read-only types (formula/rollup/lookup/uuid/relation) reject
 *  any paste — the caller skips those columns entirely. */
export function coerceValue(
  destKind: FieldKind,
  value: unknown,
  sourceKind?: FieldKind,
): [unknown, boolean] {
  if (value == null || value === '') {
    return [destKind === 'multi-select' ? [] : destKind === 'checkbox' ? false : null, false]
  }
  // Same-type pass-through is the happy path — no coercion flag.
  if (sourceKind && sourceKind === destKind) {
    return [value, false]
  }
  switch (destKind) {
    case 'number':
    case 'currency':
    case 'percent': {
      if (typeof value === 'number' && Number.isFinite(value)) {
        return [value, sourceKind !== undefined && sourceKind !== destKind]
      }
      const n = Number(typeof value === 'string' ? value.replace(/[,%$\s]/g, '') : value)
      if (Number.isFinite(n)) return [n, true]
      return [null, true]
    }
    case 'checkbox': {
      if (typeof value === 'boolean') return [value, sourceKind !== undefined && sourceKind !== destKind]
      const s = String(value).trim().toLowerCase()
      if (s === 'true' || s === '1' || s === 'yes' || s === '✓') return [true, true]
      if (s === 'false' || s === '0' || s === 'no' || s === '') return [false, true]
      return [null, true]
    }
    case 'multi-select': {
      if (Array.isArray(value)) {
        return [value.map((v) => String(v)), sourceKind !== undefined && sourceKind !== destKind]
      }
      const s = String(value)
      if (!s) return [[], false]
      return [s.split(/,\s*/).filter((p) => p.length > 0), true]
    }
    case 'select': {
      return [String(value), sourceKind !== undefined && sourceKind !== destKind]
    }
    case 'date':
    case 'time':
    case 'datetime':
    case 'url':
    case 'email':
    case 'long-text':
    case 'text': {
      // Stringify; multi-select arrays become comma-joined.
      if (Array.isArray(value)) {
        return [value.map((v) => String(v)).join(', '), true]
      }
      if (typeof value === 'object') {
        return [JSON.stringify(value), true]
      }
      return [String(value), sourceKind !== undefined && sourceKind !== destKind]
    }
    default:
      // Read-only / unsupported destination — the caller filters
      // these out before reaching us, but be defensive.
      return [null, true]
  }
}

/** True when the destination column accepts paste at all. */
export function isPasteable(def: FieldDefinition): boolean {
  switch (def.type) {
    case 'uuid':
    case 'formula':
    case 'rollup':
    case 'lookup':
    case 'relation':
      return false
    default:
      return true
  }
}

/** A typed-payload-aware clipboard read. Tries the JSON MIMEs first;
 *  falls back to plain text. The `multi-mime` ClipboardItem path is
 *  optional — older browsers / the Tauri webview without the
 *  permission can still TSV-paste from the text/plain channel. */
export async function readClipboardPayload(): Promise<{
  payload: ClipPayload | null
  text: string
}> {
  if (typeof navigator === 'undefined' || !navigator.clipboard) {
    return { payload: null, text: '' }
  }
  // Prefer ClipboardItem.read so we can pull the typed JSON; fall
  // through to readText if the API isn't there or rejects.
  const clipboard = navigator.clipboard as Clipboard & {
    read?: () => Promise<ClipboardItems>
  }
  if (typeof clipboard.read === 'function') {
    try {
      const items = await clipboard.read()
      for (const item of items) {
        for (const type of [CELLS_MIME, ROWS_MIME]) {
          if (item.types.includes(type)) {
            const blob = await item.getType(type)
            const json = await blob.text()
            const payload = parsePayload(json)
            if (payload) {
              const tsv = item.types.includes('text/plain')
                ? await (await item.getType('text/plain')).text()
                : ''
              return { payload, text: tsv }
            }
          }
        }
        if (item.types.includes('text/plain')) {
          const blob = await item.getType('text/plain')
          return { payload: null, text: await blob.text() }
        }
      }
    } catch {
      // Fall through to readText.
    }
  }
  try {
    const text = await navigator.clipboard.readText()
    return { payload: null, text }
  } catch {
    return { payload: null, text: '' }
  }
}

/** Best-effort write of TSV + JSON payload to the clipboard. The
 *  ClipboardItem multi-MIME write requires a secure context and the
 *  browser to support `application/*` types. We try it first; if it
 *  rejects or `ClipboardItem` isn't defined, fall through to a
 *  plain-text write so external apps still see the TSV. */
export async function writeClipboardPayload(
  payload: ClipPayload,
  tsv: string,
): Promise<void> {
  if (typeof navigator === 'undefined' || !navigator.clipboard) return
  const json = JSON.stringify(payload)
  const mime = payload.kind === 'cells' ? CELLS_MIME : ROWS_MIME
  const ClipboardItemCtor = (globalThis as unknown as { ClipboardItem?: typeof ClipboardItem })
    .ClipboardItem
  const writeFn = (navigator.clipboard as Clipboard & {
    write?: (items: ClipboardItems) => Promise<void>
  }).write
  if (ClipboardItemCtor && typeof writeFn === 'function') {
    try {
      const item = new ClipboardItemCtor({
        [mime]: new Blob([json], { type: mime }),
        'text/plain': new Blob([tsv], { type: 'text/plain' }),
      })
      await writeFn.call(navigator.clipboard, [item])
      return
    } catch {
      // Fall through.
    }
  }
  await navigator.clipboard.writeText(tsv)
}

function parsePayload(json: string): ClipPayload | null {
  try {
    const obj = JSON.parse(json) as unknown
    if (!obj || typeof obj !== 'object') return null
    const kind = (obj as { kind?: unknown }).kind
    if (kind === 'cells') {
      const fields = (obj as { fields?: unknown }).fields
      const rows = (obj as { rows?: unknown }).rows
      if (!Array.isArray(fields) || !Array.isArray(rows)) return null
      return {
        kind: 'cells',
        fields: fields.filter(
          (f): f is { name: string; type: FieldKind } =>
            !!f && typeof f === 'object' && typeof (f as { name?: unknown }).name === 'string' &&
            typeof (f as { type?: unknown }).type === 'string',
        ),
        rows: (rows as unknown[]).map((r) => (Array.isArray(r) ? (r as unknown[]) : [])),
      }
    }
    if (kind === 'rows') {
      const fields = (obj as { fields?: unknown }).fields
      const records = (obj as { records?: unknown }).records
      if (!Array.isArray(fields) || !Array.isArray(records)) return null
      return {
        kind: 'rows',
        fields: fields.filter(
          (f): f is { name: string; type: FieldKind } =>
            !!f && typeof f === 'object' && typeof (f as { name?: unknown }).name === 'string' &&
            typeof (f as { type?: unknown }).type === 'string',
        ),
        records: (records as unknown[]).filter(
          (r): r is BaseRecord =>
            !!r && typeof r === 'object' && typeof (r as { id?: unknown }).id === 'string',
        ),
      }
    }
    return null
  } catch {
    return null
  }
}
