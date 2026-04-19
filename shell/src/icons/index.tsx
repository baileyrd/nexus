import type { ReactElement, SVGProps } from 'react'

/**
 * Typed icon set ported from `.design-bundle/project/forge_icons.jsx`.
 *
 * Each entry is the SVG-children renderer (paths / circles / etc.)
 * minus the wrapping `<svg>` so the host can vary `width` / `height`
 * / `aria-*` while keeping the visual identical to the design.
 *
 * Stroke + fill defaults match the design (`stroke-width: 1.75`,
 * `currentColor`, round caps/joins). A handful of icons override on
 * `RENDERERS` (e.g. `chev` uses 2, `check` uses 3) — that's intentional
 * design weight and the renderer wraps each icon in its own `<svg>`
 * with the correct attributes.
 *
 * Usage:
 *   import { Icon } from '../../icons'
 *   <Icon name="bolt" size={14} />
 *   <Icon name="folder" />     // size defaults to 16
 *
 * The activity-bar contract still accepts a single `iconPath` string
 * for backwards compatibility, but new items should set
 * `iconName: 'folder'` instead — that path also covers multi-element
 * icons (search, graph, sparkle, …) which the legacy single-path
 * contract can't represent.
 */

interface IconEntry {
  /** SVG children. Wrapped in `<svg viewBox="0 0 24 24">` by `Icon`. */
  body: ReactElement
  /** Stroke width override; defaults to 1.75 (the design baseline). */
  strokeWidth?: number
  /** Set `fill="currentColor"` instead of stroke. Defaults to false. */
  filled?: boolean
}

