// Shared helpers for field-definition introspection. The schema's
// `fields` map on the wire is `Record<string, unknown>` because the
// kernel keeps definitions opaque; this module narrows that to the
// fields the table actually reads (type, options, required, primary).

/** PRD-10 / `nexus_types::bases::FieldType` values on the wire
 *  (kebab-case per `#[serde(rename_all = "kebab-case")]`). */
export type FieldKind =
  | 'text'
  | 'long-text'
  | 'number'
  | 'currency'
  | 'percent'
  | 'checkbox'
  | 'date'
  | 'time'
  | 'datetime'
  | 'select'
  | 'multi-select'
  | 'relation'
  | 'formula'
  | 'rollup'
  | 'lookup'
  | 'uuid'
  | 'url'
  | 'email'

export interface FieldDefinition {
  type: FieldKind
  required?: boolean
  primary?: boolean
  options?: string[]
  min?: number
  max?: number
  target?: string
  targetField?: string
  /** Formula expression — only set when `type === 'formula'`. The
   *  kernel evaluates this per record via `formula_eval`. */
  expression?: string
  /** Optional human-readable column header. Set when an Obsidian
   *  `.base` `properties:` entry declares `displayName`; otherwise
   *  the field name itself is used as the header. */
  displayName?: string
}

const ALL_KINDS: FieldKind[] = [
  'text',
  'long-text',
  'number',
  'currency',
  'percent',
  'checkbox',
  'date',
  'time',
  'datetime',
  'select',
  'multi-select',
  'relation',
  'formula',
  'rollup',
  'lookup',
  'uuid',
  'url',
  'email',
]
const KIND_SET = new Set<string>(ALL_KINDS)

export function parseFieldDef(def: unknown): FieldDefinition {
  if (!def || typeof def !== 'object') return { type: 'text' }
  const d = def as Record<string, unknown>
  const rawType = typeof d.type === 'string' ? d.type : 'text'
  const type = (KIND_SET.has(rawType) ? rawType : 'text') as FieldKind
  const out: FieldDefinition = { type }
  if (d.required === true) out.required = true
  if (d.primary === true) out.primary = true
  if (Array.isArray(d.options)) {
    out.options = (d.options as unknown[]).filter(
      (o): o is string => typeof o === 'string',
    )
  }
  if (typeof d.min === 'number') out.min = d.min
  if (typeof d.max === 'number') out.max = d.max
  if (typeof d.target === 'string') out.target = d.target
  if (typeof d.targetField === 'string') out.targetField = d.targetField
  if (typeof d.expression === 'string') out.expression = d.expression
  if (typeof d.displayName === 'string') out.displayName = d.displayName
  return out
}

/** Read-only cell types — edits are rejected regardless of user
 *  interaction. These are either server-populated (timestamps,
 *  user refs, uuid) or computed (formula/rollup/lookup) and must
 *  not be patched via `base_record_update`. `relation` is read-only
 *  until Phase-6 brings in a relation picker. */
export function isReadOnly(kind: FieldKind): boolean {
  switch (kind) {
    case 'uuid':
    case 'formula':
    case 'rollup':
    case 'lookup':
    case 'relation':
      return true
    default:
      return false
  }
}

export function typeGlyph(kind: FieldKind): string {
  switch (kind) {
    case 'text':
      return 'T'
    case 'long-text':
      return '¶'
    case 'number':
      return '#'
    case 'currency':
      return '$'
    case 'percent':
      return '%'
    case 'checkbox':
      return '☑'
    case 'date':
      return '📅'
    case 'time':
      return '⏱'
    case 'datetime':
      return '🕓'
    case 'select':
      return '◉'
    case 'multi-select':
      return '☰'
    case 'relation':
      return '↔'
    case 'formula':
      return 'ƒ'
    case 'rollup':
      return 'Σ'
    case 'lookup':
      return '⎋'
    case 'uuid':
      return '#'
    case 'url':
      return '🔗'
    case 'email':
      return '@'
  }
}

/** Default value for newly-created rows so required-field validation
 *  passes on the server. Null / empty string / false depending on
 *  type — the user immediately edits from there. */
export function defaultValueFor(kind: FieldKind): unknown {
  switch (kind) {
    case 'checkbox':
      return false
    case 'number':
    case 'currency':
    case 'percent':
      return 0
    case 'multi-select':
      return []
    default:
      return ''
  }
}

/** Stringify a value for read-only cell rendering. */
export function formatValue(kind: FieldKind, value: unknown): string {
  if (value == null) return ''
  switch (kind) {
    case 'checkbox':
      return value ? '✓' : ''
    case 'multi-select':
      if (Array.isArray(value)) return value.join(', ')
      return String(value)
    case 'percent':
      if (typeof value === 'number') return `${value}%`
      return String(value)
    case 'currency':
      if (typeof value === 'number') {
        return value.toLocaleString(undefined, {
          style: 'currency',
          currency: 'USD',
        })
      }
      return String(value)
    default:
      return typeof value === 'object' ? JSON.stringify(value) : String(value)
  }
}
