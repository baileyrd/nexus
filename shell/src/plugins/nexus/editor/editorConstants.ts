// R8 / #191 — module-level constants, configuration keys, command ids,
// stub-command metadata, and small pure helpers lifted out of
// `index.ts` (which exceeded 2,500 LoC) so the file stays focused on
// the plugin manifest + `activate()` wiring.
//
// Every symbol is exported with the same name it had inline; consumer
// sites in `index.ts` switch from bare-identifier references to an
// import here. No behavior changes — the values, the const block
// ordering, the doc comments are preserved verbatim.

import type { PluginAPI } from '../../../types/plugin'

export const EVENT_FILE_OPEN = 'files:open'
export const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'
// Verified against crates/nexus-storage/src/core_plugin.rs::dispatch:
//   HANDLER_READ_FILE  args `{ "path": String }` → `{ "bytes": Vec<u8> }`.
//   HANDLER_WRITE_FILE args `{ "path": String, "bytes": Vec<u8> }`
//                      → `FileMetadata` (ignored here).
// Arg key is `path`, NOT `relpath` (unlike `list_dir` / `create_file`).
// serde_json encodes `Vec<u8>` as a JSON number array, so we pass the
// UTF-8 bytes through `Array.from(new TextEncoder().encode(...))`.
export const READ_FILE_COMMAND = 'read_file'
export const WRITE_FILE_COMMAND = 'write_file'

export const COMMAND_CLOSE_TAB = 'nexus.editor.closeTab'
export const COMMAND_SAVE = 'nexus.editor.save'
export const COMMAND_NEW_UNTITLED = 'nexus.editor.newUntitled'
export const COMMAND_CLOSE_ALL = 'nexus.editor.closeAll'
export const COMMAND_TOGGLE_MODE = 'nexus.editor.toggleMode'
export const COMMAND_TOGGLE_READING_VIEW = 'nexus.editor.toggleReadingView'
export const COMMAND_FIND = 'nexus.editor.find'
export const COMMAND_REPLACE = 'nexus.editor.replace'
export const COMMAND_COPY_REL_PATH = 'nexus.editor.copyRelativePath'
export const COMMAND_COPY_ABS_PATH = 'nexus.editor.copyAbsolutePath'
export const COMMAND_REVEAL_IN_NAV = 'nexus.editor.revealInNavigation'
export const COMMAND_REVEAL_IN_OS = 'nexus.editor.revealInOS'
export const COMMAND_OPEN_DEFAULT_APP = 'nexus.editor.openInDefaultApp'
export const COMMAND_DELETE_FILE = 'nexus.editor.deleteFile'
// BL-079 — toggle the inline-blame annotations on the active tab.
// Reads / writes `useEditorBlameStore`; the editor view subscribes
// and remounts CM6 with / without the blame extension on flip.
export const COMMAND_TOGGLE_BLAME = 'nexus.editor.toggleBlame'
// BL-079 — open the modal diff viewer for the active tab. Reads
// `com.nexus.git::diff_file` and renders the hunks unified.
export const COMMAND_OPEN_DIFF = 'nexus.editor.openDiff'
// BL-141 Phase 3 — LSP find-references → multibuffer + LSP rename
// preview → multibuffer. Both surface LSP results in the new
// excerpt view shipped in BL-141 Phase 1/2 so the user can
// browse / preview before deciding to apply.
export const COMMAND_LSP_FIND_REFERENCES = 'nexus.editor.lsp.findReferences'
export const COMMAND_LSP_RENAME_PREVIEW = 'nexus.editor.lsp.renamePreview'

// BL-077 follow-up — LSP rename. Cursor-position rename across every
// file the symbol appears in; multi-file `WorkspaceEdit` applies via
// `applyWorkspaceEdit` (active tab through the live CM view, every
// other file through `com.nexus.storage::write_file`).
export const COMMAND_LSP_RENAME = 'nexus.editor.lsp.rename'
// BL-077 follow-up — LSP code actions. Cursor-position code-action
// menu surfaced via `api.input.pick`; a chosen action's
// `WorkspaceEdit` applies through the same `applyWorkspaceEdit`
// path as rename. Command-only actions (no `edit`) require the
// LSP `workspace/executeCommand` surface that the host doesn't
// expose yet — those are listed in the picker for transparency
// but with a disabled message.
export const COMMAND_LSP_CODE_ACTIONS = 'nexus.editor.lsp.codeActions'

