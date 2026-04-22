// High-level UI helpers for the Nexus shell e2e harness.
//
// Everything here runs in the WDIO/node process and either drives the
// rendered WebView2 DOM (keyboard + CM6 contenteditable) or drops into the
// page context via `browser.execute(...)` to invoke kernel IPC directly.
//
// Why mix the two: some operations (open-a-file) are faster & more
// deterministic over IPC than driving the file-tree UI, but the spec's
// purpose is to exercise the real editor glue — so saves, undos, and text
// edits go through actual keyboard input.

import fs from 'node:fs/promises'
import path from 'node:path'
import { Key } from 'webdriverio'

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
    { timeout: 30_000, timeoutMsg: '__nexusShellApi never attached' },
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
    { timeout: 30_000, timeoutMsg: 'kernel never reported booted' },
  )

  // 3) Drive the same command the launcher uses — idempotent if the
  //    workspace plugin's auto-restore already fired.
  await browser.execute(async (vault: string) => {
    const api = (window as unknown as { __nexusShellApi?: {
      commands?: { execute: (id: string, ...args: unknown[]) => Promise<unknown> }
    } }).__nexusShellApi
    if (!api?.commands) throw new Error('shell plugin API missing commands registry')
    await api.commands.execute('nexus.workspace.setRoot', vault)
  }, vaultAbsPath)
}

/** Open a file in the editor via the `files:open` event bus contract. */
export async function openFile(relpath: string): Promise<void> {
  const before = await browser.execute(() => ({
    ribbonBtns: document.querySelectorAll('.workspace-ribbon button').length,
    mainRegionHtmlLen: document.querySelector('.workspace-main-region')?.innerHTML?.length ?? -1,
    cmCount: document.querySelectorAll('.cm-content').length,
    bodyLen: document.body?.innerHTML?.length ?? 0,
  }))
  console.log('[e2e-debug] pre-openFile', JSON.stringify(before))

  const emitResult = await browser.execute((rel: string) => {
    const api = (window as unknown as { __nexusShellApi?: {
      events?: { emit: (topic: string, payload: unknown) => void; listenerCount?: (topic: string) => number }
    } }).__nexusShellApi
    if (!api?.events) throw new Error('shell plugin API not on window')
    const name = rel.split('/').pop() ?? rel
    const listeners = api.events.listenerCount?.('files:open') ?? 'n/a'
    api.events.emit('files:open', { relpath: rel, name })
    return { listeners, name }
  }, relpath)
  console.log('[e2e-debug] openFile emit', JSON.stringify(emitResult))

  // Give the plugin handler a beat, then probe again.
  await browser.pause(2000)
  const after = await browser.execute(() => {
    const classes = new Set<string>()
    document.querySelectorAll('[class]').forEach((el) => {
      el.className.toString().split(/\s+/).forEach((c) => {
        if (c.startsWith('cm-') || c.startsWith('nexus-editor') || c.startsWith('markdown-')) classes.add(c)
      })
    })
    const leaf = document.querySelector('.workspace-leaf')
    return {
      cmCount: document.querySelectorAll('.cm-content').length,
      leafCount: document.querySelectorAll('.workspace-leaf').length,
      tabCount: document.querySelectorAll('.workspace-tab-header').length,
      editorClasses: Array.from(classes),
      leafInner: leaf?.innerHTML?.slice(0, 600) ?? '(no leaf)',
    }
  })
  console.log('[e2e-debug] post-openFile', JSON.stringify(after))

  // Wait for CM6 to mount. The editor creates a `.cm-content` contenteditable
  // per open file; the active one takes focus.
  await $('.cm-content').waitForExist({ timeout: 15_000 })
}

export async function focusEditor(): Promise<void> {
  const cm = await $('.cm-content')
  await cm.click()
}

export async function typeInEditor(text: string): Promise<void> {
  await focusEditor()
  await browser.keys(text.split(''))
}

/** Send Ctrl+S (Cmd+S on macOS). */
export async function save(): Promise<void> {
  const mod = process.platform === 'darwin' ? Key.Command : Key.Ctrl
  await browser.keys([mod, 's'])
  await browser.keys([mod]) // key-up
}

export async function undo(): Promise<void> {
  const mod = process.platform === 'darwin' ? Key.Command : Key.Ctrl
  await browser.keys([mod, 'z'])
  await browser.keys([mod])
}

export async function closeTab(): Promise<void> {
  const mod = process.platform === 'darwin' ? Key.Command : Key.Ctrl
  await browser.keys([mod, 'w'])
  await browser.keys([mod])
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
