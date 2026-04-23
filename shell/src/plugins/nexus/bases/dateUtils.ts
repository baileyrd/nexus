// Shared date helpers for the calendar / timeline views. All dates
// are kept as local-time `Date` objects; the `.bases` wire format is
// opaque strings, so we accept ISO-8601 (`yyyy-mm-dd`),
// `yyyy-mm-ddThh:mm`, and anything `Date` parses as a last resort.

export function parseDate(raw: unknown): Date | null {
  if (raw == null) return null
  if (raw instanceof Date) return Number.isNaN(raw.valueOf()) ? null : raw
  if (typeof raw === 'number') {
    const d = new Date(raw)
    return Number.isNaN(d.valueOf()) ? null : d
  }
  if (typeof raw !== 'string' || raw === '') return null
  const iso = /^(\d{4})-(\d{2})-(\d{2})$/.exec(raw)
  if (iso) {
    const [, y, m, d] = iso
    return new Date(Number(y), Number(m) - 1, Number(d))
  }
  const parsed = new Date(raw)
  return Number.isNaN(parsed.valueOf()) ? null : parsed
}

export function isoDate(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

export function yyyymm(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  return `${y}-${m}`
}

export function parseYyyymm(raw: string | null): Date {
  if (raw) {
    const m = /^(\d{4})-(\d{2})$/.exec(raw)
    if (m) return new Date(Number(m[1]), Number(m[2]) - 1, 1)
  }
  const now = new Date()
  return new Date(now.getFullYear(), now.getMonth(), 1)
}

export function addMonths(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth() + n, 1)
}

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate())
}

export function daysBetween(a: Date, b: Date): number {
  const ms = startOfDay(b).valueOf() - startOfDay(a).valueOf()
  return Math.round(ms / 86400000)
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n)
}

export const WEEKDAY_LABELS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
export const MONTH_LABELS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]
