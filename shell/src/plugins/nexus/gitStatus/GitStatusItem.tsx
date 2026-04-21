import { useGitStatusStore } from './gitStatusStore'

const NO_COMMITS_SENTINEL = '(none)'

export function GitStatusItem() {
  const status = useGitStatusStore((s) => s.status)
  if (!status) return null
  const hasHead = status.head && status.head !== NO_COMMITS_SENTINEL
  const parts: string[] = []
  if (status.branch) parts.push(status.branch)
  if (hasHead) parts.push(status.head)
  if (parts.length === 0) return null
  const label = parts.join(' · ')
  const tooltip = [
    status.branch ?? '(detached)',
    hasHead ? status.head : '(no commits)',
    status.is_dirty ? 'uncommitted changes' : 'clean',
  ].join(' — ')
  return (
    <span
      title={tooltip}
      style={{
        padding: '0 8px',
        opacity: status.is_dirty ? 1 : 0.8,
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      {label}
      {status.is_dirty && <span style={{ marginLeft: 4 }}>●</span>}
    </span>
  )
}