// Tab-actions menu placeholders. Each one shows a "Coming soon"
// notification so users get feedback instead of a dead disabled row.
// Listed here so the manifest contributions and runtime registrations
// stay in sync.
export const STUB_COMMANDS: ReadonlyArray<{ id: string; title: string; label: string }> = [
  { id: 'nexus.editor.stub.splitRight', title: 'Split right', label: 'Split right' },
  { id: 'nexus.editor.stub.splitDown', title: 'Split down', label: 'Split down' },
  {
    id: 'nexus.editor.stub.openInNewWindow',
    title: 'Open in new window',
    label: 'Open in new window',
  },
  {
    id: 'nexus.editor.stub.openLinkedView',
    title: 'Open linked view',
    label: 'Open linked view',
  },
  { id: 'nexus.editor.stub.rename', title: 'Rename file', label: 'Rename' },
  { id: 'nexus.editor.stub.moveTo', title: 'Move file to…', label: 'Move file to' },
  { id: 'nexus.editor.stub.bookmark', title: 'Bookmark file', label: 'Bookmark' },
  {
    id: 'nexus.editor.stub.addProperty',
    title: 'Add file property',
    label: 'Add file property',
  },
  {
    id: 'nexus.editor.stub.backlinksInDocument',
    title: 'Backlinks in document',
    label: 'Backlinks in document',
  },
  {
    id: 'nexus.editor.stub.versionHistory',
    title: 'Open version history',
    label: 'Version history',
  },
  {
    id: 'nexus.editor.stub.mergeFile',
    title: 'Merge entire file with…',
    label: 'Merge entire file',
  },
  { id: 'nexus.editor.stub.exportPdf', title: 'Export to PDF…', label: 'Export to PDF' },
]
export const DELETE_FILE_HANDLER = 'delete_file'
export const CONTEXT_KEY_HAS_ACTIVE_TAB = 'nexus.editor.hasActiveTab'
export const CONTEXT_KEY_ACTIVE_TAB_DIRTY = 'nexus.editor.activeTabDirty'

// Configuration keys read by the editor at runtime via
// api.configuration.getValue. The Settings panel (core.settings) auto-
// generates UI from the schema we register in `activate`.
export const CONFIG_CONFIRM_CLOSE_DIRTY = 'nexus.editor.confirmCloseDirty'
export const CONFIG_DEFAULT_MODE = 'nexus.editor.defaultMode'
// BL-070: opt-in modal keybinding layer for the markdown editor.
export const CONFIG_KEYBINDINGS = 'nexus.editor.keybindings'
// BL-075: comma-separated list of file extensions that open in code
// mode (raw CM6 with a language extension) rather than document mode
// (markdown block tree). Read at file-open time.
export const CONFIG_CODE_FILE_EXTENSIONS = 'nexus.editor.codeFileExtensions'
// BL-139 — per-keystroke FIM edit prediction. Off by default per the
// BL DoD; the provider/model fields are informational (the actual
// route is decided by `com.nexus.ai`'s configured provider).
export const CONFIG_EDIT_PREDICTION_ENABLED = 'nexus.editor.editPrediction.enabled'
export const CONFIG_EDIT_PREDICTION_DEBOUNCE_MS = 'nexus.editor.editPrediction.debounceMs'
export const CONFIG_EDIT_PREDICTION_PROVIDER = 'nexus.editor.editPrediction.provider'
export const CONFIG_EDIT_PREDICTION_MODEL = 'nexus.editor.editPrediction.model'
// BL-142 Phase 2a — re-export the config key + default from
// `replKernels.ts` so the schema registration below stays in sync
// with the parser/resolver helpers.
import {
  CONFIG_REPL_KERNELS,
  REPL_KERNELS_DEFAULT_JSON,
} from './replKernels.ts'
// BL-142 Phase 2b.1 — tab-close teardown for REPL sessions.
import { makeReplClient } from './replClient.ts'
import { useReplStore } from './replStore.ts'
// BL-142 Phase 2b.2 — bus pump that routes
// `com.nexus.terminal.output.<sessionId>` events into the per-cell
// output buffer.
import { startReplOutputPump } from './replOutputPump.ts'
// BL-142 Phase 3 — Settings → REPL Kernels tab.
import { ReplKernelsTab } from './ReplKernelsTab'
// Visual settings — applied live via CSS custom properties on :root
// (see applyEditorCssVars below) and via prop flow to CodeMirrorHost.
export const CONFIG_FONT_SIZE = 'nexus.editor.fontSize'
export const CONFIG_LINE_HEIGHT = 'nexus.editor.lineHeight'
export const CONFIG_LINE_NUMBERS = 'nexus.editor.lineNumbers'
export const CONFIG_WORD_WRAP = 'nexus.editor.wordWrap'
export const CONFIG_TAB_SIZE = 'nexus.editor.tabSize'

export function applyEditorCssVars(api: PluginAPI): void {
  const root = document.documentElement
  const apply = () => {
    const fontSize = api.configuration.getValue<number>(CONFIG_FONT_SIZE, 13)
    const lineHeight = api.configuration.getValue<number>(CONFIG_LINE_HEIGHT, 1.6)
    root.style.setProperty('--editor-font-size', `${fontSize}px`)
    root.style.setProperty('--editor-line-height', String(lineHeight))
  }
  apply()
  api.configuration.onChange(CONFIG_FONT_SIZE, apply)
  api.configuration.onChange(CONFIG_LINE_HEIGHT, apply)
}

export interface FileOpenPayload {
  relpath: string
  name: string
}

export interface ReadFileResponse {
  /** Serde of `Vec<u8>` over JSON — arrives as a number[] of bytes. */
  bytes: number[]
}

/**
 * Decode a byte array response from `com.nexus.storage:read_file` as
 * UTF-8 text. Returns a human-readable sentinel for non-decodable
 * bytes so a binary file doesn't look like an error to the user.
 */
export function decodeUtf8(bytes: number[]): string {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array(bytes))
  } catch {
    return '(binary or non-UTF-8 file)'
  }
}
