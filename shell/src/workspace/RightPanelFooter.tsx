// Bottom strip of the right sidedock. Replaces the global status bar —
// shows per-document stats (words, characters, backlinks) plus the
// forge sync dot. Sits alongside ForgeSelector on the left: both are
// persistent sidedock footers bolted into SidedockFrame.

import { useMemo } from 'react'
import { useEditorStore } from '../plugins/nexus/editor/editorStore'
import { useWorkspaceStore } from '../plugins/nexus/workspace/workspaceStore'

const SEP_STYLE: React.CSSProperties = {
  opacity: 0.35,
  padding: '0 3px',
  userSelect: 'none',
}

export function RightPanelFooter(): JSX.Element {
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const synced = rootPath !== null

  const activeTab = useMemo(
    () => tabs.find((t) => t.relpath === activeRelpath) ?? null,
    [tabs, activeRelpath],
  )

  // Compute stats when a markdown tab is loaded + non-erroring. Other
  // states render the dock status row without per-file stats.
  const stats =
    activeTab && !activeTab.loading && !activeTab.error
      ? {
          words: activeTab.content.trim()
            ? activeTab.content.trim().split(/\s+/).length
            : 0,
          chars: activeTab.content.length,
        }
      : null

  // BL-XXX Phase 4.3 step 6 — the "X backlinks" indicator that lived
  // here was driven by `useBacklinksStore`, which the legacy
  // `nexus.backlinks` plugin populated. After the merge into
  // `nexus.noteContext` the store is no longer maintained (the new
  // section's data lives inside the section's React subtree and only
  // exists while the section is expanded). Re-adding the indicator
  // means re-introducing a permanent always-on subscriber outside the
  // accordion's lazy-load contract — captured as a follow-up.
  return (
    <div
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '4px 10px',
        borderTop: '1px solid var(--divider-color)',
        background: 'var(--background-secondary)',
        color: 'var(--text-muted)',
        fontSize: 11,
        fontVariantNumeric: 'tabular-nums',
        overflow: 'hidden',
        whiteSpace: 'nowrap',
      }}
    >
      {stats ? (
        <>
          <span>{stats.words.toLocaleString()} words</span>
          <span style={SEP_STYLE}>|</span>
          <span>{stats.chars.toLocaleString()} chars</span>
        </>
      ) : (
        <span>{synced ? 'Forge synced' : 'No forge open'}</span>
      )}
      <span style={{ flex: '1 1 auto' }} />
      <span
        aria-hidden
        style={{
          width: 7,
          height: 7,
          borderRadius: '50%',
          flexShrink: 0,
          background: synced ? 'var(--ok)' : 'var(--text-faint)',
          boxShadow: synced ? '0 0 4px var(--ok)' : 'none',
        }}
      />
    </div>
  )
}
