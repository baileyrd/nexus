import type { ReactElement, SVGProps } from 'react'
import {
  Archive,
  ArrowDownUp,
  ArrowLeft,
  ArrowRight,
  BookOpen,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Clock,
  CornerDownLeft,
  CornerUpRight,
  Crosshair,
  Database,
  ExternalLink,
  FileBraces,
  FileCode,
  FilePlus2,
  FileText,
  Flame,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  Grid3x3,
  Info,
  Link,
  List,
  MessageSquare,
  Minus,
  MoreHorizontal,
  PanelLeft,
  PanelRight,
  Play,
  Plug,
  Plus,
  RefreshCw,
  Search,
  Settings,
  SlidersHorizontal,
  Sparkles,
  Square,
  SquareCheckBig,
  Star,
  Tag,
  Terminal,
  Trash2,
  Waypoints,
  X,
  Zap,
  GitBranch,
  HelpCircle,
  LayoutTemplate,
  type LucideIcon,
  type LucideProps,
} from 'lucide-react'

/**
 * Named icon set. All icons are Lucide glyphs — the same open-source
 * library Obsidian uses, so anything you see in Obsidian's chrome has
 * a match here at pixel parity.
 *
 * Two kinds of names:
 *   • Descriptive keys carried over from the original design-bundle
 *     vocabulary (`folder`, `graph`, `bolt`, `ember`, …). Kept for
 *     backwards compat with the rest of the codebase.
 *   • Lucide-native PascalCase names are NOT exposed here — callers use
 *     the keys below. If you need a new icon, add it to ICON_MAP.
 *
 * Stroke width follows Lucide's default (2); a few entries override
 * (e.g. `check` uses 3) to match the original design's emphasis.
 * Filled variants (`ember` → Flame, `play` → Play) render with
 * fill=currentColor instead of stroke.
 */

interface IconEntry {
  component: LucideIcon
  strokeWidth?: number
  /** Render with fill=currentColor instead of stroke. */
  filled?: boolean
}

const ICON_MAP = {
  // Filesystem
  folder:      { component: Folder },
  folderOpen:  { component: FolderOpen },
  folderPlus:  { component: FolderPlus },
  filePlus:    { component: FilePlus2 },
  doc:         { component: FileText },
  // BL-080: per-extension file glyphs for the file tree.
  fileCode:    { component: FileCode },
  fileJson:    { component: FileBraces },
  archive:     { component: Archive },
  trash:       { component: Trash2 },

  // Navigation
  chev:        { component: ChevronRight },
  collapseAll: { component: ChevronsUpDown },
  panel:       { component: PanelRight },
  panelLeft:   { component: PanelLeft },
  arrowLeft:   { component: ArrowLeft },
  arrowRight:  { component: ArrowRight },
  chevDown:    { component: ChevronDown },
  more:        { component: MoreHorizontal },
  info:        { component: Info },
  external:    { component: ExternalLink },

  // Knowledge graph / links
  link:        { component: Link },
  linkIn:      { component: CornerDownLeft },
  linkOut:     { component: CornerUpRight },
  graph:       { component: Waypoints },
  tag:         { component: Tag },
  book:        { component: BookOpen },
  list:        { component: List },
  comment:     { component: MessageSquare },

  // Tools
  search:      { component: Search },
  terminal:    { component: Terminal },
  sparkle:     { component: Sparkles },
  bolt:        { component: Zap },
  plug:        { component: Plug },
  git:         { component: GitBranch },
  db:          { component: Database },
  grid:        { component: Grid3x3 },
  task:        { component: SquareCheckBig },
  settings:    { component: Settings },
  help:        { component: HelpCircle },
  sliders:     { component: SlidersHorizontal },
  refresh:     { component: RefreshCw },
  clock:       { component: Clock },
  star:        { component: Star },
  template:    { component: LayoutTemplate },

  // Editing / sort
  sortAZ:      { component: ArrowDownUp },
  crosshair:   { component: Crosshair },
  pencil:      { component: Pencil },

  // Controls / glyphs
  plus:        { component: Plus },
  x:           { component: X },
  check:       { component: Check, strokeWidth: 3 },
  min:         { component: Minus },
  max:         { component: Square },

  // Filled glyphs
  play:        { component: Play, filled: true },
  stop:        { component: Square, filled: true },
  ember:       { component: Flame, filled: true },
} as const satisfies Record<string, IconEntry>

export type IconName = keyof typeof ICON_MAP

interface IconProps extends Omit<LucideProps, 'size' | 'ref'> {
  name: IconName
  /** Edge length in px. Defaults to 16. */
  size?: number
}

/**
 * Render any named icon at a given size. Icons use `currentColor` so
 * they inherit `color` from the surrounding text style.
 *
 *   <Icon name="folder" size={14} />
 */
export function Icon({ name, size = 16, ...rest }: IconProps): ReactElement {
  const entry = ICON_MAP[name] as IconEntry
  const Component = entry.component
  const filled = entry.filled === true
  // Don't pass fill/stroke unless the icon is explicitly filled — passing
  // `undefined` via spread overrides Lucide's defaultAttributes (`fill:
  // "none"`), which otherwise drops back to SVG's default (black fill)
  // and every outlined icon renders as a solid black blob.
  const colorProps = filled ? { fill: 'currentColor', stroke: 'none' } : {}
  return (
    <Component
      size={size}
      strokeWidth={entry.strokeWidth}
      {...colorProps}
      aria-hidden={rest['aria-label'] ? undefined : true}
      {...rest}
    />
  )
}

// Backwards-compat export: a few call sites type-destructure the
// props shape from SVGProps<SVGSVGElement>. Keep the import stable.
export type { SVGProps }
