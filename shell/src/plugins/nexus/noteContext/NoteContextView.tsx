// The accordion panel rendered by `nexus.noteContext`.
//
// Phase 4.3 step 1 — skeleton only. Each section's body is a tiny
// placeholder; subsequent commits port the four legacy views
// (backlinks, outgoing-links, tags, graph) into these slots and
// retire the standalone plugins.

import { useCallback } from 'react'
import { AccordionSection } from './accordion'
import { useNoteContextStore } from './store'

interface SectionMeta {
  id: string
  title: string
}

// Order matters — this is the visual order in the panel.
const SECTIONS: SectionMeta[] = [
  { id: 'backlinks',     title: 'Backlinks' },
  { id: 'outgoingLinks', title: 'Outgoing Links' },
  { id: 'tags',          title: 'Tags' },
  { id: 'graph',         title: 'Graph' },
]

export function NoteContextView() {
  const expanded = useNoteContextStore((s) => s.expanded)
  const toggle = useNoteContextStore((s) => s.toggle)
  const onToggle = useCallback((id: string) => toggle(id), [toggle])

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        overflowY: 'auto',
        fontSize: 13,
        color: 'var(--text-normal)',
      }}
    >
      {SECTIONS.map((s) => (
        <AccordionSection
          key={s.id}
          id={s.id}
          title={s.title}
          expanded={expanded.has(s.id)}
          onToggle={onToggle}
        >
          <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
            Placeholder — Phase 4.3 step {s.id === 'backlinks' ? '4' : s.id === 'outgoingLinks' ? '2' : s.id === 'tags' ? '3' : '5'} wires this section.
          </div>
        </AccordionSection>
      ))}
    </div>
  )
}
