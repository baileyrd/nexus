// Generic vertical-accordion primitive used by the noteContext panel.
// Each `<Section>` has a header (click to expand/collapse), optional
// badge text (used by section authors to show counts), and body
// children that ONLY mount while `expanded` is true (hard lazy-load,
// per BL-XXX Phase 4.3 product decision).
//
// Independent of `nexus.noteContext`'s domain — colocated here because
// it's the only consumer today. If a second accordion panel lands in
// the shell, this lifts to `shell/src/plugins/nexus/_lib/`.

import { type ReactNode, useCallback, useId } from 'react'

export interface AccordionSectionProps {
  /** Stable id used for persisted "is expanded" state. */
  id: string
  /** Visible heading text. */
  title: string
  /** Optional short tag rendered next to the title — typical use is
   *  a result count (e.g. "12"). Rendered muted; hidden when null. */
  badge?: string | null
  /** True ⇒ body is mounted and visible. */
  expanded: boolean
  /** Fired when the header is clicked. */
  onToggle: (id: string) => void
  /** Children rendered only while `expanded`. */
  children: ReactNode
}

export function AccordionSection({
  id,
  title,
  badge,
  expanded,
  onToggle,
  children,
}: AccordionSectionProps) {
  const headerId = useId()
  const bodyId = useId()
  const handleClick = useCallback(() => onToggle(id), [id, onToggle])
  const handleKey = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        onToggle(id)
      }
    },
    [id, onToggle],
  )
  return (
    <div
      style={{
        borderBottom: '1px solid var(--background-modifier-border)',
      }}
    >
      <div
        id={headerId}
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-controls={bodyId}
        onClick={handleClick}
        onKeyDown={handleKey}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '6px 10px',
          fontSize: 12,
          fontWeight: 600,
          color: 'var(--text-normal)',
          cursor: 'pointer',
          userSelect: 'none',
          background: expanded ? 'var(--background-secondary)' : 'transparent',
        }}
      >
        <span
          style={{
            display: 'inline-block',
            width: 10,
            transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
            transition: 'transform 80ms',
            color: 'var(--text-muted)',
          }}
        >
          ▶
        </span>
        <span style={{ flex: 1 }}>{title}</span>
        {badge && (
          <span style={{ color: 'var(--text-faint)', fontWeight: 400 }}>{badge}</span>
        )}
      </div>
      {expanded && (
        <div id={bodyId} role="region" aria-labelledby={headerId}>
          {children}
        </div>
      )}
    </div>
  )
}
