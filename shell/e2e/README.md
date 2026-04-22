# Nexus shell e2e harness

Headless WebDriver-driven end-to-end tests for the Tauri app in `shell/`.

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
3. **A debug build of the app**:
   ```
   pnpm e2e:build
   ```
   That runs, in order:
   - `cross-env VITE_E2E=true pnpm build` — builds the frontend with
     `VITE_E2E=true` baked into `import.meta.env`, which triggers
     `src/main.tsx` to attach `window.__nexusShellApi` at boot.
   - `cargo build` under `src-tauri/` — debug, not release, to avoid the
     release-build cycle time.

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
3. Launches `src-tauri/target/debug/nexus-shell.exe` through the driver.
4. Calls `init_forge` + `boot_kernel` against the scratch vault via Tauri
   IPC (same path the launcher's "Open recent" row uses) since there is no
   environment-variable hook for the forge root.
5. Runs `specs/golden-path.spec.ts`.
6. Tears down the driver in `onComplete`.

## Known issues

- **WebView2 version mismatch** is the #1 failure mode. If `tauri-driver`
  exits immediately with `session not created`, the `msedgedriver` on PATH
  doesn't match the installed WebView2 runtime. Match major + minor.
- **Windows only.** tauri-driver does not support macOS or Linux.
- **Debug binary only.** If you build release (`tauri build`) the binary
  ends up under `target/release/` and the `tauri:options.application`
  path in `wdio.conf.ts` won't find it.
- **No env-var hook for the vault path** — see the IPC-based seeding in
  `support/app.ts::openVault()`. If the shell ever grows a
  `NEXUS_VAULT_PATH` env var, swap that in and drop the IPC dance.
- **CM6 focus**: clicking `.cm-content` is the only reliable way I've
  found to move focus into the editor on a fresh tab. Ctrl-S / Ctrl-Z
  dispatched without that click end up on the shell root and get swallowed
  by the global keybinding dispatcher in `App.tsx`.
