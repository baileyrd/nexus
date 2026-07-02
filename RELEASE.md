# Release process

> V18 (`docs/0.1.2/audits/repo-review-2026-06-10.md`) — this documents the
> process as it exists today. Update it when the process changes; the
> workflow files are authoritative for mechanics.

## What a release is today

- The versioned artifact set is the **desktop shell** (`nexus-shell`) on
  all three platforms, each built by its own tag-triggered workflow and
  attached to one shared **draft** GitHub Release:
  - **Windows** — `.msi` (WiX) + `.exe` (NSIS):
    [`.github/workflows/release-windows.yml`](.github/workflows/release-windows.yml)
  - **Linux** — `.deb` + `.rpm` + `.AppImage`:
    [`.github/workflows/release-linux.yml`](.github/workflows/release-linux.yml)
  - **macOS** — `.dmg` for `aarch64-apple-darwin` and `x86_64-apple-darwin`:
    [`.github/workflows/release-macos.yml`](.github/workflows/release-macos.yml)
- Every workflow attaches a `SHA256SUMS-<platform>-<tag>.txt` sidecar so
  downloads can be integrity-checked
  (`sha256sum -c` / `shasum -a 256 -c` / `Get-FileHash`).
- The Rust binaries (`nexus`, `nexus-tui`) and `@nexus/extension-api` are
  not yet independently released; every crate tracks the single
  `[workspace.package] version` in `Cargo.toml` and is `publish = false`.

## Cutting a release

1. **Preflight (on `main`, green CI):**
   - [ ] `ci.yml` green on the release commit (tests, clippy, fmt, pnpm,
         cargo-deny) and `ipc-drift-check.yml` green.
   - [ ] `CHANGELOG.md` — move the `Unreleased` entries under the new
         version heading with today's date.
   - [ ] Version bump if warranted: `[workspace.package] version` in
         `Cargo.toml`, `shell/package.json`, and
         `shell/src-tauri/tauri.conf.json` stay in lock-step.
   - [ ] `DEPRECATED.md` — anything announced one minor release ago is
         due for removal *before* tagging, per the deprecation policy.
2. **Tag:** `git tag vX.Y.Z && git push origin vX.Y.Z`. The tag push
   triggers all three platform builds; each attaches its artifacts +
   checksums to the same **draft** GitHub Release named after the tag.
3. **Review + publish:** wait for all three workflows, check the draft
   Release (2 Windows installers, 3 Linux packages, 2 macOS dmgs, 4
   checksum files), smoke-test at least one installer per platform,
   paste the CHANGELOG section into the Release notes, then publish.
   - macOS caveat: builds are unsigned/un-notarized — Gatekeeper
     quarantines them. Testers: `xattr -cr "Nexus Shell.app"` or
     right-click → Open.
4. **Dry-run option:** `workflow_dispatch` on any release workflow builds
   its artifacts off `main` without a tag (artifacts only, no Release).

## Who

The repository owner cuts releases. There is no release cadence; releases
are cut when a coherent set of work lands.

## Auto-update — groundwork (owner action required)

The shell has no self-update today. The plan is Tauri 2's official
updater; wiring it is blocked only on secrets that must not live in the
repo. When ready:

1. Generate the updater signing keypair **locally** (never commit the
   private key): `pnpm --filter nexus-shell tauri signer generate`.
2. Store the private key + password as repo secrets
   (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`);
   export them in the three release workflows' build steps.
3. Add `tauri-plugin-updater` to `shell/src-tauri`, set
   `bundle.createUpdaterArtifacts: true` and the `plugins.updater`
   config (public key + endpoint) in `tauri.conf.json`. The natural
   endpoint is a `latest.json` manifest attached to each GitHub Release
   (`https://github.com/<owner>/nexus/releases/latest/download/latest.json`).
4. Decide the update-check UX in the shell (background check + toast vs.
   settings-page button) — that part is a normal shell plugin
   contribution, not release plumbing.

Until then, updates are manual downloads. This section exists so the
key-handling steps are agreed before any updater code lands.

## Not yet in place (known gaps)

- Installer/package **code-signing** on every platform, and macOS
  **notarization** (the biggest UX gap — see the Gatekeeper caveat).
- **Auto-updater** — groundwork documented above; blocked on
  owner-generated signing keys.
- **SLSA provenance** on release artifacts (checksums now exist; supply-
  chain attestation does not).
- Independent versioning for `@nexus/extension-api` once the plugin
  contract stabilizes (#187 — contract reconciled 2026-07-01, soaking).
