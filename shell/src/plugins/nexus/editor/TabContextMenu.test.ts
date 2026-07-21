// C65 (#418) / C66 (#419) — regression coverage for the tab-actions menu:
// both export commands must appear with no "coming soon" tooltip now that
// they're fully wired (scoped print stylesheet / com.nexus.formats IPC).
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

test('export to PDF is wired — no coming-soon tooltip', () => {
  const item = findItem('nexus.editor.stub.exportPdf')
  assert.equal(item.label, 'Export to PDF...')
  assert.equal(item.tooltip, undefined)
})

test('export as HTML is present and wired — no coming-soon tooltip', () => {
  const item = findItem('nexus.editor.exportHtml')
  assert.equal(item.label, 'Export as HTML...')
  assert.equal(item.tooltip, undefined)
})

test('copy as rich text is present and wired — no coming-soon tooltip', () => {
  const item = findItem('nexus.editor.copyAsRichText')
  assert.equal(item.label, 'Copy as Rich Text')
  assert.equal(item.tooltip, undefined)
})

test('menu still carries genuinely unwired stubs with the coming-soon tooltip', () => {
  const item = findItem('nexus.editor.stub.rename')
  assert.equal(item.tooltip, 'Coming soon')
})
