// The accordion panel rendered by `nexus.noteContext`.
//
// Phase 4.3 step 1 — skeleton only. Each section's body is a tiny
// placeholder; subsequent commits port the four legacy views
// (backlinks, outgoing-links, tags, graph) into these slots and
// retire the standalone plugins.

import { type ReactNode, useCallback } from 'react'
import { AccordionSection } from './accordion'
import { BacklinksSection } from './sections/BacklinksSection'
import { GraphSection } from './sections/GraphSection'
import { OutgoingLinksSection } from './sections/OutgoingLinksSection'
import { TagsSection } from './sections/TagsSection'
import { useNoteContextStore } from './store'

interface SectionMeta {
  id: string
  title: string
  /** Component rendered when the section is expanded. */
  body: () => ReactNode
}

// Order matters — this is the visual order in the panel.
const SECTIONS: SectionMeta[] = [
  { id: 'backlinks',     title: 'Backlinks',      body: () => <BacklinksSection /> },
  { id: 'outgoingLinks', title: 'Outgoing Links', body: () => <OutgoingLinksSection /> },
  { id: 'tags',          title: 'Tags',           body: () => <TagsSection /> },
  { id: 'graph',         title: 'Graph',          body: () => <GraphSection /> },
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
          {s.body()}
        </AccordionSection>
      ))}
    </div>
  )
}
