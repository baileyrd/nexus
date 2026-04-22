# Nexus shell e2e harness

Headless WebDriver-driven end-to-end tests for the Tauri app in `shell/`.

## Status (2026-04-21)

Harness boots the app under tauri-driver, navigates, attaches the test
plugin API, opens the vault, and drives CodeMirror. `golden-path.spec.ts`
currently runs with **1/3 passing** (`edit + save + reopen roundtrip`).
The remaining two (`undo-across-save`, `close-without-save`) fail on
product-level semantics rather than harness bugs — see "Remaining spec
failures" below.

## Prereqs

Windows only. tauri-driver is a Windows-specific binary that wraps
`msedgedriver` to drive the WebView2 runtime Tauri uses on Windows.

1. **tauri-driver**
   ```
   cargo install tauri-driver --locked
   ```
2. **msedgedriver** — download the version matching your installed Edge /
   WebView2 (check `Get-AppxPackage *WebView*` or the Edge `edge://version`
   page). Extract `msedgedriver.exe` somewhere on `PATH`.
   Source: https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/
3. **Build the app in the e2e shape**:
   ```
   pnpm e2e:build
   ```
   That runs, in order:
   - `cross-env VITE_E2E=true pnpm build` — builds the frontend with
     `VITE_E2E=true` baked into `import.meta.env`, which triggers
     `src/main.tsx` to attach `window.__nexusShellApi` at boot.
   - `cargo build --features custom-protocol` under `src-tauri/`. The
     `custom-protocol` feature (defined in `src-tauri/Cargo.toml`) is the
     switch that makes Tauri 2 serve the embedded `frontendDist` via the
     asset protocol. Without it, `cargo build` leaves Tauri in dev-mode
     where it tries to load `devUrl` (which isn't running under e2e), and
     `http://tauri.localhost/` returns `asset not found: index.html`.
     `tauri build` passes this feature automatically; direct `cargo build`
     does not.

   The `VITE_E2E` flag is resolved at Vite build time, not runtime, so a
   normal `pnpm build` or `pnpm tauri:build` will NOT expose the global —
   the gate is inert in production bundles.

## Run

From `shell/`:

```
pnpm e2e
```

The harness:

1. Clones `e2e/fixtures/vault/` into `%TEMP%/nexus-e2e-vault/` so every run
   starts clean.
2. Spawns `tauri-driver` on port 4444.
3. Launches `src-tauri/target/debug/nexus-shell.exe` through the driver
   with `NEXUS_E2E_VAULT` in the environment.
4. The Rust `setup` hook sees `NEXUS_E2E_VAULT` and pre-boots the kernel
   against the scratch vault, then persists `last_forge_path` so the
   launcher's restore path agrees.
5. `before all` forces the WebView's initial navigation (tauri-driver
   leaves it at `about:blank`), waits for `__nexusShellApi`, then calls
   `nexus.workspace.setRoot` + flips `nexus.editor.defaultMode` to
   `source` and `nexus.editor.confirmCloseDirty` to `false`.
6. Spec drives the UI via kernel IPC + DOM keyboard events.
7. Teardown kills tauri-driver in `onComplete`.

## Known issues

- **Leaked `nexus-shell.exe` locks the scratch vault.** The kernel grabs
  exclusive file handles at boot; if a prior run crashed or was killed
  mid-session, rerunning `pnpm e2e` can fail early with
  `failed to initialise forge: I/O error: … another process has locked a
  portion of the file. (os error 33)`. Fix with
  `taskkill /F /IM nexus-shell.exe /IM tauri-driver.exe /IM msedgedriver.exe`.
- **WebView2 version mismatch** is a secondary failure mode. If
  `tauri-driver` exits immediately with `session not created`, the
  `msedgedriver` on PATH doesn't match the installed WebView2 runtime.
  Match major + minor.
- **Windows only.** tauri-driver does not support macOS or Linux.
- **Debug binary only.** If you build release (`tauri build`) the binary
  ends up under `target/release/` and the `tauri:options.application`
  path in `wdio.conf.ts` won't find it.
- **pnpm stdout buffering.** Running `pnpm e2e` via some shells (e.g.
  Git Bash through a managed harness) may buffer all output until process
  exit. If the run appears hung, let it finish — results arrive at the
  end. Interactive terminals don't see this.

## Remaining spec failures (product-side, not harness)

These two aren't infrastructure problems — the app loads, the editor
mounts, typing reaches CodeMirror. They need editor-plugin work.

1. **`undo past save returns to post-save state, then pre-save`.** The
   spec types `FIRST_EDIT`, saves, types `SECOND_EDIT`, then undoes
   twice expecting the first undo to strip `SECOND_EDIT` and the second
   to strip `FIRST_EDIT`. Currently the second undo leaves `FIRST_EDIT`
   in place — suggesting the undo stack doesn't traverse a save
   boundary. Bridge-plan decision #3 said the kernel would own the
   UndoTree across saves; that contract may not be implemented yet.
2. **`close without save preserves disk state`.** With
   `confirmCloseDirty=false`, `closeTab` via `nexus.editor.closeTab`
   command should drop the in-memory buffer. The reopened tab still
   surfaces the unsaved text, suggesting either the close path keeps
   the buffer around (cached in a store after tab removal) or
   `files:open` raises a stale tab instead of a fresh read from disk.

## Design notes for future work

- **`browserName: 'wry'`** (not `'webview2'`) matches Tauri's official
  WebdriverIO example. Using `'webview2'` let msedgedriver attach but
  tauri-driver never drove the initial navigation.
- **Navigation kick.** tauri-driver hands the WebView to msedgedriver
  at `about:blank`. `browser.url('http://tauri.localhost/')` from the
  `before all` hook is what actually triggers the bundle load — the
  asset protocol then responds normally.
- **Why command-API for save/close.** Keyboard chords (Ctrl+S, Ctrl+W)
  dispatched through webdriver get eaten by the shell's global
  keybinding dispatcher in `App.tsx` before they reach the CodeMirror
  scope. Calling `api.commands.execute('nexus.editor.save')` /
  `…closeTab` bypasses that. Undo stays on keyboard because it's a
  CM6-scoped binding, not a plugin command.
- **Source-mode default.** Markdown files open in preview by default.
  The helpers flip `nexus.editor.defaultMode` to `source` so keystroke
  tests target a real CodeMirror instance. If the first open still
  lands in preview (config read timing), `openFile` clicks the Edit
  button as a fallback.
