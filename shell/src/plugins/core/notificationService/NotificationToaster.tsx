// shell/src/plugins/core/notificationService/NotificationToaster.tsx
//
// Overlay renderer for the in-app notification queue. Subscribes to the
// shared `notificationQueue` via `useSyncExternalStore` and paints a
// bottom-right stack of toasts. Auto-dismissal is owned by the queue
// (per-item `setTimeout`); this component handles manual dismiss and
// action buttons. Sits at the `overlayFloating` z-index tier — above
// content, below modals — matching the capability banner.

import { useCallback, useSyncExternalStore } from 'react'
import { zIndex } from '../../../shell/zIndex'
import { notificationQueue, type Notification } from './notificationQueue'

/** Accent stripe per severity. Tokens are already used elsewhere in the
 *  shell (the capability banner uses --ok/--warn; the graph uses --risk). */
const SEVERITY_ACCENT: Record<Notification['type'], string> = {
  info: 'var(--interactive-accent)',
  success: 'var(--ok)',
  warning: 'var(--warn)',
  error: 'var(--risk)',
}

export interface NotificationToasterProps {
  /** Runs a notification action's `command`. The plugin wires this to
   *  `api.commands.execute`; left optional so the component is trivially
   *  testable in isolation. */
  onAction?: (command: string) => void
}

export function NotificationToaster({ onAction }: NotificationToasterProps) {
  // `subscribe` ignores the listener's argument — useSyncExternalStore
  // re-reads through `getSnapshot` on every notify. `getAll()` returns a
  // stable array reference between mutations (push/dismiss replace it),
  // satisfying the snapshot-stability contract.
  const subscribe = useCallback(
    (cb: () => void) => notificationQueue.subscribe(cb),
    [],
  )
  const getSnapshot = useCallback(() => notificationQueue.getAll(), [])
  const items = useSyncExternalStore(subscribe, getSnapshot)

  if (items.length === 0) return null

  return (
    <div
      aria-live="polite"
      style={{
        position: 'fixed',
        right: 16,
        bottom: 16,
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        zIndex: zIndex.overlayFloating,
        // Let clicks fall through the gaps; each toast re-enables itself.
        pointerEvents: 'none',
      }}
    >
      {items.map((n) => (
        <ToastItem
          key={n.id}
          n={n}
          onDismiss={() => notificationQueue.dismiss(n.id)}
          onAction={onAction}
        />
      ))}
    </div>
  )
}

function ToastItem({
  n,
  onDismiss,
  onAction,
}: {
  n: Notification
  onDismiss: () => void
  onAction?: (command: string) => void
}) {
  return (
    <div
      role={n.type === 'error' ? 'alert' : 'status'}
      style={{
        pointerEvents: 'auto',
        background: 'var(--background-secondary)',
        color: 'var(--text-normal)',
        border: '1px solid var(--background-modifier-border)',
        borderLeft: `3px solid ${SEVERITY_ACCENT[n.type]}`,
        borderRadius: 'var(--radius-s)',
        padding: '10px 14px',
        minWidth: 320,
        maxWidth: 420,
        boxShadow: 'var(--shadow)',
        fontFamily: 'var(--font-interface)',
        fontSize: 13,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <div style={{ flex: 1, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {n.message}
        </div>
        <button
          type="button"
          aria-label="Dismiss notification"
          onClick={onDismiss}
          style={{
            flex: '0 0 auto',
            width: 20,
            height: 20,
            lineHeight: '16px',
            padding: 0,
            background: 'transparent',
            color: 'var(--text-faint)',
            border: 'none',
            borderRadius: 'var(--radius-s)',
            font: 'inherit',
            fontSize: 16,
            cursor: 'pointer',
          }}
        >
          ×
        </button>
      </div>
      {n.actions && n.actions.length > 0 && (
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          {n.actions.map((a, i) => (
            <button
              key={`${a.command}-${i}`}
              type="button"
              onClick={() => {
                onAction?.(a.command)
                onDismiss()
              }}
              style={{
                padding: '4px 10px',
                background: 'transparent',
                color: 'var(--interactive-accent)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
                font: 'inherit',
                cursor: 'pointer',
              }}
            >
              {a.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
