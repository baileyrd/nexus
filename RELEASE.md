# Release process

> V18 (`docs/0.1.2/audits/repo-review-2026-06-10.md`) — this documents the
> process as it exists today. Update it when the process changes; the
> workflow files are authoritative for mechanics.

## What a release is today

- The single versioned artifact set is the **Windows desktop shell**
  (`nexus-shell`): an `.msi` (WiX) and an `.exe` (NSIS), built by
  [`.github/workflows/release-windows.yml`](.github/workflows/release-windows.yml).
- The Rust binaries (`nexus`, `nexus-tui`) and `@nexus/extension-api` are
  not yet independently released; every crate tracks the single
  `[workspace.package] version` in `Cargo.toml` and is `publish = false`.
- Linux/macOS shell builds are not yet automated (the workflow is
  Windows-only).

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
   triggers the Windows build, which attaches both installers to a
   **draft** GitHub Release named after the tag.
3. **Review + publish:** check the draft Release, smoke-test the
   installer, paste the CHANGELOG section into the Release notes, then
   publish.
4. **Dry-run option:** `workflow_dispatch` on the release workflow builds
   the installers off `main` without a tag (artifacts only, no Release).

## Who

The repository owner cuts releases. There is no release cadence; releases
are cut when a coherent set of work lands.

## Not yet in place (known gaps)

- Linux (`.deb`/`.AppImage`) and macOS (`.dmg`) release workflows.
- Installer code-signing and update-channel metadata.
- Checksums/SLSA provenance on release artifacts.
- Independent versioning for `@nexus/extension-api` once the plugin
  contract stabilizes (#187).
