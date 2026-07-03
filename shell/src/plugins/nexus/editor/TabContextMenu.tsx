import type { ContextMenuItem } from '../../../shell/ContextMenu'
import type { EditorTabMode } from './editorStore'

// Placeholder commands. Each one is registered in editor/index.ts and
// fires a "Coming soon" notification — clicking gives feedback instead
// of leaving the user with a greyed-out row that looks broken.
const COMING_SOON_TOOLTIP = 'Coming soon'

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
    {
      kind: 'item',
      label: 'Split right',
      commandId: 'nexus.editor.stub.splitRight',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Split down',
      commandId: 'nexus.editor.stub.splitDown',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Open in new window',
      commandId: 'nexus.editor.stub.openInNewWindow',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Open linked view',
      commandId: 'nexus.editor.stub.openLinkedView',
      tooltip: COMING_SOON_TOOLTIP,
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
      commandId: 'nexus.editor.stub.rename',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Move file to...',
      commandId: 'nexus.editor.stub.moveTo',
      tooltip: COMING_SOON_TOOLTIP,
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
      commandId: 'nexus.editor.stub.bookmark',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Add file property',
      commandId: 'nexus.editor.stub.addProperty',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Backlinks in document',
      commandId: 'nexus.editor.stub.backlinksInDocument',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Open version history',
      commandId: 'nexus.editor.stub.versionHistory',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Merge entire file with...',
      commandId: 'nexus.editor.stub.mergeFile',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      kind: 'item',
      label: 'Export to PDF...',
      commandId: 'nexus.editor.stub.exportPdf',
      tooltip: COMING_SOON_TOOLTIP,
    },
    {
      // C66 (#419) — wired via com.nexus.formats::export_html + save dialog.
      kind: 'item',
      label: 'Export as HTML...',
      commandId: 'nexus.editor.exportHtml',
    },
  ]
}
