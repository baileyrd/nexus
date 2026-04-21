import { useMemo } from 'react'
import { useEditorStore } from '../editor/editorStore'
import { useBacklinksStore } from '../backlinks/backlinksStore'

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

export function FileStats() {
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const backlinksCount = useBacklinksStore((s) => s.links.length)
  const backlinksLoading = useBacklinksStore((s) => s.loading)

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
