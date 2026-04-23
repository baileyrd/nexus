// High-level UI helpers for the Nexus shell e2e harness.
//
// Everything here runs in the WDIO/node process and either drives the
// rendered WebView2 DOM (keyboard + CM6 contenteditable) or drops into the
// page context via `browser.execute(...)` to invoke kernel IPC directly.
//
// Why mix the two: some operations (open-a-file, save, close) are faster
// and more deterministic over IPC than driving the UI, but the spec's
// purpose is to exercise the real editor glue — so typing and undo go
// through actual keyboard input.

import fs from 'node:fs/promises'
import path from 'node:path'
import { Key } from 'webdriverio'

// Cold-boot on CI (fresh Tauri debug binary, no OS page cache, shared
// runners) is dramatically slower than a warm local rebuild. Give it
// headroom via env override; default stays tight for local dev.
const BOOT_TIMEOUT_MS = Number(process.env.NEXUS_E2E_BOOT_TIMEOUT_MS ?? 30_000)

/** Open the e2e vault and wait for the workspace to be ready.
 *
 * The kernel is pre-booted by the Rust `setup` hook when NEXUS_E2E_VAULT
 * is set (see shell/src-tauri/src/lib.rs), and `last_forge_path` is
 * persisted so the launcher's restore path points at the e2e vault.
 * But the workspace plugin's auto-restore runs asynchronously during
 * plugin activation — rather than race it, we drive the same command
 * the launcher's "Open recent" row fires.
 *
 * The initial navigation also needs a kick: tauri-driver hands the
 * WebView to msedgedriver at about:blank and Tauri's own bundle load
 * never fires. Navigating once via `browser.url('http://tauri.localhost/')`
 * triggers the asset protocol (asset embedding requires the
 * `custom-protocol` cargo feature — see shell/src-tauri/Cargo.toml).
 */
export async function openVault(vaultAbsPath: string): Promise<void> {
  // 0) Force the initial navigation. tauri-driver leaves us at about:blank.
  const currentUrl = await browser.getUrl()
  if (!currentUrl.startsWith('http://tauri.localhost')) {
    await browser.url('http://tauri.localhost/')
  }

  // 1) Wait for the shell's plugin API — signals `boot()` in main.tsx
  //    has run and plugins are activated.
  await browser.waitUntil(
    async () =>
      browser.execute(() => {
        const w = window as unknown as { __nexusShellApi?: unknown }
        return Boolean(w.__nexusShellApi)
      }),
    { timeout: BOOT_TIMEOUT_MS, timeoutMsg: '__nexusShellApi never attached' },
  )

  // 2) Wait for the kernel to report booted.
  await browser.waitUntil(
    async () =>
      browser.execute(async () => {
        const api = (window as unknown as { __nexusShellApi?: {
          kernel?: { available: () => Promise<boolean> }
        } }).__nexusShellApi
        if (!api?.kernel) return false
        try {
          return await api.kernel.available()
        } catch {
          return false
        }
      }),
    { timeout: BOOT_TIMEOUT_MS, timeoutMsg: 'kernel never reported booted' },
  )

  // 3) Drive the same command the launcher uses — idempotent if the
  //    workspace plugin's auto-restore already fired. Also flip two
  //    editor configs so specs can drive keystrokes directly:
  //    - defaultMode=source: markdown files open straight into a CM6
  //      instance rather than the preview renderer.
  //    - confirmCloseDirty=false: closeTab() silently discards the
  //      buffer instead of stalling on a modal confirm dialog.
  await browser.execute(async (vault: string) => {
    const api = (window as unknown as { __nexusShellApi?: {
      commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
      configuration?: { setValue: (key: string, value: unknown) => void | Promise<void> }
    } }).__nexusShellApi
    if (!api?.commands) throw new Error('shell plugin API missing commands registry')
    await api.commands.execute('nexus.workspace.setRoot', vault)
    if (api.configuration) {
      await api.configuration.setValue('nexus.editor.defaultMode', 'source')
      await api.configuration.setValue('nexus.editor.confirmCloseDirty', false)
    }
  }, vaultAbsPath)
}

