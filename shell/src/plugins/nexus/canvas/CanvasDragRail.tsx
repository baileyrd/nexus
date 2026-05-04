// Bottom-centre drag rail. Three drag sources, mirroring Obsidian's
// canvas: blank card, note from vault, media from vault. The blank-
// card source emits a custom MIME type that the canvas drop handler
// turns into an empty text node at the drop position. The other two
// sources fire a "Coming soon" toast on dragend (Nexus has no vault
// file picker for canvas embeds yet).

import { type CSSProperties } from 'react'

/** Custom drag MIME for "drop a blank text card here". */
export const CANVAS_BLANK_CARD_MIME = 'application/x-nexus-canvas-card'

interface Props {
  /** Coming-soon toast factory. */
  comingSoon: (label: string) => () => void
}

export function CanvasDragRail({ comingSoon }: Props) {
  return (
    <div
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        left: '50%',
        bottom: 16,
        transform: 'translateX(-50%)',
        display: 'flex',
        gap: 4,
        padding: 4,
        borderRadius: 8,
        background: 'var(--background-secondary)',
        border: '1px solid var(--divider-color)',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
        pointerEvents: 'auto',
      }}
    >
      <DragSource
        title="Drag to add card"
        onDragStart={(e) => {
          e.dataTransfer?.setData(CANVAS_BLANK_CARD_MIME, '1')
          if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'
        }}
        icon={<CardGlyph />}
      />
      <DragSource
        title="Drag to add note from vault"
        onDragStart={(e) => {
          // No drop target understands this yet — flag it as a stub
          // payload so the receiver can ignore it cleanly.
          if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'
        }}
        onDragEnd={comingSoon('Drag to add note from vault')}
        icon={<NoteGlyph />}
      />
      <DragSource
        title="Drag to add media from vault"
        onDragStart={(e) => {
          if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'
        }}
        onDragEnd={comingSoon('Drag to add media from vault')}
        icon={<MediaGlyph />}
      />
    </div>
  )
}

const SOURCE_BASE: CSSProperties = {
  width: 36,
  height: 36,
  display: 'inline-grid',
  placeItems: 'center',
  border: '1px solid transparent',
  borderRadius: 6,
  background: 'transparent',
  color: 'var(--text-muted)',
  cursor: 'grab',
}

function DragSource({
  title,
  onDragStart,
  onDragEnd,
  icon,
}: {
  title: string
  onDragStart: (e: React.DragEvent) => void
  onDragEnd?: () => void
  icon: React.ReactNode
}) {
  return (
    <div
      draggable
      title={title}
      aria-label={title}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      style={SOURCE_BASE}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--background-modifier-hover)'
        e.currentTarget.style.color = 'var(--text-normal)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
        e.currentTarget.style.color = 'var(--text-muted)'
      }}
    >
      {icon}
    </div>
  )
}

// Tiny inline glyphs to match the Obsidian look (blank rectangle,
// document with lines, picture). Keeping them inline means no asset
// pipeline and they pick up `currentColor` from the parent.

function CardGlyph() {
  return (
    <svg width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
      <rect x={4} y={4} width={16} height={16} rx={2} />
    </svg>
  )
}

function NoteGlyph() {
  return (
    <svg width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
      <rect x={5} y={3} width={14} height={18} rx={2} />
      <line x1={8} y1={8} x2={16} y2={8} />
      <line x1={8} y1={12} x2={16} y2={12} />
      <line x1={8} y1={16} x2={13} y2={16} />
    </svg>
  )
}

function MediaGlyph() {
  return (
    <svg width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
      <rect x={4} y={5} width={16} height={14} rx={2} />
      <circle cx={9} cy={10} r={1.5} fill="currentColor" stroke="none" />
      <path d="M5 18 L11 12 L15 16 L20 11" />
    </svg>
  )
}
