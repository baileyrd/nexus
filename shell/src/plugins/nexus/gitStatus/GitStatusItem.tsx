import { useGitStatusStore } from './gitStatusStore'

export function GitStatusItem() {
  const status = useGitStatusStore((s) => s.status)
  if (!status) return null
  const parts: string[] = []
  if (status.branch) parts.push(status.branch)
  if (status.shortSha) parts.push(status.shortSha)
  if (parts.length === 0) return null
  const label = parts.join(' · ')
  const tooltip = [
    status.branch ?? '(detached)',
    status.shortSha ?? '(no commits)',
    status.dirty ? 'uncommitted changes' : 'clean',
  ].join(' — ')
  return (
    <span
      title={tooltip}
      style={{
        padding: '0 8px',
        opacity: status.dirty ? 1 : 0.8,
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      {label}
      {status.dirty && <span style={{ marginLeft: 4 }}>●</span>}
    </span>
  )
}
