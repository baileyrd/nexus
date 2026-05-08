// BL-053 Phase 4 — shared status-pill / status-dot rendering.
//
// Two render variants: the inline pill (label + dot) used in the
// frontmatter metadata bar, and the dot-only variant used in the
// file-tree row at the right edge.
//
// The known status set mirrors the BL-053 Phase 3 callout palette
// (`info` / `warn` / `risk` / `ok`) so callouts and pills share the
// same color tokens. Unknown values render with a neutral
// `--text-faint` accent and the raw label text — graceful
// degradation rather than crashing on `status: foo`.

import type { CSSProperties, ReactElement } from 'react'

/** Canonical status values that get themed coloring. Anything else
 *  renders with the neutral fallback. */
export type KnownStatus = 'info' | 'warn' | 'risk' | 'ok'

const KNOWN_STATUSES: ReadonlySet<string> = new Set<string>([
  'info',
  'warn',
  'risk',
  'ok',
])

/** True when `s` is one of the four canonical status values; pure so
 *  unit tests can pin the membership without instantiating the React
 *  tree. */
export function isKnownStatus(s: string | null | undefined): s is KnownStatus {
  return s != null && KNOWN_STATUSES.has(s)
}

/** Map a status label → CSS variable name for its accent color.
 *  Mirrors the per-type variables baked into the callout CSS so a
 *  status pill rendered on a page with a callout of the same type
 *  shows the same color. */
export function statusAccentVar(status: string | null | undefined): string {
  switch (status) {
    case 'info':
      return '--cool'
    case 'warn':
      return '--warn'
    case 'risk':
      return '--risk'
    case 'ok':
      return '--ok'
    default:
      return '--text-faint'
  }
}

interface PillProps {
  /** The raw frontmatter / inline value. Passed through verbatim as
   *  the pill's text; the dot color is themed only when the value is
   *  in the known set. */
  status: string
  /** Optional override for the title attribute (hover tooltip).
   *  Defaults to `Status: <value>`. */
  title?: string
}

/** Inline pill — colored dot + label. Used in the document
 *  frontmatter metadata bar. */
export function StatusPill({ status, title }: PillProps): ReactElement {
  const accent = `var(${statusAccentVar(status)})`
  const tooltip = title ?? `Status: ${status}`
  return (
    <span
      className="nx-status-pill"
      title={tooltip}
      data-status={isKnownStatus(status) ? status : 'other'}
      style={pillStyle}
    >
      <span
        aria-hidden
        className="nx-status-dot"
        style={{ ...dotStyle, background: accent }}
      />
      <span className="nx-status-pill-label">{status}</span>
    </span>
  )
}

interface DotProps {
  status: string
  /** Optional aria-label override — defaults to `status: <value>`. */
  ariaLabel?: string
}

/** Dot-only variant — a tiny themed circle. Used in the file-tree
 *  row at the right edge so a glance at the sidebar gives an at-a-
 *  status read without expanding the file. */
export function StatusDot({ status, ariaLabel }: DotProps): ReactElement {
  const accent = `var(${statusAccentVar(status)})`
  return (
    <span
      className="nx-status-dot"
      role="img"
      aria-label={ariaLabel ?? `status: ${status}`}
      data-status={isKnownStatus(status) ? status : 'other'}
      style={{ ...dotStyle, background: accent }}
    />
  )
}

const dotStyle: CSSProperties = {
  width: 8,
  height: 8,
  borderRadius: '50%',
  flexShrink: 0,
  display: 'inline-block',
}

const pillStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 6,
  padding: '1px 8px',
  borderRadius: 999,
  fontSize: '0.78em',
  fontFamily: 'var(--font-interface)',
  textTransform: 'uppercase',
  letterSpacing: '0.06em',
  color: 'var(--text-muted)',
  background: 'var(--background-secondary)',
  border: '1px solid var(--background-modifier-border)',
  lineHeight: 1.2,
}
