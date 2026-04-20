import { createElement } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useRightPanelStore } from './rightPanelStore'

/**
 * Host view into the `rightPanel` slot.
 *
 * As of the titlebar-icon refactor, tab switching lives in the
 * titlebar's right cluster (one button per contributed tab, firing
 * the tab plugin's `.focus` command). The host no longer renders an
 * INSPECTOR header or a tab row — each contributed view is expected
 * to carry its own header/toolbar. The host just picks whichever
 * view is active and mounts it.
 */
export function RightPanelHost() {
  const activeViewId = useRightPanelStore((s) => s.activeViewId)
  const entries = useSlotStore((s) => s.slots.rightPanelContent)

  const activeEntry =
    activeViewId != null ? entries.find((e) => e.id === activeViewId) : undefined

  return (
    <div
      style={{
        height: '100%',
        width: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {activeEntry ? (
        createElement(activeEntry.component)
      ) : (
        <div
          style={{
            padding: 16,
            color: 'var(--fg-dim)',
            fontSize: 12,
            textAlign: 'center',
          }}
        >
          No inspectors registered
        </div>
      )}
    </div>
  )
}
