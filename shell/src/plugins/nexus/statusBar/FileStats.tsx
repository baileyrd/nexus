import { useMemo } from 'react'
import { useEditorStore } from '../editor/editorStore'
import { useBacklinksDataStore } from '../noteContext/backlinksDataStore'
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

// BL-110: pull the four selectors that drive this status-bar slice
// through one rAF-coalesced snapshot. Editor tabs + backlinks are
// updated from independent async sources (kernel watch events vs
// post-active-file-change backlinks load); pre-BL-110 a save followed
// by a backlinks refresh produced two paint commits, now one per
// frame.
//
// Phase 4.3 follow-up — backlinks now come from
// `nexus.noteContext`'s shared `useBacklinksDataStore` (populated by
// the always-on subscriber in `backlinksLoader.ts`) rather than the
// retired `nexus.backlinks` plugin's store. Same selectors, different
// owner.
const FILE_STATS_ENTRIES = [
  snap(useEditorStore, (s) => s.tabs),
  snap(useEditorStore, (s) => s.activeRelpath),
  snap(useBacklinksDataStore, (s) => s.links.length),
  snap(useBacklinksDataStore, (s) => s.loading),
] as const

export function FileStats() {
  const [tabs, activeRelpath, backlinksCount, backlinksLoading] =
    useFrameSnapshot(FILE_STATS_ENTRIES)

  const activeTab = useMemo(
    () => tabs.find((t) => t.relpath === activeRelpath) ?? null,
    [tabs, activeRelpath],
  )

  if (!activeTab || activeTab.loading || activeTab.error) return null

  const ext = fileExt(activeTab.name)
  const content = activeTab.content
  const words = content.trim() ? content.trim().split(/\s+/).length : 0
  const chars = content.length

  const backlinksLabel = backlinksLoading
    ? '… backlinks'
    : `${backlinksCount} backlinks`

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
      <span style={SEP_STYLE}>|</span>
      <span>{backlinksLabel}</span>
    </span>
  )
}
