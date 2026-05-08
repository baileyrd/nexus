// BL-069 — type-aware cell formatting for inline database-view blocks.
//
// `formatCell(value, fieldDef?)` resolves the human-readable string
// for one cell, respecting `BaseSchema.fields[name].type` when that
// metadata is present. Without a field def the helper falls back to
// the legacy "stringify-and-truncate" behaviour so widgets that
// haven't routed schema through (yet) don't regress.
//
// The mapping mirrors `nexus_types::bases::FieldType`:
//
// | FieldType           | Rendering                                       |
// |---------------------|-------------------------------------------------|
// | text / long-text    | the string verbatim                             |
// | url / email / uuid  | the string verbatim                             |
// | number              | locale-formatted with grouping                  |
// | currency            | locale-formatted with `currency` (USD default) |
// | percent             | `value*` already-applied; locale + `%` suffix   |
// | checkbox            | `✓` / empty                                     |
// | date                | locale `YYYY-MM-DD` (no time)                   |
// | time                | locale time-of-day                              |
// | datetime            | locale date+time                                |
// | select              | the chosen label (string)                       |
// | multi-select        | comma-separated labels                          |
// | relation / lookup   | best-effort label from `{ id, name? }` shape    |
// | formula / rollup    | passthrough (already string from server)        |
//
// Currency / percent assume the server has already normalised the
// value (records store cents-as-dollars or 0.7-as-70%). The shell
// doesn't second-guess the database engine.

/** Subset of `nexus_types::bases::FieldDefinition` we care about for
 *  rendering. We accept `unknown` from the schema (the wire type) and
 *  read fields defensively — schemas in the wild may omit `type`. */
export interface FieldDef {
  type?: string
  options?: Array<string | { id?: string; label?: string; name?: string }>
  /** Currency code for `currency` fields (default `USD`). */
  currency?: string
  /** Locale override (default `'en-US'`). */
  locale?: string
}

/** Look up a field definition from a `BaseSchema.fields` map. Returns
 *  `undefined` if the schema doesn't define `name`. */
export function lookupFieldDef(
  schemaFields: Record<string, unknown> | undefined,
  name: string,
): FieldDef | undefined {
  const raw = schemaFields?.[name]
  if (!raw || typeof raw !== 'object') return undefined
  return raw as FieldDef
}

/** Format a single cell. The `def` argument is optional: when absent
 *  the formatter falls back to the legacy stringify-and-truncate
 *  behaviour so widgets that haven't routed the schema through yet
 *  still render cleanly. */
export function formatCell(value: unknown, def?: FieldDef): string {
  if (value === null || value === undefined) return ''
  const type = def?.type
  switch (type) {
    case 'text':
    case 'long-text':
    case 'url':
    case 'email':
    case 'uuid':
    case 'formula':
    case 'rollup':
    case 'lookup':
      return typeof value === 'string' ? value : stringify(value)
    case 'number':
      return formatNumber(value, def)
    case 'currency':
      return formatCurrency(value, def)
    case 'percent':
      return formatPercent(value, def)
    case 'checkbox':
      return value === true ? '✓' : ''
    case 'date':
      return formatDate(value)
    case 'time':
      return formatTime(value)
    case 'datetime':
      return formatDateTime(value)
    case 'select':
      return formatSelect(value)
    case 'multi-select':
      return formatMultiSelect(value)
    case 'relation':
      return formatRelation(value)
    default:
      return primitiveOrStringify(value)
  }
}

/** Truncating stringify for unknown / non-primitive types. Caps the
 *  output at 200 chars so a runaway record doesn't blow up the cell. */
function stringify(value: unknown): string {
  try {
    const s = JSON.stringify(value)
    if (s == null) return ''
    return s.length > 200 ? `${s.slice(0, 197)}…` : s
  } catch {
    return ''
  }
}

function primitiveOrStringify(value: unknown): string {
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  return stringify(value)
}

function formatNumber(value: unknown, def?: FieldDef): string {
  const n = toNumber(value)
  if (n == null) return primitiveOrStringify(value)
  try {
    return new Intl.NumberFormat(def?.locale ?? 'en-US').format(n)
  } catch {
    return String(n)
  }
}

function formatCurrency(value: unknown, def?: FieldDef): string {
  const n = toNumber(value)
  if (n == null) return primitiveOrStringify(value)
  try {
    return new Intl.NumberFormat(def?.locale ?? 'en-US', {
      style: 'currency',
      currency: def?.currency ?? 'USD',
    }).format(n)
  } catch {
    return String(n)
  }
}

function formatPercent(value: unknown, def?: FieldDef): string {
  const n = toNumber(value)
  if (n == null) return primitiveOrStringify(value)
  // Records are stored as the displayable value (45 means 45%, not
  // 0.45). Avoid the `style: percent` formatter — it would multiply
  // by 100 and turn 45 into 4500%.
  try {
    return `${new Intl.NumberFormat(def?.locale ?? 'en-US').format(n)}%`
  } catch {
    return `${n}%`
  }
}

function formatDate(value: unknown): string {
  const d = toDate(value)
  if (!d) return primitiveOrStringify(value)
  // Render in `YYYY-MM-DD` to match how the server buckets calendar
  // groups; using `toLocaleDateString` would vary across locales.
  return d.toISOString().slice(0, 10)
}

function formatTime(value: unknown): string {
  const d = toDate(value)
  if (!d) {
    if (typeof value === 'string' && /^\d{2}:\d{2}/.test(value)) return value
    return primitiveOrStringify(value)
  }
  return d.toISOString().slice(11, 16)
}

function formatDateTime(value: unknown): string {
  const d = toDate(value)
  if (!d) return primitiveOrStringify(value)
  // ISO with the seconds trimmed; calendar / list views can re-parse
  // this back to a Date.
  return `${d.toISOString().slice(0, 10)} ${d.toISOString().slice(11, 16)}`
}

function formatSelect(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'object') {
    const obj = value as { label?: unknown; name?: unknown; id?: unknown }
    if (typeof obj.label === 'string') return obj.label
    if (typeof obj.name === 'string') return obj.name
    if (typeof obj.id === 'string') return obj.id
  }
  return primitiveOrStringify(value)
}

function formatMultiSelect(value: unknown): string {
  if (!Array.isArray(value)) return formatSelect(value)
  return value.map(formatSelect).filter((s) => s.length > 0).join(', ')
}

function formatRelation(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return value
  if (Array.isArray(value)) {
    return value.map(formatRelation).filter((s) => s.length > 0).join(', ')
  }
  if (typeof value === 'object') {
    const obj = value as { name?: unknown; label?: unknown; id?: unknown }
    if (typeof obj.name === 'string') return obj.name
    if (typeof obj.label === 'string') return obj.label
    if (typeof obj.id === 'string') return obj.id
  }
  return primitiveOrStringify(value)
}

function toNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string') {
    const n = Number(value)
    if (Number.isFinite(n)) return n
  }
  return null
}

function toDate(value: unknown): Date | null {
  if (value instanceof Date) return Number.isFinite(value.getTime()) ? value : null
  if (typeof value === 'number') {
    // Heuristic: > 1e12 means milliseconds, otherwise seconds.
    const ms = value > 1e12 ? value : value * 1000
    const d = new Date(ms)
    return Number.isFinite(d.getTime()) ? d : null
  }
  if (typeof value === 'string') {
    const d = new Date(value)
    return Number.isFinite(d.getTime()) ? d : null
  }
  return null
}
