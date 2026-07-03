// C66 (#419) — regression coverage for the new "Export as HTML…" tab-actions
// menu entry: it must appear wired (no "coming soon" tooltip) from the start.
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { buildTabContextMenu } from './TabContextMenu.tsx'

function flatten(items: ReturnType<typeof buildTabContextMenu>): ReturnType<typeof buildTabContextMenu> {
  const out: ReturnType<typeof buildTabContextMenu> = []
  for (const item of items) {
    out.push(item)
    if (item.kind === 'item' && item.submenu) out.push(...flatten(item.submenu))
  }
  return out
}

function findItem(commandId: string) {
  const items = flatten(buildTabContextMenu({ mode: 'source', isUntitled: false }))
  const found = items.find((i) => i.kind === 'item' && i.commandId === commandId)
  assert.ok(found, `expected a menu item with commandId ${commandId}`)
  return found
}

test('export as HTML is present and wired — no coming-soon tooltip', () => {
  const item = findItem('nexus.editor.exportHtml')
  assert.equal(item.label, 'Export as HTML...')
  assert.equal(item.tooltip, undefined)
})

test('menu still carries genuinely unwired stubs with the coming-soon tooltip', () => {
  const item = findItem('nexus.editor.stub.rename')
  assert.equal(item.tooltip, 'Coming soon')
})
