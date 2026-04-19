// src/shell/slots/SlotSurface.tsx
// Renders whatever plugins have registered into a slot.
// Empty slot = empty DOM. No fallbacks, no placeholders.

import type { SlotEntry } from '../../registry/SlotRegistry'

interface Props {
  entries: SlotEntry[]
}

export function SlotSurface({ entries }: Props) {
  return (
    <>
      {entries.map(entry => (
        <entry.component key={entry.id} />
      ))}
    </>
  )
}