/** Open a file in the editor via the `files:open` event bus contract. */
export async function openFile(relpath: string): Promise<void> {
  await browser.execute((rel: string) => {
    const api = (window as unknown as { __nexusShellApi?: {
      events?: { emit: (topic: string, payload: unknown) => void }
    } }).__nexusShellApi
    if (!api?.events) throw new Error('shell plugin API not on window')
    const name = rel.split('/').pop() ?? rel
    api.events.emit('files:open', { relpath: rel, name })
  }, relpath)

  // If the tab lands in preview mode, flip it to source so the keystroke
  // helpers below target a real CodeMirror instance. openVault() sets
  // `nexus.editor.defaultMode=source`, but that preference is read only
  // when a NEW tab is opened — reopening an already-known file can land
  // in whatever mode was last active.
  await browser.waitUntil(
    async () => {
      const hasCm = await $('.cm-content').isExisting()
      if (hasCm) return true
      const editBtn = await $('button[aria-label="Edit"][title="Edit"]')
      if (await editBtn.isExisting()) {
        await editBtn.click()
      }
      return false
    },
    { timeout: 15_000, timeoutMsg: 'editor never reached source mode' },
  )
}

export async function focusEditor(): Promise<void> {
  const cm = await $('.cm-content')
  await cm.click()
}

export async function typeInEditor(text: string): Promise<void> {
  await focusEditor()
  await browser.keys(text.split(''))
}

/** Save the active tab. Routes through the editor plugin's command
 *  rather than a Ctrl+S key chord — WebDriver-injected modifier chords
 *  get swallowed by the shell's global keybinding dispatcher before
 *  they reach the CodeMirror scope. */
export async function save(): Promise<void> {
  await browser.execute(async () => {
    const api = (window as unknown as { __nexusShellApi?: {
      commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
    } }).__nexusShellApi
    if (!api?.commands) throw new Error('shell plugin API missing commands')
    await api.commands.execute('nexus.editor.save')
  })
}

/** Undo inside the active CodeMirror instance. Uses a key chord — undo
 *  is a CM6 binding, not a plugin command, so the command-API detour
 *  used for save/close doesn't apply. */
export async function undo(): Promise<void> {
  await focusEditor()
  const mod = process.platform === 'darwin' ? Key.Command : Key.Ctrl
  await browser.keys([mod, 'z'])
  await browser.keys([mod])
}

/** Close the active tab via the editor plugin's command. */
export async function closeTab(): Promise<void> {
  await browser.execute(async () => {
    const api = (window as unknown as { __nexusShellApi?: {
      commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
    } }).__nexusShellApi
    if (!api?.commands) throw new Error('shell plugin API missing commands')
    await api.commands.execute('nexus.editor.closeTab')
  })
}

/** Re-open a file that was just closed. */
export async function reopen(relpath: string): Promise<void> {
  await openFile(relpath)
}

export async function readEditorText(): Promise<string> {
  const cm = await $('.cm-content')
  return cm.getText()
}

/** Read a file from the test vault on the node side. */
export async function readSavedFile(
  vaultRoot: string,
  relpath: string,
): Promise<string> {
  return fs.readFile(path.join(vaultRoot, relpath), 'utf-8')
}

/** Copy the immutable fixture vault into a scratch dir so tests can mutate. */
export async function cloneVaultTo(
  fixtureRoot: string,
  scratchRoot: string,
): Promise<void> {
  await fs.rm(scratchRoot, { recursive: true, force: true })
  await fs.mkdir(scratchRoot, { recursive: true })
  await copyDir(fixtureRoot, scratchRoot)
}

async function copyDir(src: string, dst: string): Promise<void> {
  await fs.mkdir(dst, { recursive: true })
  for (const entry of await fs.readdir(src, { withFileTypes: true })) {
    const s = path.join(src, entry.name)
    const d = path.join(dst, entry.name)
    if (entry.isDirectory()) await copyDir(s, d)
    else if (entry.isFile()) await fs.copyFile(s, d)
  }
}
