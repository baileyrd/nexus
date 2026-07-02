// C1 (#354) — clipboard-paste and drag-drop attachment importer.
//
// A CM6 extension that intercepts `paste` events carrying files
// (screenshots, copied images, files copied from an OS file manager)
// and DOM `drop` events carrying `dataTransfer.files`, writes the
// bytes into the forge via `com.nexus.storage::write_file` (location
// honours the Files & Links settings — see `attachmentDirFor`), and
// inserts the matching markdown (`![](path)` for images,
// `[name](path)` otherwise) at the paste/drop position.
//
// Known platform caveat: with Tauri v2's default `dragDropEnabled`,
// OS-file drops are intercepted by the webview and surface as
// `onDragDropEvent` *paths* (not DOM drops), and reading those
// absolute paths needs a binary host-fs primitive the shell doesn't
// expose yet — so native-shell OS drops are a follow-up on #354.
// Clipboard paste works everywhere, including the packaged shell.

import { EditorView } from '@codemirror/view'
import type { Extension } from '@codemirror/state'
import type { KernelAPI } from '../../../../types/plugin.ts'
import {
  attachmentDirFor,
  attachmentMarkdown,
  pastedImageName,
  writeAttachment,
} from '../attachments'

export interface AttachmentPasteOptions {
  /** Relpath of the note being edited — anchors the `same`-folder
   *  attachment-location mode. */
  relpath: string
  kernel: KernelAPI
  /** Surfaced on write failures (wired to the runtime's bridge-error
   *  reporter). Absent in test drivers → console.error. */
  onError?: (message: string, err: unknown) => void
}

/**
 * Pull attachment-worthy `File`s out of a clipboard/drag payload.
 * Plain-text pastes (`files` empty) return `[]` so the default CM
 * paste handling runs untouched. Exported for tests.
 */
export function filesFromDataTransfer(dt: DataTransfer | null): File[] {
  if (!dt) return []
  return Array.from(dt.files ?? [])
}

/** Name to store a pasted/dropped file under. Clipboard screenshots
 *  arrive as a generic `image.png` (Chromium) or unnamed blob — those
 *  get the timestamped pasted-image name; real file names are kept
 *  (sanitized + deduplicated downstream). Exported for tests. */
export function attachmentNameFor(file: File, now: Date): string {
  const generic =
    file.name.length === 0 || /^image\.(png|jpe?g|gif|webp)$/i.test(file.name)
  if (generic && file.type.startsWith('image/')) {
    return pastedImageName(file.type, now)
  }
  return file.name.length > 0 ? file.name : pastedImageName(file.type, now)
}

async function importFiles(
  view: EditorView,
  pos: number,
  files: File[],
  opts: AttachmentPasteOptions,
): Promise<void> {
  const dir = attachmentDirFor(opts.relpath)
  const snippets: string[] = []
  for (const file of files) {
    try {
      const bytes = new Uint8Array(await file.arrayBuffer())
      const name = attachmentNameFor(file, new Date())
      const relpath = await writeAttachment(opts.kernel, dir, name, bytes)
      snippets.push(attachmentMarkdown(relpath))
    } catch (err) {
      if (opts.onError) opts.onError(`attachment import (${file.name || file.type})`, err)
      else console.error('attachment import failed', err)
    }
  }
  if (snippets.length === 0) return
  const insert = snippets.join('\n')
  const clamped = Math.min(pos, view.state.doc.length)
  view.dispatch({
    changes: { from: clamped, to: clamped, insert },
    selection: { anchor: clamped + insert.length },
    scrollIntoView: true,
  })
  view.focus()
}

/**
 * The extension. Attached to markdown editor tabs (live + source);
 * text-only pastes and internal drags (file-tree / block-ref MIME
 * payloads carry no `files`) fall through to CM's defaults.
 */
export function attachmentPasteExt(opts: AttachmentPasteOptions): Extension {
  return EditorView.domEventHandlers({
    paste(event, view) {
      const files = filesFromDataTransfer(event.clipboardData)
      if (files.length === 0) return false
      event.preventDefault()
      void importFiles(view, view.state.selection.main.head, files, opts)
      return true
    },
    dragover(event) {
      // Signal droppability for OS file payloads so the drop event
      // fires; internal drags keep their existing handling.
      if (filesFromDataTransfer(event.dataTransfer).length === 0 &&
          !event.dataTransfer?.types.includes('Files')) {
        return false
      }
      event.preventDefault()
      return true
    },
    drop(event, view) {
      const files = filesFromDataTransfer(event.dataTransfer)
      if (files.length === 0) return false
      event.preventDefault()
      const pos =
        view.posAtCoords({ x: event.clientX, y: event.clientY }) ??
        view.state.selection.main.head
      void importFiles(view, pos, files, opts)
      return true
    },
  })
}
