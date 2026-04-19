// Renders status-bar items from the registry. Items may carry rich
// `content` (React node) or plain `text`; `className` is appended to
// the root so Forge-flavored modifiers like `ember` apply.

import type { StatusBarItem } from '../../../registry/StatusBarRegistry'
import { useStatusBarStore } from '../../../registry/StatusBarRegistry'
import { getRegistry } from '../../../host/shellRegistry'

export function StatusBarLeft() {
  const items = useStatusBarStore(s => s.items)
  return (
    <>
      {items.filter(i => i.slot === 'left').map(item => (
        <StatusBarItemView key={item.id} item={item} />
      ))}
    </>
  )
}

export function StatusBarRight() {
  const items = useStatusBarStore(s => s.items)
  return (
    <>
      {items.filter(i => i.slot === 'right').map(item => (
        <StatusBarItemView key={item.id} item={item} />
      ))}
    </>
  )
}

function StatusBarItemView({ item }: { item: StatusBarItem }) {
  const handleClick = () => {
    if (item.command) getRegistry()?.commands.execute(item.command)
  }
  const classes = [
    'status-bar-item',
    item.command ? 'status-bar-item--clickable' : '',
    item.className ?? '',
  ].filter(Boolean).join(' ')

  return (
    <span
      className={classes}
      onClick={handleClick}
      title={item.tooltip ?? item.text}
    >
      {item.content ?? item.text}
    </span>
  )
}
