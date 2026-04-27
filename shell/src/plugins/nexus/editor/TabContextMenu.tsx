import type { ContextMenuItem } from '../../../shell/ContextMenu'
import type { EditorTabMode } from './editorStore'

const NOT_YET = 'Not yet implemented'

export function buildTabContextMenu(args: {
  mode: EditorTabMode
  isUntitled: boolean
}): ContextMenuItem[] {
  const { mode, isUntitled } = args
  // Two adjacent toggles:
  //   - Source ↔ Live: edit-style flip. Label tracks the destination.
  //   - Reading view: route into / out of rendered preview.
  const sourceToggleLabel = mode === 'source' ? 'Live preview' : 'Edit source'
  const readingViewLabel = mode === 'preview' ? 'Edit' : 'Reading view'

  return [
    {
      kind: 'item',
      label: sourceToggleLabel,
      commandId: 'nexus.editor.toggleMode',
    },
    {
      kind: 'item',
      label: readingViewLabel,
      commandId: 'nexus.editor.toggleReadingView',
    },
    { kind: 'separator' },
    // Deferred: needs a `workspace.splitLeaf(direction)` API on workspaceStore that doesn't exist yet; out of scope for this menu.
    {
      kind: 'item',
      label: 'Split right',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Split down',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Open in new window',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Open linked view',
      disabled: true,
      tooltip: NOT_YET,
    },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Find...',
      commandId: 'nexus.editor.find',
    },
    {
      kind: 'item',
      label: 'Replace...',
      commandId: 'nexus.editor.replace',
    },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Rename...',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Move file to...',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Delete file',
      commandId: 'nexus.editor.deleteFile',
    },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Copy path',
      submenu: [
        {
          kind: 'item',
          label: 'Relative path',
          commandId: 'nexus.editor.copyRelativePath',
          disabled: isUntitled,
          tooltip: isUntitled ? 'Untitled tabs have no path' : undefined,
        },
        {
          kind: 'item',
          label: 'Absolute path',
          commandId: 'nexus.editor.copyAbsolutePath',
          disabled: isUntitled,
          tooltip: isUntitled ? 'Untitled tabs have no path' : undefined,
        },
      ],
    },
    {
      kind: 'item',
      label: 'Reveal file in navigation',
      commandId: 'nexus.editor.revealInNavigation',
      disabled: isUntitled,
      tooltip: isUntitled ? 'Untitled tabs have no file in navigation' : undefined,
    },
    {
      kind: 'item',
      label: 'Show in system explorer',
      commandId: 'nexus.editor.revealInOS',
      disabled: isUntitled,
      tooltip: isUntitled ? 'Untitled tabs have no on-disk path' : undefined,
    },
    {
      kind: 'item',
      label: 'Open in default app',
      commandId: 'nexus.editor.openInDefaultApp',
      disabled: isUntitled,
      tooltip: isUntitled ? 'Untitled tabs have no on-disk path' : undefined,
    },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Bookmark...',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Add file property',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Backlinks in document',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Open version history',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Merge entire file with...',
      disabled: true,
      tooltip: NOT_YET,
    },
    {
      kind: 'item',
      label: 'Export to PDF...',
      disabled: true,
      tooltip: NOT_YET,
    },
  ]
}
