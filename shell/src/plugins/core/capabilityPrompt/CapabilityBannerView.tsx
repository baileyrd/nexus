// shell/src/plugins/core/capabilityPrompt/CapabilityBannerView.tsx
//
// WI-31 — Non-blocking capability banner. Shown for plugins that
// declare only low/medium risk capabilities. Auto-dismisses after 10s
// if the user doesn't interact; clicking "Review" opens the modal for
// that plugin (same component as the blocking path) so the user can
// audit the detail list.

import { useEffect } from 'react'
import { useCapabilityPromptStore, type Banner } from './capabilityPromptStore'
import { highestRisk } from '../../nexus/pluginsMgmt/capabilityInfo'
import { zIndex } from '../../../shell/zIndex'

const BANNER_AUTO_DISMISS_MS = 10_000

export function CapabilityBannerView() {
  const banners = useCapabilityPromptStore((s) => s.banners)
  const dismiss = useCapabilityPromptStore((s) => s.dismissBanner)

  // Per-banner auto-dismiss timer. useEffect re-runs when `banners`
  // changes; we schedule a timeout for any banner that doesn't already
  // have one. Timeouts are cleaned up via the cleanup function so
  // dismiss-before-timeout doesn't double-fire.
  useEffect(() => {
    const timers: Array<ReturnType<typeof setTimeout>> = []
    for (const b of banners) {
      const remaining = Math.max(
        0,
        BANNER_AUTO_DISMISS_MS - (Date.now() - b.raisedAt),
      )
      timers.push(setTimeout(() => dismiss(b.id), remaining))
    }
    return () => {
      for (const t of timers) clearTimeout(t)
    }
  }, [banners, dismiss])

  if (banners.length === 0) return null

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
        pointerEvents: 'none',
      }}
    >
      {banners.map((b) => (
        <BannerItem key={b.id} banner={b} onDismiss={() => dismiss(b.id)} />
      ))}
    </div>
  )
}

function BannerItem({
  banner,
  onDismiss,
}: {
  banner: Banner
  onDismiss: () => void
}) {
  const risk = highestRisk(banner.caps) ?? 'low'
  return (
    <div
      role="status"
      style={{
        pointerEvents: 'auto',
        background: 'var(--background-secondary)',
        color: 'var(--text-normal)',
        border: '1px solid var(--background-modifier-border)',
        borderLeft: `3px solid var(--${risk === 'medium' ? 'warn' : 'ok'})`,
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
      <div>
        <strong style={{ fontWeight: 600 }}>{banner.pluginName}</strong>{' '}
        loaded with {banner.caps.length} capabilit
        {banner.caps.length === 1 ? 'y' : 'ies'}.
      </div>
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={onDismiss}
          style={{
            padding: '4px 10px',
            background: 'transparent',
            color: 'var(--text-faint)',
            border: '1px solid var(--divider-color)',
            borderRadius: 'var(--radius-s)',
            font: 'inherit',
            cursor: 'pointer',
          }}
        >
          Dismiss
        </button>
      </div>
    </div>
  )
}
