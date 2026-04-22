// WebdriverIO config for the Nexus shell Tauri app.
//
// Prereqs (see e2e/README.md):
//   - `cargo install tauri-driver --locked` (Windows-only; wraps WebView2 via
//     msedgedriver).
//   - msedgedriver on PATH, version matching the installed Edge WebView2.
//   - `pnpm e2e:build` in shell/ to produce the debug binary at
//     src-tauri/target/debug/nexus-shell.exe.
//
// The harness launches tauri-driver as a child process in `onPrepare` and
// tears it down in `onComplete`. tauri-driver itself spawns the application
// binary named by `tauri:options.application`.
//
// Vault seeding: Tauri has no env-var hook for forge_root (the shell picks
// via a launcher dialog or restores `rootPath` from WebView2 localStorage
// under the nexus.workspace plugin's storage). Since we can't pre-seed
// WebView2 localStorage before launch, the `before` hook waits for the app
// to boot, then drives `api.kernel.invoke('nexus.workspace', 'setRoot', ...)`
// via `browser.execute(...)` — which is exactly what the launcher does when
// the user clicks a recents row. See e2e/support/app.ts `openVault()`.

import { spawn, type ChildProcess } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { cloneVaultTo } from './support/app.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

const SHELL_ROOT = path.resolve(__dirname, '..')
const TAURI_BIN = path.join(
  SHELL_ROOT,
  'src-tauri',
  'target',
  'debug',
  process.platform === 'win32' ? 'nexus-shell.exe' : 'nexus-shell',
)

export const VAULT_FIXTURE = path.resolve(__dirname, 'fixtures', 'vault')
/** Scratch dir that `onPrepare` clones the fixture vault into. Tauri's
 *  setup hook reads `NEXUS_E2E_VAULT` (set on the child env below) and
 *  pre-boots the kernel against this path — sidestepping the webdriver
 *  BiDi + Tauri v2 IPC incompatibility for the vault-setup step. */
export const SCRATCH_VAULT = path.join(os.tmpdir(), 'nexus-e2e-vault')

let tauriDriver: ChildProcess | null = null

export const config: WebdriverIO.Config = {
  runner: 'local',
  tsConfigPath: path.resolve(__dirname, 'tsconfig.json'),

  specs: [path.resolve(__dirname, 'specs', '**', '*.spec.ts')],
  maxInstances: 1,

  capabilities: [
    {
      maxInstances: 1,
      'tauri:options': {
        application: TAURI_BIN,
        // Belt-and-suspenders: tauri-driver will launch the binary with
        // VITE_E2E=true in its environment. Note that `import.meta.env.VITE_E2E`
        // is resolved at VITE BUILD time, not runtime — so the authoritative
        // flag path is `pnpm e2e:build` (which sets VITE_E2E=true before
        // `vite build`). This env is here so any future Rust-side / runtime
        // check (e.g. `invoke('is_e2e')`) can read it from `std::env`.
        env: { VITE_E2E: 'true', NEXUS_E2E_VAULT: SCRATCH_VAULT },
      },
      // Tauri's official WebdriverIO example uses `wry` — the engine name
      // tauri-driver gates on. `webview2` (a msedgedriver alias) leaves the
      // session attached to about:blank with no initial navigation.
      browserName: 'wry',
    } as WebdriverIO.Capabilities,
  ],

  logLevel: 'info',
  bail: 0,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 60_000,
  connectionRetryCount: 1,

  hostname: 'localhost',
  port: 4444,
  path: '/',

  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 120_000,
  },

  async onPrepare() {
    // Clone the fixture vault into the scratch dir BEFORE spawning
    // tauri-driver — the Rust `setup` hook reads NEXUS_E2E_VAULT at
    // app startup and expects the directory to exist.
    await cloneVaultTo(VAULT_FIXTURE, SCRATCH_VAULT)

    // Spawn tauri-driver. Assumes it's on PATH; users install via
    // `cargo install tauri-driver --locked`.
    // tauri-driver defaults to listening on :4444 and forwards to
    // msedgedriver on :17556 (or a free port).
    tauriDriver = spawn('tauri-driver', [], {
      stdio: ['ignore', 'inherit', 'inherit'],
      shell: process.platform === 'win32',
    })
    tauriDriver.on('error', (err) => {
      console.error('[tauri-driver] failed to spawn:', err)
    })
    // Small grace period so the driver's HTTP listener is bound before
    // WDIO tries to connect. tauri-driver itself reports when ready on
    // stderr; 1s is empirically enough on Windows.
    return new Promise((resolve) => setTimeout(resolve, 1_500))
  },

  onComplete() {
    if (tauriDriver && !tauriDriver.killed) {
      tauriDriver.kill()
      tauriDriver = null
    }
  },
}
