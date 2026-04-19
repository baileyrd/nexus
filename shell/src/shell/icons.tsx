// src/shell/icons.tsx
// Ported from docs/test/forge_icons.jsx — stroke 1.75 primitives.
// Each icon forwards its props to the inner <svg>.

import type { SVGProps } from 'react'

type P = SVGProps<SVGSVGElement>

const common = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
}

export const Folder = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
  </svg>
)
export const FolderOpen = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1H3V7z" />
    <path d="M3 9h18l-2 8a2 2 0 0 1-2 1.5H5.5A2 2 0 0 1 3.5 17L3 9z" />
  </svg>
)
export const Doc = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
    <path d="M14 3v5h5" />
    <path d="M9 13h7M9 17h5" />
  </svg>
)
export const Chev = (p: P) => (
  <svg {...common} strokeWidth={2} {...p}>
    <path d="M9 6l6 6-6 6" />
  </svg>
)
export const Search = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <circle cx={11} cy={11} r={7} />
    <path d="m20 20-3-3" />
  </svg>
)
export const Graph = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <circle cx={6} cy={6} r={2.2} />
    <circle cx={18} cy={6} r={2.2} />
    <circle cx={12} cy={18} r={2.2} />
    <path d="M7.6 7.2l2.8 9M16.4 7.2l-2.8 9M8 6h8" />
  </svg>
)
export const Bolt = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M13 3L4 14h7l-1 7 9-11h-7l1-7z" />
  </svg>
)
export const Sparkle = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3z" />
    <path d="M18.5 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8.8-2z" />
  </svg>
)
export const Terminal = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <rect x={3} y={4} width={18} height={16} rx={2} />
    <path d="M7 9l3 3-3 3M13 15h4" />
  </svg>
)
export const Task = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <rect x={4} y={4} width={16} height={16} rx={3} />
    <path d="m8 12 3 3 5-6" />
  </svg>
)
export const Git = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <circle cx={6} cy={6} r={2} />
    <circle cx={6} cy={18} r={2} />
    <circle cx={18} cy={12} r={2} />
    <path d="M6 8v8M8 6h6a4 4 0 0 1 4 4v0" />
  </svg>
)
export const Plug = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M9 3v4M15 3v4M7 7h10v5a5 5 0 0 1-10 0V7zM12 17v4" />
  </svg>
)
export const Settings = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <circle cx={12} cy={12} r={3} />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h0a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v0a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
  </svg>
)
export const Star = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="m12 3 2.8 5.8 6.4.9-4.6 4.5 1 6.3L12 17.8 6.4 20.5l1-6.3L2.8 9.7l6.4-.9L12 3z" />
  </svg>
)
export const Clock = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <circle cx={12} cy={12} r={9} />
    <path d="M12 7v5l3 2" />
  </svg>
)
export const Link = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 1 0-7-7l-1 1" />
    <path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 1 0 7 7l1-1" />
  </svg>
)
export const PanelIcon = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <rect x={3} y={4} width={18} height={16} rx={2} />
    <path d="M14 4v16" />
  </svg>
)
export const X = (p: P) => (
  <svg {...common} strokeWidth={2} {...p}>
    <path d="M18 6L6 18M6 6l12 12" />
  </svg>
)
export const Plus = (p: P) => (
  <svg {...common} strokeWidth={2} {...p}>
    <path d="M12 5v14M5 12h14" />
  </svg>
)
export const Check = (p: P) => (
  <svg {...common} strokeWidth={3} {...p}>
    <path d="m5 12 5 5L20 7" />
  </svg>
)
export const Ember = (p: P) => (
  <svg viewBox="0 0 24 24" fill="currentColor" {...p}>
    <path d="M12 3c2 3 4 5 4 8a4 4 0 0 1-8 0c0-2 2-3 2-5-1 0-2-1-2-2 2 0 3-1 4-1z" />
  </svg>
)
export const Db = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <ellipse cx={12} cy={6} rx={8} ry={3} />
    <path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
  </svg>
)
export const Tag = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M3 12V5a2 2 0 0 1 2-2h7l9 9-9 9-9-9z" />
    <circle cx={7.5} cy={7.5} r={1} />
  </svg>
)
export const Book = (p: P) => (
  <svg {...common} strokeWidth={1.75} {...p}>
    <path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2V5z" />
    <path d="M6 3v16" />
  </svg>
)
export const Min = (p: P) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} {...p}>
    <path d="M5 12h14" />
  </svg>
)
export const Max = (p: P) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} {...p}>
    <rect x={5} y={5} width={14} height={14} rx={1} />
  </svg>
)
export const Sliders = (p: P) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" {...p}>
    <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h14M20 18h0" />
    <circle cx={16} cy={6} r={2} />
    <circle cx={10} cy={12} r={2} />
    <circle cx={18} cy={18} r={2} />
  </svg>
)

export const Ic = {
  folder: Folder, folderOpen: FolderOpen, doc: Doc, chev: Chev, search: Search,
  graph: Graph, bolt: Bolt, sparkle: Sparkle, terminal: Terminal, task: Task,
  git: Git, plug: Plug, settings: Settings, star: Star, clock: Clock, link: Link,
  panel: PanelIcon, x: X, plus: Plus, check: Check, ember: Ember, db: Db,
  tag: Tag, book: Book, min: Min, max: Max, sliders: Sliders,
  // aliases — stable names used by the activity bar store
  files: Doc,
}

export type IconName = keyof typeof Ic