const RENDERERS = {
  folder: { body: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" /> },
  folderOpen: {
    body: (
      <>
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1H3V7z" />
        <path d="M3 9h18l-2 8a2 2 0 0 1-2 1.5H5.5A2 2 0 0 1 3.5 17L3 9z" />
      </>
    ),
  },
  doc: {
    body: (
      <>
        <path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
        <path d="M14 3v5h5" />
        <path d="M9 13h7M9 17h5" />
      </>
    ),
  },
  chev: { body: <path d="M9 6l6 6-6 6" />, strokeWidth: 2 },
  search: {
    body: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-3-3" />
      </>
    ),
  },
  graph: {
    body: (
      <>
        <circle cx="6" cy="6" r="2.2" />
        <circle cx="18" cy="6" r="2.2" />
        <circle cx="12" cy="18" r="2.2" />
        <path d="M7.6 7.2l2.8 9M16.4 7.2l-2.8 9M8 6h8" />
      </>
    ),
  },
  bolt: { body: <path d="M13 3L4 14h7l-1 7 9-11h-7l1-7z" /> },
  sparkle: {
    body: (
      <>
        <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3z" />
        <path d="M18.5 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8.8-2z" />
      </>
    ),
  },
  terminal: {
    body: (
      <>
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M7 9l3 3-3 3M13 15h4" />
      </>
    ),
  },
  task: {
    body: (
      <>
        <rect x="4" y="4" width="16" height="16" rx="3" />
        <path d="m8 12 3 3 5-6" />
      </>
    ),
  },
  git: {
    body: (
      <>
        <circle cx="6" cy="6" r="2" />
        <circle cx="6" cy="18" r="2" />
        <circle cx="18" cy="12" r="2" />
        <path d="M6 8v8M8 6h6a4 4 0 0 1 4 4v0" />
      </>
    ),
  },
  plug: { body: <path d="M9 3v4M15 3v4M7 7h10v5a5 5 0 0 1-10 0V7zM12 17v4" /> },
  settings: {
    body: (
      <>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h0a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v0a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
      </>
    ),
  },
  star: { body: <path d="m12 3 2.8 5.8 6.4.9-4.6 4.5 1 6.3L12 17.8 6.4 20.5l1-6.3L2.8 9.7l6.4-.9L12 3z" /> },
  clock: {
    body: (
      <>
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
      </>
    ),
  },
  link: {
    body: (
      <>
        <path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 1 0-7-7l-1 1" />
        <path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 1 0 7 7l1-1" />
      </>
    ),
  },
  panel: {
    body: (
      <>
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M14 4v16" />
      </>
    ),
  },
  x: { body: <path d="M18 6L6 18M6 6l12 12" />, strokeWidth: 2 },
  plus: { body: <path d="M12 5v14M5 12h14" />, strokeWidth: 2 },
  check: { body: <path d="m5 12 5 5L20 7" />, strokeWidth: 3 },
  ember: { body: <path d="M12 3c2 3 4 5 4 8a4 4 0 0 1-8 0c0-2 2-3 2-5-1 0-2-1-2-2 2 0 3-1 4-1z" />, filled: true },
  db: {
    body: (
      <>
        <ellipse cx="12" cy="6" rx="8" ry="3" />
        <path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
      </>
    ),
  },
  tag: {
    body: (
      <>
        <path d="M3 12V5a2 2 0 0 1 2-2h7l9 9-9 9-9-9z" />
        <circle cx="7.5" cy="7.5" r="1" />
      </>
    ),
  },
  book: {
    body: (
      <>
        <path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2V5z" />
        <path d="M6 3v16" />
      </>
    ),
  },
  min: { body: <path d="M5 12h14" />, strokeWidth: 2 },
  max: { body: <rect x="5" y="5" width="14" height="14" rx="1" />, strokeWidth: 2 },
  sliders: {
    body: (
      <>
        <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h14M20 18h0" />
        <circle cx="16" cy="6" r="2" />
        <circle cx="10" cy="12" r="2" />
        <circle cx="18" cy="18" r="2" />
      </>
    ),
  },
  // Lucide-style refresh / cycle — not in the design bundle but
  // reused across "reload" buttons in the shell. Two arcs + two
  // arrowheads, single stroke weight.
  refresh: {
    body: (
      <>
        <path d="M3 12a9 9 0 0 1 15.5-6.36L21 8" />
        <path d="M21 3v5h-5" />
        <path d="M21 12a9 9 0 0 1-15.5 6.36L3 16" />
        <path d="M3 21v-5h5" />
      </>
    ),
  },
  // Right-pointing play triangle for "run" actions. Filled glyph,
  // no stroke — design vocabulary for terminal/agent run buttons.
  play: { body: <path d="M5 4l14 8-14 8z" />, filled: true },
  // 2×2 grid — four small squares. Used by nexus.processes for the
  // "many things" / observability mental model. Stroke-only.
  grid: {
    body: (
      <>
        <rect x="3" y="3" width="7" height="7" rx="0.5" />
        <rect x="14" y="3" width="7" height="7" rx="0.5" />
        <rect x="3" y="14" width="7" height="7" rx="0.5" />
        <rect x="14" y="14" width="7" height="7" rx="0.5" />
      </>
    ),
  },
  // Lucide-style trash for delete actions.
  trash: {
    body: (
      <>
        <path d="M3 6h18" />
        <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        <path d="M6 6l1 14a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-14" />
        <path d="M10 11v6M14 11v6" />
      </>
    ),
  },
} satisfies Record<string, IconEntry>

export type IconName = keyof typeof RENDERERS

interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'children' | 'viewBox' | 'fill' | 'stroke'> {
  name: IconName
  /** Edge length in px. Width and height are set together. Defaults to 16. */
  size?: number
}

/**
 * Render any icon from the design vocabulary at a given size. All
 * icons share `viewBox="0 0 24 24"` and use `currentColor` so they
 * inherit `color` from the surrounding text style.
 */
export function Icon({ name, size = 16, ...rest }: IconProps): ReactElement {
  // The literal-narrow inference under `satisfies` drops optional
  // fields from individual entries; widen back to the contract here.
  const entry = RENDERERS[name] as IconEntry
  const isFilled = entry.filled === true
  const sw = entry.strokeWidth ?? 1.75
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={isFilled ? 'currentColor' : 'none'}
      stroke={isFilled ? 'none' : 'currentColor'}
      strokeWidth={isFilled ? undefined : sw}
      strokeLinecap={isFilled ? undefined : 'round'}
      strokeLinejoin={isFilled ? undefined : 'round'}
      aria-hidden={rest['aria-label'] ? undefined : true}
      {...rest}
    >
      {entry.body}
    </svg>
  )
}
