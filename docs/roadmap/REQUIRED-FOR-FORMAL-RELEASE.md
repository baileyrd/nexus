# Required for Formal Release

> **Tracked in** [PRDs/BACKLOG.md → "Formal release scope (deferred)"](PRDs/BACKLOG.md#formal-release-scope-deferred). This file holds the full design / effort detail for WI-41, WI-42, WI-44, WI-46; BACKLOG.md indexes them.

**Status:** Deferred — not needed for personal-tool use
**Date:** 2026-04-24
**Context:** Nexus is currently a personal tool. The work items captured here are extracted from the original Phase 5 plan and apply only if/when Nexus is prepared for external distribution.

Sections below are preserved verbatim from the pre-scope-reduction Phase 5 plan so they can be re-adopted wholesale. They are not current work.

## Open questions (from original §8)

- Q1 Code-signing certs (Apple Developer ID, Windows Authenticode — 1–3 week procurement lead)
- Q2 Telemetry scope (Sentry crash-only vs broader analytics vs self-hosted vs email-only)
- Q4 Marketplace scope (minimal JSON index vs ambitious separate repo vs deferred)
- Q5 Docs site (VitePress/Docusaurus) vs markdown-in-repo
- Q7 `@nexus/extension-api` npm publish at v1.0.0
- Q8 Beta channel (public pre-release, private list, or hybrid)

---

## WI-41 — Tauri auto-updater + code-signing + release channel


Ship a desktop app that can update itself. Without this, every v1.0.1 bug fix requires every user to manually re-download. With this, we push a signed build to the updater endpoint and users get a notification on next launch.

The WI has three parts tightly coupled by the signing key chain:

1. **Updater integration.** `tauri-plugin-updater` wired into `shell/src-tauri`, `tauri.conf.json` pointing at a static manifest URL.
2. **Code-signing.** Platform-specific signatures applied to the built artifacts: macOS `codesign` + notarization, Windows Authenticode signing, Linux signify (or `.deb`/`.AppImage` signed-by-gpg). Without signing, Windows SmartScreen and macOS Gatekeeper reject the install silently; the user sees "unknown developer, blocked."
3. **Release channel.** The scheme by which new builds are published. Simplest model: a GitHub Release per tag, with the Tauri update manifest (`latest.json`) served from GitHub Pages (or the Release's own asset URL) declaring the newest version + per-platform URLs + per-platform signatures.

#### 3.1.2 Current state

- **No updater dep.** `shell/src-tauri/Cargo.toml:25-35` lists `tauri = "2"`, `tauri-plugin-fs = "2"`, `tauri-plugin-dialog = "2"`. No `tauri-plugin-updater`. (Shell Tauri config also does not reference the updater plugin — `tauri.conf.json:19-45` has no `updater` key.)
- **No updater config in `tauri.conf.json`.** The current file (53 lines total) covers build, window, CSP, and bundle — no `app.updater.endpoints`, no `app.updater.pubkey`. The Tauri 2 updater requires both.
- **No release build infrastructure.** `.github/` directory does not exist. The `scripts/` directory holds 30+ bench/check/test shells but nothing for release, signing, or artifact publishing. There is no `cargo-dist`, no `goreleaser`, no `electron-builder` equivalent.
- **One release tag exists.** `git tag` returns `v0.1.0-legacy-shell` only (from Phase 0's ADR 0011 freeze anchor). No `v0.2.0`, no pre-release builds, no continuous-delivery tags.
- **`reqwest` already in workspace** (`Cargo.toml:126`) — irrelevant for this WI because the updater uses its own HTTP path, but worth noting for WI-42.
- **No signing certificates on record.** No entries in `.env.example`, no secrets file, no references in `CONTRIBUTING.md`. This is the long-lead item. To ship a signed macOS app you need:
  - Apple Developer Program membership ($99/yr).
  - A Developer ID Application certificate (not the Mac App Store cert).
  - An app-specific password or App Store Connect API key for `notarytool`.
  - Allow 1–5 business days for enrollment approval if not already a member.

  To ship a signed Windows app you need:
  - An EV (Extended Validation) or standard code-signing certificate from a CA (DigiCert, Sectigo, etc.).
  - EV certs ship on a hardware token (USB dongle) — physical delivery is 3–7 days.
  - Standard certs issue same-day but trigger SmartScreen "unknown" warnings until ~1000 installs pile up reputation.

  For Linux, GPG-signing a `.deb` or `.AppImage` is free; no CA required.

#### 3.1.3 Design sketch

**Part 1 — Updater wiring (1–2 days engineering).**

In `shell/src-tauri/Cargo.toml`:
```toml
tauri-plugin-updater = "2"
```

In `shell/src-tauri/src/lib.rs`, inside the builder chain (currently around `.plugin(...)` calls, roughly line 370-ish — audit confirms no `.plugin()` calls exist yet because fs/dialog are used via `tauri = { features = [] }`, not the plugin crates directly), add:
```rust
.plugin(tauri_plugin_updater::Builder::new().build())
```

In `shell/src-tauri/tauri.conf.json`, add:
```json
"plugins": {
  "updater": {
    "endpoints": [
      "https://<owner>.github.io/nexus/updates/{{target}}-{{arch}}/{{current_version}}"
    ],
    "pubkey": "<generated via `tauri signer generate`>"
  }
}
```

The `{{target}}` / `{{arch}}` / `{{current_version}}` are Tauri 2's template variables (resolved to e.g. `darwin-aarch64/0.9.0`). The endpoint must return JSON of shape:
```json
{
  "version": "1.0.0",
  "notes": "Release notes...",
  "pub_date": "2026-05-20T12:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<base64>",
      "url": "https://github.com/<owner>/nexus/releases/download/v1.0.0/nexus-shell_1.0.0_aarch64.app.tar.gz"
    },
    ...
  }
}
```

Shell-side UI: a small "Update available" toast handled by the notification service plugin (`shell/src/plugins/core/notificationService`). Hook into Tauri's `tauri://update-available` event emitted by the plugin; surface an "Install now / Later" prompt; call `installUpdate()` on accept.

**Part 2 — Signing keys + CI (3–4 days engineering, plus cert lead).**

Repo structure additions:
- `.github/workflows/release.yml` — triggered on `v*.*.*` tag push; matrix over macOS, Windows, Linux; each job:
  1. Builds the Tauri app with `--target <platform>`.
  2. Signs the bundle (platform-specific; see below).
  3. Generates the Tauri updater signature (`tauri signer sign` with the private key loaded from `TAURI_UPDATER_PRIVATE_KEY` secret).
  4. Attaches the signed bundle + signature to the GitHub Release.
- `.github/workflows/update-manifest.yml` — on Release publish; rebuilds the `latest.json` manifest that GitHub Pages serves.

Platform-specific signing steps in the release workflow:

| Platform | Tool | Secrets needed |
|---|---|---|
| macOS | `codesign -s "Developer ID Application: <Team>"` + `xcrun notarytool submit ... --apple-id --team-id --password` | `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERT_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_NOTARY_PASSWORD` |
| Windows | `signtool sign /fd SHA256 /t http://timestamp.digicert.com ...` | `WINDOWS_PFX_BASE64`, `WINDOWS_PFX_PASSWORD` (or an Azure Key Vault reference for EV) |
| Linux | `gpg --detach-sign` or rely on `.AppImage` self-signing | `LINUX_GPG_KEY` (optional) |
| Updater | `tauri signer sign` | `TAURI_UPDATER_PRIVATE_KEY`, `TAURI_UPDATER_KEY_PASSWORD` |

**Part 3 — Release channel (1 day).**

The simplest model: one channel, `stable`. Pre-releases (betas) ship as `v1.0.0-beta.1` tags and produce the same artifacts but the updater endpoint filters them out of `latest.json` unless the user flips a setting.

Schema for `latest.json` manifest — hosted at `gh-pages` branch or a dedicated `updates/` directory. Writeable by the `update-manifest.yml` workflow. Version-pinned schema so the shell can parse it deterministically.

Channel switcher: a setting in `settings.json` for opt-in to beta channel. Default stable.

#### 3.1.4 Subagent pattern

**Agent 1 (investigation, 1 call) — cert inventory.** Prompt: *"Confirm whether the user (via their org or personal Apple Developer Program) has a Developer ID Application cert on macOS. Confirm Windows code-signing cert status. Produce a 'yes/no/unknown' response per platform with evidence and next-action. No code; reports only."* This is mostly a user interview — the agent structures the questions.

**Agent 2 (implementation, 1 call) — GitHub Actions workflow.** Prompt: *"Write `.github/workflows/release.yml` that builds, signs, and publishes Nexus shell for macOS (aarch64, x86_64), Windows (x64), and Linux (x86_64 .AppImage + .deb). Use action-caching for cargo + pnpm. Gate signing on the presence of the secrets (so a fork PR still builds unsigned). Output: the YAML + a one-paragraph summary of each secret the user must add in GitHub settings."*

**Agent 3 (tauri.conf + shell wiring, 1 call).** Prompt: *"Add `tauri-plugin-updater` to `shell/src-tauri/Cargo.toml`, wire it in `shell/src-tauri/src/lib.rs`, add the `plugins.updater` block to `tauri.conf.json` with a placeholder pubkey. Add a minimal shell-side `useUpdater` React hook that listens for `tauri://update-available` and surfaces a notification via the notification service. Diff only."*

Main thread: review each, sequence them, and manually handle the secret-setup step (Agents shouldn't touch GitHub secrets).

#### 3.1.5 Commit plan

1. `chore(ci): add release.yml workflow for signed multi-platform builds` — new `.github/workflows/release.yml`, new `.github/workflows/update-manifest.yml`, new `scripts/sign-macos.sh` helper.
2. `feat(shell): wire tauri-plugin-updater + shell-side notification` — Cargo.toml, lib.rs, tauri.conf.json, shell notification hook.
3. `docs(release): document signing + cert procurement runbook` — new `docs/RELEASE-RUNBOOK.md` covering cert procurement, manual release drill, rollback.
4. `chore(repo): add CHANGELOG.md scaffold` — empty CHANGELOG covering v0.1.0 → unreleased, ready for WI-46 to populate.

**Files touched:**
- `.github/workflows/release.yml` — new (~200 lines YAML).
- `.github/workflows/update-manifest.yml` — new (~50 lines).
- `shell/src-tauri/Cargo.toml` — +1 dep.
- `shell/src-tauri/src/lib.rs` — +1 `.plugin()` call.
- `shell/src-tauri/tauri.conf.json` — +`plugins.updater` block.
- `shell/src/plugins/core/notificationService/useUpdater.ts` — new (~40 lines).
- `docs/RELEASE-RUNBOOK.md` — new (~200 lines).
- `CHANGELOG.md` — new.

#### 3.1.6 Acceptance

- `cargo build -p nexus-shell` compiles with the updater dep added.
- `pnpm --filter nexus-shell tauri build` on a local machine produces an installer, which on macOS is code-signed and notarized (verified with `spctl -a -v <.app>`).
- The GitHub Actions workflow runs green on a manual tag push (`v0.2.0-rc1` test tag), produces all five artifacts (.dmg, .msi, .deb, .AppImage, .zip), signs them, attaches them to the Release, and regenerates `latest.json`.
- Installing a `v0.2.0-rc1` build, then pushing a `v0.2.0-rc2` tag, surfaces an "update available" notification in the `v0.2.0-rc1` shell within the updater's check interval (default 4 hours; manually flushable via a dev menu).
- `spctl -a -v` on the `.app` reports `source=Notarized Developer ID` on macOS. `signtool verify /pa` on Windows reports `Successfully verified`.
- The release runbook is complete enough that a second engineer could cut a release.

#### 3.1.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Apple Developer enrollment delay pushes ship date past the v1.0 target | High | Start enrollment **day 1 of Phase 5**, in parallel with WI-42. Escalate via Apple Developer support if the enrollment review stalls past 3 business days. |
| Windows EV cert token physical delivery delay (3–7 days) | High | Start procurement day 1. Alternative: accept a standard (OV) cert for v1.0 and upgrade to EV in a v1.x patch — users see the SmartScreen warning for a few thousand installs before reputation builds. |
| `tauri-plugin-updater` config shape changes between Tauri 2.0 and 2.1 | Low | Pin to a tested minor version; test the flow on a canary machine before cutting v1.0. |
| Signed-but-notarization-failed macOS builds silently deliver (notarization fails open) | Medium | Add a verification step post-sign in the workflow: `xcrun stapler validate <.app>`. Fail the workflow if the staple is missing. |
| Updater endpoint URL bakes into every binary; a broken URL strands all clients | High | Host the manifest on GitHub Pages (99.9% SLA). The endpoint template supports a fallback list; configure at least two URLs. Include a setting to override the endpoint for enterprise/self-host users (defer to v1.1 if time-boxed). |
| Private updater signing key compromise forces every existing install to re-install manually | Critical | Store the key in 1Password / BitWarden / a HW key; never commit. GitHub Actions secret is OK. Document the rotation procedure in the runbook. |
| Signed release is invalidated by a change in `tauri.conf.json` identifier (bundle id) | Medium | Freeze `identifier: "dev.nexus.shell"` at ship-time; any change forces a re-sign + re-distribute. Flag in the runbook. |
| Linux `.AppImage` auto-update is a separate implementation path from Tauri's macOS/Windows updater | Low | For v1, `.AppImage` ships via GitHub Releases with a "check for updates" manual flow. Automatic `.AppImage` update can land post-v1. |

#### 3.1.8 Size

**M** engineering (~5–7 engineer-days), **plus 1–3 week calendar lead for cert procurement**. If certs are not in hand by the end of Phase 4, engineering day-1 of Phase 5 is "order certs" — the engineer then works on other WIs while certs are processed.

---

### 3.2 WI-42 — Crash reporting & telemetry (M, P0)

#### 3.2.1 Intent

A shipping desktop app needs a way to know when it crashed. Today, a Rust panic in `nexus-shell` (or the kernel running in-process) writes to stderr and dies silently; a TypeScript error in a plugin activation writes to `console.error` and may or may not show a toast depending on `plugin:error` event subscribers. Neither surfaces outside the user's machine.

The WI ships a **minimum-scope opt-in crash reporter** that captures:

- **Rust panics** via `std::panic::set_hook`, forwarded to Sentry's Rust SDK (`sentry` crate).
- **JS errors** via `window.addEventListener('error', ...)` and `unhandledrejection`, forwarded to Sentry's browser SDK (`@sentry/browser`).
- **Environment fingerprint:** OS, arch, shell version, plugin IDs + versions (not plugin *state*).

It explicitly **does not** capture:

- Note content, file paths from inside the forge, search queries, AI prompts, or any user-authored text.
- Feature-use counters or analytics (separate policy decision; not in Phase 5 scope).
- Anything at all if the user hasn't opted in.

#### 3.2.2 Current state

- **No panic handler anywhere.** `grep -rn "set_hook\|panic::catch\|PanicInfo" crates/ shell/src-tauri/src/` returns only a comment in `shell/src-tauri/src/lib.rs:261` ("Atomic write: tmp + rename so a crash mid-write can't produce a …" — unrelated to this WI). A Rust panic in the shell today unwinds, rolls back, and exits the process.
- **Tracing is set up per-binary.** `crates/nexus-cli/src/main.rs:981` and `crates/nexus-tui/src/main.rs:86` each call `tracing_subscriber::fmt()...init()` to stderr. The legacy `crates/nexus-app/src/lib.rs:108` init site was retired by Phase 4 WI-37. Shell has no tracing init today — `shell/src-tauri/src/lib.rs` has zero `tracing` references.
- **No HTTP client wired for telemetry.** `reqwest` is in workspace deps (`Cargo.toml:126`), used by `nexus-ai` and `nexus-mcp`; never from the shell.
- **No Sentry deps.** Not in `Cargo.toml`, not in `shell/package.json`.
- **No opt-in UI.** No `settings.json` schema key for telemetry, no toggle in the settings plugin.
- **File-as-truth rule** (PRD 01, ADR 0005) is explicit: note content lives in files, files are the source of truth, and the kernel never leaks file content outside the local machine without explicit user action. This constrains **every capture** — the panic hook must serialize *only* the panic message and stack, not any `&str` locals that might reference note content.

#### 3.2.3 Design sketch

**Part 1 — Opt-in UX (1 day).**

Add a settings schema entry (`settings.json`):
```json
{
  "telemetry.enabled": {
    "type": "boolean",
    "default": false,
    "description": "Send anonymous crash reports to help us fix bugs. No note content is ever uploaded."
  }
}
```

On first launch **of v1.0.0**, surface a one-time modal: "Would you like to help improve Nexus by sharing crash reports? No note content is uploaded. [Enable] [Skip]". Store the decision in `telemetry.prompted: true` alongside the enabled bit. `Skip` leaves enabled at `false`; user can toggle later in settings.

**Part 2 — Rust panic capture (1–2 days).**

Add `sentry = { version = "0.34", default-features = false, features = ["rustls", "backtrace"] }` to `shell/src-tauri/Cargo.toml`.

In `shell/src-tauri/src/main.rs` (currently minimal — wraps `nexus_shell_lib::run()`), before the `run()` call:
```rust
fn main() {
    let _guard = init_sentry_if_opted_in();
    std::panic::set_hook(Box::new(|info| {
        // Log locally first (never lost to stderr).
        tracing::error!(panic = %info, "shell panic");
        // Sentry capture is gated inside the bridge — if !opted_in, no-op.
        sentry::integrations::panic::panic_handler(info);
    }));
    nexus_shell_lib::run();
}
```

`init_sentry_if_opted_in()`:
- Reads the persisted `settings.json` directly (can't wait for plugin load — panic could happen earlier).
- If `telemetry.enabled == true`, call `sentry::init` with DSN pulled from env var `NEXUS_SENTRY_DSN` (baked at release-build time by the CI workflow; empty in dev).
- Returns a `ClientInitGuard` that must live the lifetime of the process.
- **Scrubbing rule:** install a `before_send` callback that strips `frames[].vars`, clears `message` if it contains a suspicious substring, and rejects any event whose `tags` contain a key prefixed `user.` or `file.`. Whitelist-based capture, not blacklist.

**Part 3 — JS error capture (1 day).**

Add `@sentry/browser` to `shell/package.json` devDependencies. (Production usage — but it loads deferred.)

In `shell/src/main.tsx`, after `boot()` returns but before React mount, check `settings.json` via the kernel's `configurationService` plugin. If opted in, call `Sentry.init({ dsn: import.meta.env.VITE_SENTRY_DSN, beforeSend: scrubBeforeSend })`. The `scrubBeforeSend` function:
- Strips `extra`, `contexts.state`, and any `breadcrumb.data.message` field — these most commonly carry user content.
- Redacts any string member matching a regex for a file path (`/forge/.../*.md`, `~/forge/...`, etc.) to `<redacted-path>`.
- Drops the event entirely if the `message` matches a known false-positive (e.g., CodeMirror cursor-position errors that are user-edit noise).

Wire global error handlers:
```ts
window.addEventListener('error', ev => Sentry.captureException(ev.error))
window.addEventListener('unhandledrejection', ev => Sentry.captureException(ev.reason))
```

Plugin errors: the Phase 3 WI-35 crash quarantine (`CommandRegistry.ts:38-54` try/catch) already logs + emits `plugin:handlerError`. Subscribe `plugin:handlerError` from the telemetry plugin and forward to Sentry **with plugin ID as a tag** so failures cluster per-plugin. This is valuable telemetry: "which plugin crashes most" is exactly what the maintainer needs.

**Part 4 — Env fingerprint (0.5 day).**

On Sentry init, set static tags once:
```ts
Sentry.setTag('nexus.shell.version', import.meta.env.VITE_APP_VERSION)
Sentry.setTag('nexus.os', navigator.platform) // scrubbed to 'darwin'/'win32'/'linux'
Sentry.setTag('nexus.arch', ...)  // via a Tauri command
Sentry.setContext('plugins', {
  ids: reg.manifests().map(m => `${m.id}@${m.version}`)
})
```

Rust side adds `sentry::configure_scope(|s| s.set_tag("nexus.kernel.version", env!("CARGO_PKG_VERSION")))`.

**Part 5 — File-as-truth audit (0.5 day).**

Write a dedicated test file `shell/src-tauri/tests/telemetry_scrubbing.rs` (or a vitest spec for the JS side) that:
- Simulates a panic carrying a file path in the message.
- Simulates a plugin error whose stack captures a note's content.
- Asserts both are scrubbed before the `before_send` returns.
- Uses a spy Sentry transport so no real network call occurs.

The test is the **contract** for the file-as-truth rule — if someone future adds a new capture path, they have to either (a) prove it's scrubbed, or (b) break the test and think about it.

#### 3.2.4 Subagent pattern

**Agent 1 (investigation) — Sentry DSN/setup runbook.** One-shot: *"I need to create a free-tier Sentry project for a Rust+TS desktop app called Nexus. Walk through the sign-up, project creation, DSN retrieval, and how to add the DSN as a GitHub secret in CI. Flag any tier limits (events/month, retention) and what happens when we exceed. ~500 words."*

**Agent 2 (implementation) — scrubbing contract.** Prompt: *"Write `scrubBeforeSend` for `@sentry/browser` and the equivalent `before_send` for the `sentry` Rust crate. They must strip any file path under `~/forge/**`, any note content (string longer than 200 chars inside a breadcrumb), and any tag key under `user.*` or `file.*`. Produce both implementations with tests. ~200 lines total."*

**Agent 3 (UX) — first-launch opt-in modal.** Prompt: *"Design a non-blocking-but-prominent opt-in modal that appears exactly once on first launch of v1.0.0+. Persist the prompted decision. Integrate with the settings plugin. No blocking the shell boot. Diff only."*

Main-thread: review, sequence, and add the Sentry DSN secret to CI.

#### 3.2.5 Commit plan

1. `feat(shell): opt-in telemetry setting + first-launch prompt` — settings schema + prompt UI + persistence.
2. `feat(shell): Rust panic handler + Sentry integration (opt-in gated)` — Cargo.toml, main.rs, scrubbing module.
3. `feat(shell): JS error capture + scrubbing` — main.tsx, scrub module, plugin:error subscription.
4. `test(shell): file-as-truth scrubbing contract` — telemetry_scrubbing tests (Rust + TS).
5. `docs(shell): telemetry policy + what-we-capture enumeration` — new `docs/TELEMETRY-POLICY.md` explaining exactly what we capture and what we never capture. Links to the scrubbing tests as enforcement.

**Files touched:**
- `shell/src-tauri/Cargo.toml` — +`sentry` dep.
- `shell/src-tauri/src/main.rs` — panic hook + init.
- `shell/src-tauri/src/telemetry.rs` — new (~150 lines).
- `shell/src-tauri/tests/telemetry_scrubbing.rs` — new (~200 lines).
- `shell/package.json` — +`@sentry/browser` dep.
- `shell/src/main.tsx` — Sentry init after settings load.
- `shell/src/telemetry/scrub.ts` — new (~100 lines).
- `shell/src/telemetry/scrub.test.ts` — new (~150 lines).
- `shell/src/plugins/nexus/settings/TelemetrySection.tsx` — new toggle.
- `docs/TELEMETRY-POLICY.md` — new (~150 lines).

#### 3.2.6 Acceptance

- First launch of a v1.0.0 build shows the opt-in modal exactly once; dismissing it sets `telemetry.prompted = true` and leaves `telemetry.enabled = false`.
- With `telemetry.enabled = true`, a deliberate panic in the kernel (guarded dev flag) produces a Sentry event visible in the Sentry dashboard within 60 seconds.
- With `telemetry.enabled = false`, the same panic produces **zero** network traffic (verified with a local proxy during a test run).
- `cargo test -p nexus-shell --test telemetry_scrubbing` passes.
- `pnpm --filter nexus-shell test -- scrub` passes.
- A plugin-error event for `com.nexus.editor` captures `plugin = "com.nexus.editor@0.9.0"` as a tag so the Sentry dashboard can group by-plugin.
- `docs/TELEMETRY-POLICY.md` explicitly enumerates the captured fields.

#### 3.2.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| A scrubbing rule misses a content leak path and note content ends up on Sentry | Critical | Whitelist (not blacklist) what we capture. Tests enforce scrubbing. Run a two-week internal soak before opening to users. |
| Sentry free-tier quota exceeded during beta (5K events/month) | Medium | Rate-limit at the client: one crash per plugin per session, aggregate the rest. Upgrade to Team tier ($26/mo) if beta exceeds. |
| Default-on telemetry is a bad look; default-off loses signal | Medium | Default-off. Accept the signal loss for v1; revisit for v1.x with more consent-design work. §6 Q2 flags this explicitly. |
| Sentry as a company/policy changes relative to our file-as-truth rule | Low | The scrubbing happens client-side; Sentry receives already-scrubbed events. Switching backends later is 1–2 days of SDK-swap work. |
| Panic hook interferes with debugger attach during development | Low | Gate the Sentry-side init on `cfg!(debug_assertions) == false` or `NEXUS_SENTRY_DEV_ENABLE == "1"`. |
| JS `window.error` handler fires for every CodeMirror cursor error, floods Sentry | Medium | Add a `beforeSend` filter that drops known-benign error messages. Keep the list short and commented. |
| Telemetry adds ~300KB to the bundle | Low | Acceptable; users who opted out never load the Sentry SDK runtime code (dynamic import gated on settings). |

#### 3.2.8 Size

**M** — ~5 engineer-days. Breakdown: 1d opt-in UX, 2d Rust + JS capture paths, 1d scrubbing tests, 1d docs + integration review.

---


---

## WI-44 — Minimal marketplace


Give users a way to discover and install community plugins without manually dropping directories into `~/.nexus-shell/plugins/`. "Minimal" means: no review pipeline, no ratings, no uploader portal, no backend server. Just a static JSON file in the main repo listing approved community plugins + their download URLs, a shell UI that renders it, and a CLI/UI install action that fetches + unpacks.

Users who want to distribute their own plugin outside the marketplace still can (manual drop); marketplace entries are the "curated, vouched-for" shortlist.

#### 4.2.2 Current state

- **Community plugins already work.** `~/.nexus-shell/plugins/` is scanned by `shell/src-tauri/src/lib.rs:54-112` on every boot. Phase 3 WI-30 iframe sandbox isolates them; WI-31 consent gates them; WI-33 `api_version` check rejects incompatibles. Install is `mkdir ~/.nexus-shell/plugins/my-plugin && cp plugin.json index.js ...`.
- **No install command.** `crates/nexus-cli/src/` has no `plugin install` subcommand (verified). No `plugin list` either — the CLI is kernel-focused, and plugins are a shell concept.
- **No JSON index.** No `nexus-plugins.json` in the repo, no hosted equivalent elsewhere.
- **Only one community plugin exists in-repo.** `shell/src/plugins/community/hello-world/` with `plugin.json` declaring `apiVersion: 1`, `sandboxed: true`, `capabilities: ["UiNotify"]`. Used as the E2E test fixture. A second external plugin would need to be authored.
- **No tarball/zip pipeline.** Community plugins ship as source directories today.

So: everything downstream of "unpack a tarball into `~/.nexus-shell/plugins/<id>/`" is already in place. The gap is the "discover + fetch" layer.

#### 4.2.3 Design sketch

**Part 1 — Index schema (0.5 day).**

`nexus-plugins.json` at repo root (or a separate `marketplace/` directory):
```json
{
  "$schema": "https://nexus.dev/marketplace/v1.schema.json",
  "plugins": [
    {
      "id": "community.hello-world",
      "name": "Hello World",
      "description": "Example community plugin. Registers a 'Say Hello' command.",
      "author": "nexus team",
      "version": "1.0.0",
      "apiVersion": 1,
      "capabilities": ["UiNotify"],
      "sandboxed": true,
      "tarballUrl": "https://github.com/.../releases/download/plugins-v1/hello-world-1.0.0.tar.gz",
      "tarballSha256": "<sha256-hex>",
      "homepage": "https://github.com/.../tree/main/plugins/hello-world",
      "license": "MIT"
    },
    ...
  ]
}
```

Hosted at either:
- `https://raw.githubusercontent.com/<owner>/nexus/main/nexus-plugins.json` (simplest, one HTTP GET).
- A GitHub Pages mirror (`https://<owner>.github.io/nexus/plugins.json`) to avoid exhausting raw.githubusercontent rate limits. Same content, better SLA.

The `tarballUrl` + `tarballSha256` pair is the integrity anchor — without signing (Phase 6 feature), the SHA is the best we can do. Phase 5 stance: **marketplace plugins are first-party-vouched**, meaning a PR to the main repo had to merge before the index lists them. The SHA guards against mirror tampering, not author dishonesty.

**Part 2 — Shell Marketplace tab (1.5 days).**

Add a `shell/src/plugins/nexus/marketplace/` plugin. Contributes a tab inside `pluginsMgmtPlugin`'s view: "Installed" | "Available (bundled)" | "Marketplace". The Marketplace tab:

1. Fetches the JSON index on open (cached for session; refresh button).
2. Renders a card per entry with name, description, capability chips (reuse Phase 2 WI-18 chip component), version, author, "Install" button.
3. On Install: kicks off a progress toast, calls a new Tauri command `install_community_plugin(url, sha256, id)`, waits for completion, prompts for a shell reload.
4. Post-reload, the newly-installed plugin appears in the Installed section — indistinguishable from a manually-dropped plugin.

Tauri command `install_community_plugin`:
- Fetches the tarball via `reqwest` (already workspace dep).
- Verifies SHA256 against `tarballSha256`.
- Extracts to a tempdir.
- Validates that the extracted dir contains a `plugin.json` with matching `id`.
- Moves the tempdir to `~/.nexus-shell/plugins/<id>/` atomically (rename).
- Returns `Ok(())` on success, typed error on failure.

**Part 3 — CLI install (1 day).**

Add `plugin` subcommand group to `nexus-cli`:
```
nexus plugin list            # enumerate marketplace index
nexus plugin search <term>   # filter by substring
nexus plugin install <id>    # download + unpack
nexus plugin uninstall <id>  # rm -rf ~/.nexus-shell/plugins/<id>
nexus plugin enabled         # list installed (scan the dir)
```

Implementation lives in `crates/nexus-cli/src/commands/plugin.rs` (already exists — Phase 4 WI-38 added the `install|list|remove` subcommand stubs that WI-44 fleshes out). Uses `reqwest` + a tar-extraction crate (`tar` + `flate2`). The operation is identical to the shell-side Tauri command — factor out to `crates/nexus-shell-lib-marketplace` or keep duplicated for v1; the Phase 4 WI-38 unified binary is the shared shell entry.

**Part 4 — Tarball publishing process (0.5 day).**

The marketplace index points at tarball URLs that must exist. Short-term: per-plugin GitHub Release (e.g. `plugins-hello-world-1.0.0`) with the tarball as an asset. Longer-term: the marketplace could live in its own repo with CI-generated tarballs. v1 scope is the short-term flow.

Document the author-side flow in `docs/PUBLISHING-A-PLUGIN.md`: (1) ship your plugin dir with `plugin.json`, (2) `tar -czf <id>-<version>.tar.gz -C your-plugin .`, (3) compute SHA256, (4) file a PR to `nexus-plugins.json` with a new entry pointing at a Release asset you've prepped.

**Part 5 — Update path (deferred).**

Marketplace plugin updates are **out of scope for v1**. If a new version lands in the index, the user sees it as "update available" but has to re-install manually. Auto-update of community plugins is a Phase 6 item tied to plugin-signing policy.

#### 4.2.4 Subagent pattern

**Agent 1 (Rust Tauri command).** Prompt: *"Implement `install_community_plugin` in `shell/src-tauri/src/lib.rs`. Signature: `(url: String, sha256: String, id: String) -> Result<(), String>`. Uses reqwest + tar + flate2. Verifies SHA before extract. Atomic move to final location. Unit test with a local mock tarball. ~150 lines + tests."*

**Agent 2 (shell Marketplace UI).** Prompt: *"Create `shell/src/plugins/nexus/marketplace/` — a plugin that contributes a 'Marketplace' tab to the pluginsMgmt view. Fetches a JSON index, renders cards, wires an Install button to the Tauri command. Uses existing capability-chip component. ~400 lines."*

**Agent 3 (CLI plugin subcommand).** Prompt: *"Add `nexus plugin {list,search,install,uninstall,enabled}` subcommands to `crates/nexus-cli`. Reuses the shell-side install logic (factor a shared helper crate `nexus-marketplace` if practical, otherwise duplicate 80 LOC). ~300 lines + tests."*

Main-thread: author `nexus-plugins.json` (initial with hello-world only), publish the hello-world tarball to a Release, verify the round-trip.

#### 4.2.5 Commit plan

1. `feat(marketplace): add nexus-plugins.json index + schema` — root index file (seeded with hello-world), schema doc.
2. `feat(shell): install_community_plugin Tauri command` — Rust side.
3. `feat(shell): Marketplace tab in pluginsMgmt` — UI side.
4. `feat(cli): nexus plugin {list,install,uninstall,...} subcommands` — CLI side.
5. `docs: PUBLISHING-A-PLUGIN guide for marketplace submissions` — author docs.

**Files touched:**
- `nexus-plugins.json` (root) — new.
- `nexus-plugins.schema.json` — new (JSON Schema).
- `shell/src-tauri/Cargo.toml` — +`reqwest`, `tar`, `flate2`, `sha2`.
- `shell/src-tauri/src/lib.rs` — +`install_community_plugin` command.
- `shell/src/plugins/nexus/marketplace/` — new plugin (~500 lines).
- `crates/nexus-cli/Cargo.toml` — +`reqwest`, `tar`, `flate2`.
- `crates/nexus-cli/src/plugin.rs` — new subcommand module.
- `crates/nexus-cli/src/main.rs` — register subcommand.
- `docs/PUBLISHING-A-PLUGIN.md` — new (~200 lines).

#### 4.2.6 Acceptance

- `nexus plugin list` prints the index (one entry: hello-world).
- `nexus plugin install community.hello-world` downloads + unpacks to `~/.nexus-shell/plugins/community.hello-world/`; next shell boot loads it.
- Shell Marketplace tab shows one card, Install button triggers the same flow, post-install notification nudges a reload.
- SHA mismatch triggers a clear "integrity check failed" error in both CLI and UI.
- Uninstall (CLI or UI) removes the directory; plugin is gone on next boot.

#### 4.2.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Marketplace index gets out of sync with real tarball versions (index says 1.0.1, tarball is 1.0.0) | Medium | CI lint on PRs that modify `nexus-plugins.json`: verify each entry's tarball exists + SHA matches. |
| A user installs a marketplace plugin, the tarball URL is dead (GitHub Release deleted) | Low | Mirror tarballs via GitHub Pages (optional v1.x enhancement). For v1, a 404 surfaces as a clear error. |
| User manually edits `nexus-plugins.json` in the repo to install a non-listed plugin | Low | That's fine — equivalent to dropping the plugin into the dir by hand. The index is a convenience, not a gate. |
| Scope creep: the user wants rating stars, reviews, download counts | High | Hard-bounded at "static JSON + tarballs" for v1. Surfaced in §6 Q4. Anything more is Phase 6. |
| Community plugin author experience is bad; they PR to the index and wait 3 weeks for review | Medium | Set an explicit policy in `PUBLISHING-A-PLUGIN.md`: "first-party review within 5 business days for v1; cadence re-evaluated as volume grows." |
| Marketplace tab loads a cached index with a stale malicious entry (if an entry was pulled post-install) | Medium | Refresh on tab-open (no hard cache). Add a "last updated" timestamp to the index. |
| Phase 3 WI-30 iframe sandbox + WI-31 consent flow already gates each install — this WI piles on | Low (not a risk) | The sandbox/consent is the safety story; marketplace is the discovery story. Composition is fine. |

#### 4.2.8 Size

**M** — ~5 engineer-days. Breakdown: 1d schema + index, 1.5d shell UI + Tauri command, 1d CLI subcommands, 0.5d tarball-publishing process, 1d tests + docs.

---

---

## WI-46 — Beta → GA


Run a structured 2-week beta with a test group. Triage what comes in. Fix ship-blocking bugs. Cut `v1.0.0`. This WI is logistics, not code. Planning it well is the difference between "v1 ships on time" and "v1 slips six weeks while we chase reported issues."

#### 5.2.2 Current state

- **No beta infrastructure.** No beta channel, no test group defined, no triage rubric, no ship criteria artifact, no `v1.0.0` branch strategy.
- **One release tag exists** (`v0.1.0-legacy-shell`).
- **No `CHANGELOG.md`.** (WI-41 scaffolds an empty one.)
- **No public download page.** (WI-41 lands GitHub Releases as the distribution path.)
- **Phase 4 WI-37** (landed 2026-04-24) deleted `crates/nexus-app` + `app/`. No pre-release coordination needed on this axis.

#### 5.2.3 Design sketch — the ship plan

**Week 0 (prep, 3 engineer-days, overlaps WI-41/42/45):**

1. **Define the beta tester group** (0.5d). Target: 20–50 volunteers. Sources: existing sessions, Nexus discord/forum if one exists, /r/ObsidianMD or /r/NotePad users who opt-in via a short survey. A private email list of opt-ins.
2. **Create the triage rubric** (0.5d). New `docs/TRIAGE-RUBRIC.md`:
   - **Ship-blocker (S0):** data loss, security issue, can't boot, can't open a forge.
   - **Must-fix (S1):** core workflow breaks (can't create/edit/save a note), crashes in a default-on plugin.
   - **Should-fix (S2):** UX regressions, polish bugs, non-default-plugin crashes.
   - **Nice-to-have (S3):** feature requests, enhancements.
   - v1.0.0 criteria: zero S0/S1 open for 5 business days. S2 count < 10. S3 count doesn't gate.
3. **Set up a triage board** (0.5d). GitHub Issues with labels `S0`, `S1`, `S2`, `S3`, plus `beta`, `post-v1`. An Issue template for bug reports gathering environment + Sentry event link + repro steps.
4. **Populate `CHANGELOG.md`** (0.5d). Seed with v0.1.0 → v0.2.0 (if intermediate tags cut) → v1.0.0. Use Keep-a-Changelog format. Each Phase 1–5 WI gets a line.
5. **Write the beta announcement + tester onboarding doc** (0.5d). `docs/BETA-TESTER-ONBOARDING.md`: how to install the beta build, how to file a report, what to test, where to find logs, how to opt out.
6. **Cut a `v1.0.0-beta.1` tag** (0.5d). Triggers the WI-41 workflow. Produces signed installers on the beta channel. Announcement goes out.

**Weeks 1–2 (beta, ~10 business days):**

Daily rhythm:
- Morning: triage new reports against the rubric.
- Midday: fix S0/S1 bugs, cut `v1.0.0-beta.2+` as they land.
- End of day: update the tester group (Discord / email / status page) with "what's fixed / what's in flight."

Weekly rhythm:
- **End of Week 1:** beta-retro meeting with top-5 active testers. What's working, what's rough. Capture qualitative signal the Issue list misses.
- **End of Week 2 (Day 14):** **go/no-go review.** Hit the rubric criteria? Ship. Don't hit them? Explicit `Beta Extended` communication, new 1-week cycle, another review on Day 21.

**Week 3 (ship, 1–2 days):**

1. Cut `v1.0.0` tag on the commit that meets the rubric.
2. WI-41 workflow produces signed releases.
3. Announcement goes out.
4. Update the updater `latest.json` manifest so `v1.0.0-beta.N` users get auto-prompted to update to `v1.0.0`.
5. `CHANGELOG.md` final v1.0.0 entry lands.
6. Roadmap for v1.1 goes into `docs/archive/planning/ROADMAP.md`. Closes Phase 5.

#### 5.2.4 Subagent pattern

**Minimal.** Ops work doesn't fan out well. One agent could help:

**Agent 1 (announcement drafts).** Prompt: *"Write three draft announcements for the beta: (a) tester invitation email, (b) Discord/forum post, (c) in-app notification toast for v0.2.0 users prompting them to upgrade. Match Nexus's voice (see README.md). ~200 words each."*

Main thread owns the triage board, the daily rhythm, and the go/no-go call.

#### 5.2.5 Deliverables checklist

| Deliverable | Due | Owner |
|---|---|---|
| Beta tester list + contacts | Week 0 day 3 | Main |
| `docs/TRIAGE-RUBRIC.md` | Week 0 day 2 | Main |
| GitHub Issues templates + labels | Week 0 day 3 | Main |
| Seeded `CHANGELOG.md` | Week 0 day 4 | Main |
| `docs/BETA-TESTER-ONBOARDING.md` | Week 0 day 5 | Main |
| `v1.0.0-beta.1` tag pushed | Week 0 day 5 | Main |
| Beta kickoff announcement | Week 1 day 1 | Main |
| Daily triage log | Weeks 1–2 | Main |
| Week 1 retro summary | Week 1 day 5 | Main |
| Go/no-go decision | Week 2 day 5 | Main |
| `v1.0.0` tag + announcement | Week 3 day 1 | Main |
| v1.1 roadmap draft | Week 3 day 2 | Main |

#### 5.2.6 Acceptance — the v1 gate

The v1.0.0 ship criteria, in rubric terms:
- **Zero S0 open for 5 business days.**
- **Zero S1 open for 5 business days.**
- **S2 count ≤ 10, each with a documented owner or explicit "post-v1" defer.**
- **S3 count is irrelevant** — sorted as a v1.x backlog.

Non-rubric ship criteria:
- Updater round-trip verified (v1.0.0-beta.N → v1.0.0 installs cleanly on all 3 platforms).
- Telemetry opt-in flow verified for a first-launch experience.
- README + docs link graph has no broken links (`grep` + link-check tool).
- CHANGELOG v1.0.0 section is complete.

#### 5.2.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| No testers recruited | High | Start recruitment immediately (Phase 4 shipped 2026-04-24); don't wait for Phase 5 engineering to finish. |
| Testers file feature requests instead of bugs; rubric blurs | Medium | Explicit onboarding doc section: "what we're testing vs. what we're not." |
| Critical bug found in Week 2, fix requires cert re-issue (e.g., bundle ID conflict) | Critical | Cert-related debugging happens in Week 0 during `v1.0.0-beta.1`. Don't wait for bugs. |
| Beta test group is too small (<5 active testers) — no real signal | High | Aim for 20–50. If under 10 active by Week 1 Day 3, extend recruitment, delay the ship date. |
| Testers have a dramatically different OS mix from the main user base | Medium | Recruit across Windows, macOS (both Intel & Apple Silicon), Linux explicitly. Ask on the survey. |
| Go/no-go decision is political ("just ship it, we'll patch") | High | The rubric is the rubric. Adherence is a discipline test; document in the retro if overridden and justify. |
| Sentry is flooded with beta errors; free tier quota burns | Low | Pre-emptively raise the quota or rate-limit at the client per WI-42 risk. |
| Update prompt during beta is annoying | Medium | Only prompt once per day; aggressive changelog notes so testers understand why they're updating. |
| v1.0.0 ships with undeclared/stale docs from the planning era | Low | WI-45 prevents this; verify before final tag. |
| 2-week window slips to 4–6 weeks | High | That's OK. "Ship v1 on a hard date" is worse than "ship v1 when rubric is met." But the plan should acknowledge it. |

#### 5.2.8 Size

**Ops-only**, ~3 engineer-days for the Week 0 setup + daily triage load throughout Weeks 1–2 + Week 3 ship. Not gated on engineering velocity; gated on bug inflow + fix rate.

---

---

## Formal-release-only documentation deliverables

From original WI-45, the following deliverables are formal-release-scoped and deferred:

- `docs/RELEASE-RUNBOOK.md` (how a second engineer cuts a release)
- `docs/TELEMETRY-POLICY.md` (scrub/opt-in policy)
- `docs/PUBLISHING-A-PLUGIN.md` (marketplace submission guide)
- `docs/TRIAGE-RUBRIC.md` (S0/S1/S2/S3 bug-severity rubric)
- `docs/BETA-TESTER-ONBOARDING.md`
- `CHANGELOG.md` populated against external version tags
- Full top-level README rewrite as a v1 product landing page
- `docs/README.md` audience-indexed hub polished for public readers
