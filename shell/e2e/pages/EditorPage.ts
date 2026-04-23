// Page object for the editor surface.
//
// This is a thin wrapper around the existing support/app.ts helpers
// (which the golden-path spec already uses); specs in tier1/ should
// prefer EditorPage so the selectors live in one place going forward.

import { Key } from 'webdriverio'
import {
  closeTab as _closeTab,
  focusEditor as _focusEditor,
  openFile as _openFile,
  readEditorText as _readEditorText,
  save as _save,
  typeInEditor as _typeInEditor,
  undo as _undo,
} from '../support/app.js'

export class EditorPage {
  static async openNote(relpath: string): Promise<void> {
    await _openFile(relpath)
  }

  static async focus(): Promise<void> {
    await _focusEditor()
  }

  static async type(text: string): Promise<void> {
    await _typeInEditor(text)
  }

  static async save(): Promise<void> {
    await _save()
  }

  static async closeTab(): Promise<void> {
    await _closeTab()
  }

  static async undo(): Promise<void> {
    await _undo()
  }

  static async readText(): Promise<string> {
    return _readEditorText()
  }

  /** Clear the editor buffer with Select-All + Delete. */
  static async selectAllAndDelete(): Promise<void> {
    await _focusEditor()
    const mod = process.platform === 'darwin' ? Key.Command : Key.Ctrl
    await browser.keys([mod, 'a'])
    await browser.keys([mod])
    await browser.keys(['Delete'])
  }

  /** Wait for a CodeMirror instance to exist. */
  static async waitForMounted(timeoutMs = 15_000): Promise<void> {
    const cm = await $('.cm-content')
    await cm.waitForExist({ timeout: timeoutMs })
  }
}
