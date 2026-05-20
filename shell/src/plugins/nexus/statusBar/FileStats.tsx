import { useMemo } from 'react'
import { useEditorStore } from '../editor/editorStore'
import { snap, useFrameSnapshot } from '../../../stores/useFrameSnapshot'

function fileExt(name: string): string | null {
  const i = name.lastIndexOf('.')
  if (i <= 0 || i === name.length - 1) return null
  return name.slice(i + 1).toUpperCase()
}

const SEP_STYLE: React.CSSProperties = {
  opacity: 0.35,
  padding: '0 3px',
  userSelect: 'none',
}

// BL-110: pull selectors that drive this status-bar slice through one
// rAF-coalesced snapshot. Editor tabs update from kernel watch events;
// pre-BL-110 a save followed by a re-render produced two paint
// commits, now one per frame.
//
// BL-XXX Phase 4.3 step 6 — the "X backlinks" indicator that previously
// lived here was driven by useBacklinksStore, populated by the legacy
// `nexus.backlinks` plugin. After the merge into `nexus.noteContext`
// the store is no longer maintained (the new section's data lives
// inside the section's React subtree). Re-adding the indicator means
// re-introducing a permanent always-on subscriber outside the
// accordion's lazy-load contract; captured as a follow-up.
const FILE_STATS_ENTRIES = [
  snap(useEditorStore, (s) => s.tabs),
  snap(useEditorStore, (s) => s.activeRelpath),
] as const

export function FileStats() {
  const [tabs, activeRelpath] = useFrameSnapshot(FILE_STATS_ENTRIES)

  const activeTab = useMemo(
    () => tabs.find((t) => t.relpath === activeRelpath) ?? null,
    [tabs, activeRelpath],
  )

  if (!activeTab || activeTab.loading || activeTab.error) return null

  const ext = fileExt(activeTab.name)
  const content = activeTab.content
  const words = content.trim() ? content.trim().split(/\s+/).length : 0
  const chars = content.length

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      {ext && (
        <>
          <span>{ext}</span>
          <span style={SEP_STYLE}>|</span>
        </>
      )}
      <span>UTF-8</span>
      <span style={SEP_STYLE}>|</span>
      <span>
        {words.toLocaleString()} words · {chars.toLocaleString()} chars
      </span>
    </span>
  )
}
