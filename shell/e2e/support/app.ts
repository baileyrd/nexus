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

/** Wait for the workspace view to be ready.
 *
 * The kernel is pre-booted by the Rust `setup` hook when NEXUS_E2E_VAULT
 * is set (see shell/src-tauri/src/lib.rs), and the nexus.workspace
 * plugin restores the root from shell-state's lastForgePath. By the
 * time the shell API is attached and `.cm-editor` is mountable, the
 * workspace is open and ready for file-open events. No webdriver-side
 * invoke() calls are needed — which is the whole point of this path,
 * since Tauri v2 + tauri-driver BiDi rejects webdriver-injected invokes
 * with "Origin header is not a valid URL".
 */
export async function openVault(_vaultAbsPath: string): Promise<void> {
  // 0) Force navigation. tauri-driver hands the WebView to msedgedriver at
  //    about:blank; Tauri's own navigation never fires while WebDriver owns
  //    the context. On Windows WebView2, Tauri's embedded bundle is served
  //    from http://tauri.localhost/ (index.html at the root).
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

  // 2) Wait for the kernel to report booted — pure in-renderer `invoke`
  //    via @tauri-apps/api/core works because the call originates from
  //    the page's own JS context (not a webdriver-injected script with
  //    a bad Origin).
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

  // 3) Wait for the workspace store to reflect an open root. The
  //    workspace plugin emits `workspace:opened` at the end of setRoot;
  //    by the time the context key flips on, downstream plugins
  //    (files, editor) have registered their commands.
  await browser.waitUntil(
    async () =>
      browser.execute(() => {
        const root = document.documentElement
        // nexus.workspace sets this context key; bodyClasses mirrors
        // nothing for it, so fall back to the launcher overlay being
        // gone as the ready signal.
        const overlay = document.querySelector(
          '[data-view-id="nexus.launcher.view"]',
        )
        return !overlay || root.getAttribute('data-workspace-ready') === 'true'
      }),
    { timeout: 30_000, timeoutMsg: 'launcher overlay never cleared' },
  ).catch(() => {
    // Non-fatal: a future view-id rename would break the heuristic
    // above. The subsequent openFile() will fail loudly if the
    // workspace really isn't ready, so don't block the happy path.
  })
}

/** Open a file in the editor via the `files:open` event bus contract. */
export async function openFile(relpath: string): Promise<void> {
  await browser.execute((rel: string) => {
    const api = (window as unknown as { __nexusShellApi?: {
      events?: { emit: (topic: string, payload: unknown) => void }
    } }).__nexusShellApi
    if (!api?.events) throw new Error('shell plugin API not on window')
    api.events.emit('files:open', { path: rel })
  }, relpath)

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
