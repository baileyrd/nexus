# Phase 5 Implementation Plan — v1 Polish & Ship

**Status:** Plan only (no code changes yet)
**Date:** 2026-04-24
**Author:** Claude (audit + planning run)
**Phase:** 5 of 6 in the shell-migration roadmap (per [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5)
**Prerequisite:** Phases 1+2+3+**4** complete and pushed to `main`. Phase 4 shipped 2026-04-24 as commits `38a1e82`…`36d1f06` plus the `74b098f` WI-37 follow-up; the WI-38 unified `nexus` binary, WI-39 `--template script` plugin scaffold, and WI-40 MCP-parity subcommands are all live, and WI-37 retired `crates/nexus-app` + `app/`. The `74b098f` follow-up cleaned up 28 orphan `app/src/bindings/*.ts` files that WI-37 missed and stripped the `nexus-theme` `#[ts(export, export_to="../../../app/src/bindings/")]` attributes that were regenerating the directory.
**Source outline:** [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5 "Phase 5 — v1 Polish & Ship (3–4 weeks)" — six items.
**Backing audits:** fresh audit of `shell/src-tauri/{Cargo.toml, tauri.conf.json, src/lib.rs}`, `shell/src/main.tsx`, `crates/**/Cargo.toml`, `scripts/`, top-level `README.md`, `shell/README.md`, `shell/docs/`, `packages/nexus-extension-api/README.md`, `docs/writing-your-first-plugin.md`, top-level directory structure. (`app/README.md` no longer exists — retired by Phase 4 WI-37.)

---

## 1. Executive summary

Phase 5 is the shape-shift. Phases 1–4 were engineering phases; Phase 5 is **release engineering + product operations + calendar**. Six work items (WI-41 through WI-46) covering updater infrastructure, crash/telemetry, bundled plugin curation, a minimum marketplace, the documentation pass, and the beta-to-GA window.

The audit finds almost everything in this phase is **greenfield** — none of the six WIs has existing scaffolding in-repo. There are no GitHub Actions (`.github/` does not exist), no code-signing scripts, no `tauri-plugin-updater` dep, no panic hook, no HTTP client wired to a telemetry sink, no marketplace index, no published docs site, and no release tag beyond the Phase 0 freeze anchor `v0.1.0-legacy-shell`. What this means is that the scope is well-understood (nothing to rip out), but it also means the **long-lead items must start early**, specifically code-signing certificate procurement (WI-41) and the telemetry-scope user decision (WI-42). Both are calendar risks, not engineering risks.

**Readiness corrections from the audit — all six WIs are fresh:**

| WI | INTEGRATION-REVIEW estimate | Audit finding | Audit-corrected |
|---|---|---|---|
| **WI-41** Auto-update | Implied S–M in the "3–4 weeks" total | **Greenfield + long-lead.** `shell/src-tauri/Cargo.toml` has no `tauri-plugin-updater` dep; `shell/src-tauri/tauri.conf.json` has no `updater` key; there is no CI workflow to build or sign release artifacts (`.github/` absent); there is no code-signing certificate procurement happening today. The engineering is **M** (~1 week wiring + sign/verify) but the **calendar is blocked on certs**: Apple Developer ID + notarization takes 1–5 business days, Windows EV certs are a 3–7 day process with token shipment. | **M engineering + 1–3 week calendar lead for certs.** Flag as critical-path; start procurement before any code lands. |
| **WI-42** Crash reporting & telemetry | Implied M | **Greenfield + policy decision.** No `panic::set_hook` or `catch_unwind` anywhere in `crates/` or `shell/src-tauri/src/`. Shared tracing exists in `crates/nexus-cli/src/main.rs:981` and `crates/nexus-tui/src/main.rs:86` (`tracing_subscriber::fmt` stderr sinks only); the legacy `crates/nexus-app` tracing-init site was retired by Phase 4 WI-37. `reqwest` is in workspace deps at `Cargo.toml:126` (present). No Sentry, no telemetry HTTP client, no opt-in UI. File-as-truth rule from PRD 01 demands **zero note-content** in any submitted report — scoping is the hard part. | **M engineering, but depends on the §6 Q2 scope decision.** Conservative scope = Sentry free tier + stack-trace-only + env fingerprint ≈ 1 week. Broader scope (anon analytics, feature-use counts) = multi-week. |
| **WI-43** Bundled core plugin set | Implied S | **Curation task, not code.** `shell/src/main.tsx:155-196` registers exactly **38 plugins at boot**: 6 core services + 32 nexus feature plugins. Some are clearly load-bearing (activity bar, status bar, editor, files, workspace, files, search, command palette, settings); some are niche or incomplete (skills, workflow, mcp, processes, global graph, bases, canvas). No split today between "ship by default" and "opt-in." | **S (~3 days)** to do the curation, document the policy, and add a gate that surfaces non-default plugins as "available but disabled" in the settings UI. |
| **WI-44** Marketplace (minimal) | Implied M | **Greenfield-minus-stubs.** Phase 4 WI-38 landed `nexus plugin {install,list,remove}` subcommand stubs — `install` prints the "requires marketplace (Phase 5 WI-44)" message and exits 2, while `list --shell` and `remove <id>` already do real work against `~/.nexus-shell/plugins/` (`crates/nexus-cli/src/commands/plugin.rs`). Community plugins today still arrive by being dropped into that directory (`shell/src-tauri/src/lib.rs:54-112` is the scanner). No JSON index hosted anywhere. Phase 3 WI-30 iframe sandbox, WI-31 consent, WI-33 `api_version` surfacing, and Phase 4 WI-39 `--template script` scaffold all landed — plumbing is there; only the "discover + fetch + unpack" layer is missing. | **M (~1 week)** for the minimum: one `nexus-plugins.json` in the main repo, a shell-side Marketplace tab rendering it, one Tauri command to download+unpack a tarball to `~/.nexus-shell/plugins/`, and replacing the `install` stub with a real implementation that hits the same code path. |
| **WI-45** Documentation pass | Implied S–M | **Partial.** Several existing docs: top-level `README.md` (references "Phase 4–5 features as functional" — stale language from pre-migration era; should read post-v1), `shell/README.md` (107 lines, current), `shell/docs/writing-a-plugin.md` (~290 lines, word-count example tutorial — pre-dates Phase 3 sandbox + Phase 4 scaffold), and **a second tutorial shipped by Phase 4 WI-39 at `docs/writing-your-first-plugin.md`** (164 lines, built around `nexus plugin scaffold --template script`). `app/README.md` is gone (retired by Phase 4 WI-37 together with the `app/` directory and `crates/nexus-app/`). 22 other `shell/docs/*.md` files exist. 13+ `docs/*.md` design docs reference Phase 1–3 state, some stale. No published docs website today. | **S (~4 days) for a bounded deliverables checklist.** Scope control is the risk; pin to 5 deliverables max. Reconcile the two plugin tutorials (`shell/docs/writing-a-plugin.md` and `docs/writing-your-first-plugin.md`) — either merge or make their audiences distinct. |
| **WI-46** Beta → GA | Implied 2 weeks calendar (explicitly in the review) | **Ops-only.** No release tag beyond `v0.1.0-legacy-shell`. No `CHANGELOG.md`, no beta channel, no triage rubric, no v1.0.0 criteria artifact, no downloader page. Everything in this WI is logistics: procure beta testers, define a triage SLA, set a go/no-go rubric, execute the 2-week window, cut `v1.0.0`. | **2 weeks calendar + ~3 engineer-days of setup** (CHANGELOG bootstrapping, release-branch process doc, bug-triage board). Not shippable without the prior five WIs. |

**Net effect.** INTEGRATION-REVIEW §5 estimated "3–4 weeks" for all six WIs. The audit-corrected aggregate is:

- **Engineering work:** ~4 weeks (WI-41 M + WI-42 M + WI-43 S + WI-44 M + WI-45 S + WI-46 3d). Sums to 4.0–4.5 engineer-weeks.
- **Calendar work (can overlap engineering):** 1–3 week cert procurement lead-time for WI-41 + the 2-week beta window for WI-46. These are not additive with engineering if you start the cert procurement on day 1.
- **Realistic end-to-end calendar: 5–6 weeks** from "Phase 4 complete" to "`v1.0.0` tag pushed." The review's "3–4 weeks" is achievable *only* if certs are already in hand; the plan treats that assumption as explicit (§6 Q1).

**Shape of the phase.** Unlike Phase 2 (feature parity, all engineering) or Phase 3 (all engineering + one ADR decision), Phase 5 is **bi-modal**:

- **Code-work WIs (41, 42, 43, 44):** look like normal engineering — design, implement, commit, test.
- **Ops/release WIs (45, 46):** look like project management — deliverables checklist, calendar, meetings, go/no-go reviews.

This plan treats them differently: WI-41/42/44 get full design sketches; WI-43/45 get deliverables checklists; WI-46 gets a go/no-go rubric and calendar template.

**Phase 5 acceptance — the v1 gate:**

1. `tauri-plugin-updater` is wired in `shell/src-tauri`; a signed release artifact built by CI installs on all three target platforms (Windows, macOS, Linux) and the in-app "update available" notification appears when a newer build is published.
2. A plugin panic or shell panic produces a Sentry-delivered report (scrubbed per §3.2 policy) if and only if the user has opted in via settings. Default is **opt-out** at first launch.
3. `shell/src/main.tsx` registers only the **curated default set** (count TBD in WI-43 — current floor suggested ≈16–18 plugins); non-default nexus plugins ship on disk but load lazily via the marketplace/enable path.
4. `nexus plugin install <id>` fetches a plugin from the repo-hosted `nexus-plugins.json` index, unpacks it to `~/.nexus-shell/plugins/`, and the shell surfaces it on next boot. The Settings > Plugins UI gains a "Marketplace" tab.
5. Top-level `README.md` reflects post-migration reality (legacy `app/` is gone; no DEPRECATED section left to merge). `shell/docs/writing-a-plugin.md` either incorporates or is superseded by Phase 4's `docs/writing-your-first-plugin.md` (which already references the WI-39 `--template script` scaffold command). `CHANGELOG.md` exists and covers v0.1.0 → v1.0.0.
6. Beta phase has run its two-week cycle with a triage SLA met; a go/no-go review happens on Day 14; `v1.0.0` is tagged and published on the updater channel.

---

## 2. Scope summary

### 2.1 Partitioning

Phase 5 splits naturally into three sub-phases by shape-of-work:

- **Phase 5a — Release infrastructure (P0, ~2 weeks engineering + cert lead).** The updater + signing + crash reporting pipeline. Must land before a beta can run. WI-41, WI-42.
- **Phase 5b — Plugin ecosystem (P0/P1, ~1.5 weeks engineering).** Curated defaults + marketplace + install command. The "plugins work like a product" beat. WI-43, WI-44.
- **Phase 5c — Ship (P0, ~3 days setup + 2 weeks calendar).** Docs pass, beta, GA. WI-45, WI-46.

### 2.2 Work items by priority

| ID | Title | Size | Priority | Sub-phase | Blocks beta? |
|---|---|---|---|---|---|
| **WI-41** | Tauri auto-updater + code-signing + release channel | M (+cert lead) | P0 | 5a | **Yes** |
| **WI-42** | Crash reporting & telemetry (Sentry-minimal) | M | P0 | 5a | **Yes** |
| **WI-43** | Bundled core plugin set curation | S | P1 | 5b | Optional |
| **WI-44** | Minimal marketplace (JSON index + shell UI + CLI install) | M | P1 | 5b | Optional |
| **WI-45** | Documentation pass (bounded deliverables checklist) | S | P0 | 5c | **Yes** |
| **WI-46** | Beta → GA (2-week window + triage + v1.0.0 tag) | Ops | P0 | 5c | N/A (is the ship) |

**Blocks-beta reasoning:** A beta build must be installable via the updater (WI-41) and must produce crash reports if it crashes (WI-42). If it has stale docs, testers will file bugs on the docs (WI-45 must be merged before beta). WI-43 and WI-44 are desirable for v1 but slip-tolerant: if marketplace is late, users can still manually install plugins — that's the §6 Q6 scope-cut recommendation.

**Total Phase 5:** 6 WIs; ~4 engineer-weeks of code + 5–6 weeks calendar (calendar expanded by cert lead + beta window). §5 dependency graph shows the critical path runs through WI-41 signing.

---

## 3. Phase 5a work items (release infrastructure)

---

### 3.1 WI-41 — Tauri auto-updater + code-signing + release channel (M + cert lead, P0)

#### 3.1.1 Intent

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

## 4. Phase 5b work items (plugin ecosystem)

---

### 4.1 WI-43 — Bundled core plugin set curation (S, P1)

#### 4.1.1 Intent

Today's shell boots 38 plugins. For v1 that's a product-positioning problem: users see "Workflow", "MCP", "Skills", "Global Graph", etc. enabled by default — and most of those plugins are incomplete or niche. For the v1 release we want the default set to feel **focused**: the plugins a first-time user needs to understand "this is a file-based markdown notebook with a search engine and a plugin API."

The WI **does not** delete plugins. It splits the set into:

1. **Default-on** — loaded at boot, user sees them.
2. **Default-off** — shipped in the binary but not registered by `main.tsx`; user discovers them via the Settings > Plugins tab with a one-click enable.

#### 4.1.2 Current state

- `shell/src/main.tsx:155-196` enumerates 38 plugin imports and registers them unconditionally.
- Split on disk: `shell/src/plugins/core/*` has 16 directories (most are stub reference UI — **only 5 are actually registered**: `configurationService`, `notificationService`, `fileSystemService`, `settings`, `capabilityPrompt`, `themeService`). The remaining 11 core directories (`activityBar`, `commandPalette`, `editorArea`, `fileExplorer`, `panelArea`, `rightPanel`, `sidebar`, `statusBar`, `terminal`, `titleBar`) are reference UI left over from the shell template — `main.tsx:40-54` comments explicitly "UI & feature plugins (DISABLED) ... retained on disk as reference only."
- `shell/src/plugins/nexus/*` has 31 directories, 32 plugins registered (`graphPlugin` + `graphGlobalPlugin` both from `graph/`).
- Phase 2 WI-19 added activation events (deferred activation). Plugin **loading** still happens at boot; **activation** now waits for a trigger. So the 38 plugins all take disk/memory even if they never activate. WI-43 adds a level above activation: "not even in the plugin list."

No split exists between "core to v1" and "installable." That's the gap.

#### 4.1.3 Design sketch

**Part 1 — Categorize (0.5 day).**

Draft curation below — open to §6 Q3 override:

**Default-on (v1 essentials, ~18 plugins):**
- All 6 core services: `configurationService`, `notificationService`, `fileSystemService`, `settings`, `capabilityPrompt`, `themeService`.
- Workspace + git: `workspacePlugin`, `gitStatusPlugin`.
- Frame: `activityBarPlugin`, `sidebarPlugin`, `rightPanelPlugin`, `statusBarPlugin`, `launcherPlugin`.
- File navigation: `filesPlugin`.
- Editor: `editorPlugin`, `outlinePlugin`.
- Interaction: `commandPalettePlugin`, `confirmPlugin`, `paneModePlugin`.
- Search: `searchPlugin`.
- Settings UX: `pluginsMgmtPlugin`.

That's 19. This is the "it boots and you can read/write/search notes with a file tree" minimum.

**Default-off (shipped-but-dormant, ~14 plugins):**
- `aiPlugin`, `agentPlugin` — require provider setup; surface behind an explicit enable.
- `mcpPlugin`, `workflowPlugin`, `skillsPlugin` — niche for v1 audience.
- `terminalPlugin`, `processesPlugin` — power-user surfaces.
- `graphPlugin`, `graphGlobalPlugin` — compelling demo but not v1-essential.
- `canvasPlugin`, `basesPlugin` — Obsidian-faithful but visually heavy for a first-launch impression.
- `backlinksPlugin`, `bookmarksPlugin`, `outgoingLinksPlugin`, `filePropertiesPlugin`, `tagsPlugin`, `allPropertiesPlugin` — sidebar leaves that compete with a clean default layout.

Some of those (backlinks, tags) might move back to default-on after a §6 Q3 decision — this plan takes the conservative cut and flags it.

**Part 2 — Implement the split (1 day).**

Introduce `shell/src/plugins/catalog.ts`:
```ts
export const DEFAULT_ON_PLUGINS: Plugin[] = [ configurationServicePlugin, ... ]
export const DEFAULT_OFF_PLUGINS: Plugin[] = [ aiPlugin, ... ]
export const ALL_PLUGINS = [...DEFAULT_ON_PLUGINS, ...DEFAULT_OFF_PLUGINS]
```

In `main.tsx:155-196`, replace the inline array with `DEFAULT_ON_PLUGINS` for the first boot, then merge in user-enabled DEFAULT_OFF ones from a `settings.json` field `plugins.enabled: string[]`.

In `pluginsMgmtPlugin`, add an "Available (disabled)" section rendering the DEFAULT_OFF set with a per-row "Enable" button. Clicking Enable writes the plugin ID to `plugins.enabled`; next reload (or in-session, via the already-existing `host.loadAll`) brings it in.

**Part 3 — User doc (0.5 day).**

Update `shell/docs/core-plugins.md` (already exists; audit) to describe the default-on/default-off split. Add a one-paragraph note in `README.md` about where to find the "available plugins" list.

**Part 4 — Telemetry hook (0.5 day).**

With WI-42 in flight, emit an anonymized event ("plugin.enable: com.nexus.ai", no content) on a Enable action from the settings UI. This is valuable signal: we'll know which default-off plugins users actually enable and can reconsider the default-on cut for v1.1.

#### 4.1.4 Subagent pattern

**Agent 1 (curation investigation).** Prompt: *"Read the 32 nexus.* plugins in `shell/src/plugins/nexus/` and score each on (a) completeness (does it render anything?), (b) standalone-value (does it work without the AI provider / MCP server?), (c) first-launch fit. Produce a ranked list with justification. ~1000 words."*

Main-thread: evaluate the ranking, decide the cut, implement the catalog + mgmt UI.

#### 4.1.5 Commit plan

1. `refactor(shell): split plugin registrations into default-on / default-off catalog` — new `catalog.ts`, updated `main.tsx`.
2. `feat(plugins-mgmt): Available section renders default-off plugins with Enable button` — pluginsMgmt UI.
3. `docs(shell): document curated default-on plugin set policy` — `shell/docs/core-plugins.md` update + `README.md` paragraph.

**Files touched:**
- `shell/src/plugins/catalog.ts` — new (~80 lines).
- `shell/src/main.tsx` — refactor.
- `shell/src/plugins/nexus/pluginsMgmt/*.tsx` — Available section.
- `shell/docs/core-plugins.md` — policy doc.

#### 4.1.6 Acceptance

- Fresh install of v1.0.0 with empty `~/.nexus-shell/` shows exactly the DEFAULT_ON plugin set in the activity bar + sidebar + status bar.
- Settings > Plugins shows "Installed" (default-on) + "Available" (default-off) sections.
- Clicking Enable on a default-off plugin, then reloading the shell, surfaces the plugin with its contributions intact.
- `grep -c "import.*Plugin" shell/src/plugins/catalog.ts` equals exactly the current 38 (no regressions).
- Manual first-launch test: a new user can write and search notes without enabling anything beyond the default.

#### 4.1.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| The curation cut removes a plugin a power user depends on; they feel punished | Medium | Default-off is one-click-enable, not "uninstall." §6 Q3 offers to surface the cut to the user for review. Document the rollback (edit catalog.ts) in `core-plugins.md`. |
| The Enable flow has a bug; user clicks enable, nothing happens | Low | Reuse the existing persistence pathway from `set_plugin_enabled` / plugin consent; don't invent a new write path. |
| Telemetry hook on Enable action feels invasive | Low | Gated behind WI-42 opt-in. Already anonymized. |
| Activation-events infrastructure from Phase 2 WI-19 conflicts with the default-off idea (double gating) | Low | No conflict: default-off gates *registration*, activation events gate *activation* after registration. Both compose. |

#### 4.1.8 Size

**S** — ~2.5 engineer-days. Breakdown: 0.5d curation, 1d catalog + mgmt UI, 0.5d docs, 0.5d tests.

---

### 4.2 WI-44 — Minimal marketplace (M, P1)

#### 4.2.1 Intent

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

## 5. Phase 5c work items (ship)

---

### 5.1 WI-45 — Documentation pass (S, P0)

#### 5.1.1 Intent

Every shipped product needs docs a new user can land on without a guide. Nexus has *a lot* of docs already — `docs/` has 30+ markdown files covering design decisions, migration plans, and architecture; `shell/docs/` has 22 files covering the plugin contract; `packages/nexus-extension-api/README.md` covers the TS author surface. What's missing is **a single, audited, up-to-date entry path for a new user**: "I've heard about Nexus; I'm on nexus.dev; what do I read?"

The WI is deliberately bounded: **5 deliverables max**. Scope drift here is the single biggest calendar risk in Phase 5.

#### 5.1.2 Current state — audit

- **`README.md` (top-level, 242 lines):** Last audited 2026-04-23. Says "Alpha (v0.1.0) — Phase 1 foundation is solid, Phase 4-5 features (AI, MCP) are functional. Not yet production-ready." The "Phase N" numbering here refers to the **PRD** phase numbering (Phase 4 = AI, Phase 5 = MCP) which *completely conflicts* with the migration-roadmap phase numbering (Phase 5 = v1 polish). This is a known source of confusion. The readme also has a `⚠️ Desktop shell freeze` block referencing the correct migration state. **Stale: yes — the "Phase 4-5 features functional" line reads as pre-release in a post-release context.**
- **`app/README.md`:** Gone. Phase 4 WI-37 retired the `app/` directory and `crates/nexus-app/` on 2026-04-24; the `74b098f` follow-up cleaned up the 28 orphan `app/src/bindings/*.ts` files. No Phase 5 action needed. Historical context (freeze rationale, migration outcome) lives in `docs/legacy-shell-retirement.md`.
- **`shell/README.md` (107 lines):** Current. Describes the plugin-first shell. Links to `shell/docs/`. **Needs a "Plugins that ship by default" section** once WI-43 curates the list.
- **Two plugin tutorials now exist.** `shell/docs/writing-a-plugin.md` (~290 lines, word-count plugin, pre-dates Phase 3 sandbox + Phase 4 scaffold) is the older one; `docs/writing-your-first-plugin.md` (164 lines, shipped by Phase 4 WI-39, built around `nexus plugin scaffold --template script`) is the newer one. WI-45 must reconcile them — candidate split: `docs/writing-your-first-plugin.md` = quickstart tutorial (the scaffold path), `shell/docs/writing-a-plugin.md` = in-depth guide (activation events, sandbox model, capability declaration, slot system). Or fold both into one file and redirect.
- **`shell/docs/*` (22 files):** Architecture, plugin-system, extension-host, context-keys, slot-system, etc. Mostly current. Audit for "Phase N" references.
- **`docs/*.md` (30+ files):** Many are design-phase documents from the migration planning era. Phase 5 should **not** rewrite these — they are historical planning artifacts, not user docs. They can move to `docs/planning/` or `docs/archive/` to signal "reference only."
- **`packages/nexus-extension-api/README.md`:** Current per Phase 1 + 3 updates. Lists API surface. Could use a "Quick start" section.
- **`CHANGELOG.md`:** Does not exist. Will be created in WI-41 scaffold.
- **`docs/RELEASE-RUNBOOK.md`:** Will be created in WI-41.
- **`docs/TELEMETRY-POLICY.md`:** Will be created in WI-42.
- **`docs/PUBLISHING-A-PLUGIN.md`:** Will be created in WI-44.
- **No published site.** `shell/docs/` lives only in the repo. The INTEGRATION-REVIEW §5 item says "publish `shell/docs/` to a website" — the §6 Q5 recommendation is to defer a real docs site (VitePress/Docusaurus) to v1.x and land v1 with polished markdown served via GitHub's repo viewer.

#### 5.1.3 Design sketch — the 5 deliverables

1. **`README.md` rewrite (top-level).** Retarget to "v1 is out, here's what this is." Remove "Phase 4-5 features" language (PRD phases — conflicting numbering). Lead with a screenshot (or ASCII-art equivalent). Three-sentence pitch. Install link (installers from GitHub Releases). Link to: docs overview, plugin marketplace, release runbook, changelog. Remove the `⚠️ Desktop shell freeze` block — the freeze is already resolved (legacy shell deleted in Phase 4 WI-37). ~150 lines. **Target: 1 hour to read cover-to-cover.**

2. **`docs/README.md` (new): docs landing page.** One-page navigation hub that links to the most important docs by audience:
   - *New user:* "Install", "First note", "Writing your first plugin."
   - *Plugin author:* `shell/docs/writing-a-plugin.md`, `shell/docs/plugin-api.md`, `packages/nexus-extension-api/README.md`, `docs/PUBLISHING-A-PLUGIN.md`.
   - *Developer / contributor:* `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/adr/`.
   - *Release engineer:* `docs/RELEASE-RUNBOOK.md`, `docs/TELEMETRY-POLICY.md`.
   ~80 lines.

3. **Plugin tutorials — reconcile the two that now exist.** `docs/writing-your-first-plugin.md` (Phase 4 WI-39, 164 lines, script-template scaffold path) and `shell/docs/writing-a-plugin.md` (~290 lines, pre-Phase-3 word-count example). Bring both current with Phase 1–4 reality — activation events, `@nexus/extension-api` path, sandbox model (`sandboxed: true`), capability declaration, the `nexus plugin scaffold --template script` command. Recommended split: keep the WI-39 tutorial as the quickstart, rework `shell/docs/writing-a-plugin.md` into an in-depth reference, cross-link both. Add "publishing your plugin to the marketplace" section linking to `docs/PUBLISHING-A-PLUGIN.md`. ~80 lines added across both, ~40 revised.

4. **`docs/ARCHITECTURE.md` audit.** Currently exists (file present); verify it reflects post-migration reality (shell is the sole desktop target, `crates/nexus-app` is deleted, kernel crate DAG is the one in INTEGRATION-REVIEW §2.1). Light edit, not a rewrite. ~50 edits.

5. **`docs/planning/` archive move.** Mechanical: move `PHASE-1..5-IMPLEMENTATION-PLAN.md`, `INTEGRATION-REVIEW.md`, `UI-AUDIT.md`, `MICROKERNEL-AUDIT.md`, `SHELL-COMPARISON.md`, `PARITY-CHECKLIST.md`, etc. — everything that is a *planning artifact* — to `docs/planning/`. Leave design docs that describe current architecture (`ARCHITECTURE.md`, `leaf-architecture.md`) in place. Add a `docs/planning/README.md` pointer explaining what this directory is. ~30 lines.

**Explicitly not in scope:**
- A docs site (VitePress / Docusaurus). §6 Q5 defers to v1.x.
- A video walkthrough.
- A multi-page plugin-API reference rewritten from TS types (that's what the `@nexus/extension-api` package is for).
- A migration guide for v0.x → v1.0 users (we expect zero external v0.x users; the beta will be the first external signal).

#### 5.1.4 Subagent pattern

**Agent 1 (README rewrite).** Prompt: *"Rewrite top-level README.md as a v1 product landing. Remove pre-migration language. Include: 3-sentence pitch, install instructions, four top-level links, screenshot placeholder. ~150 lines. Match the tone of [1-2 reference READMEs we like, e.g., Obsidian, Helix]."*

**Agent 2 (docs landing).** Prompt: *"Create `docs/README.md` as an audience-indexed navigation hub. Four audience sections (new user, plugin author, contributor, release). Link to existing docs; don't duplicate content. ~80 lines."*

**Agent 3 (plugin tutorial reconciliation).** Prompt: *"Two plugin tutorials exist: `docs/writing-your-first-plugin.md` (quickstart, scaffold-driven) and `shell/docs/writing-a-plugin.md` (in-depth, word-count example). Reconcile: make the first a true quickstart that ends by pointing at the second; make the second a reference that assumes the reader scaffolded via `nexus plugin scaffold --template script`. Mention activation events, @nexus/extension-api import path, sandbox model, capability declaration, publishing to marketplace. Diff only."*

Main-thread: do the archive move manually (it's 8 `git mv` commands), do the top-level README final pass.

#### 5.1.5 Commit plan

1. `docs: rewrite top-level README for v1 launch` — README.md.
2. `docs: add docs landing page with audience-indexed navigation` — docs/README.md.
3. `docs: reconcile plugin tutorials (quickstart + in-depth)` — docs/writing-your-first-plugin.md + shell/docs/writing-a-plugin.md.
4. `docs: audit ARCHITECTURE.md post-migration` — docs/ARCHITECTURE.md.
5. `docs: move planning artifacts to docs/planning/` — `git mv` + planning/README.md.

**Files touched (~12 files, mostly edits):**
- `README.md` — rewrite.
- `docs/README.md` — new.
- `docs/planning/README.md` — new.
- `docs/planning/*` — moves (8+ files).
- `docs/writing-your-first-plugin.md` — edit (quickstart).
- `shell/docs/writing-a-plugin.md` — edit (in-depth guide).
- `docs/ARCHITECTURE.md` — edit.
- `shell/README.md` — small additions.

#### 5.1.6 Acceptance

- A new user who lands on `README.md` can, within 5 minutes of reading, (a) install Nexus, (b) find the docs hub, (c) know where to go next.
- A plugin author who lands on `docs/README.md` → "plugin author" section reaches `shell/docs/writing-a-plugin.md` in one click.
- No doc references stale Phase-N terminology (verified via `grep -n "Phase [0-9]" README.md shell/README.md docs/README.md | grep -v "planning"` — all remaining references are in archive-marked files).
- The `docs/planning/` archive pattern is explained in its own README so future maintainers understand why historical plans are preserved in place.

#### 5.1.7 Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Doc scope creep eats a week of Phase 5 | High | Bound to 5 deliverables (§5.1.3). Reject anything else as "v1.x." |
| Inaccurate docs written without code check land | Medium | Every code snippet in a rewritten doc gets copy-pasted from a working file. `grep` references to verify symbols exist. |
| Planning archive move creates broken links all over the repo | Medium | Do the move in one commit; `grep -rn "PHASE-1-IMPL" docs/ shell/ README.md` before and after, fix all references. |
| The README pitch is bland | Low | Iterate with a trusted reviewer. Not a calendar risk; minor polish. |
| A docs site (VitePress) feels necessary mid-WI and derails scope | Medium | §6 Q5 closes this explicitly before WI-45 starts. Decision is **no site in v1.** |

#### 5.1.8 Size

**S** — ~4 engineer-days. Breakdown: 1d README rewrite + landing page, 1d writing-a-plugin update, 0.5d ARCHITECTURE audit, 0.5d archive move + broken-link sweep, 1d review + polish.

---

### 5.2 WI-46 — Beta → GA (Ops, P0)

#### 5.2.1 Intent

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
6. Roadmap for v1.1 goes into `docs/planning/ROADMAP.md`. Closes Phase 5.

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

## 6. Dependency graph

```
Phase 5 critical path — left-to-right, time flows right

[cert procurement]──┐
  (calendar, 1-3w)  │
                    ▼
                 ┌───────────────────┐
                 │  WI-41  Updater   │  ─────┐
                 │  + signing + CI   │       │ (blocks beta)
                 │       (M)         │       │
                 └─────────┬─────────┘       │
                           │                 │
                           ▼                 ▼
                 ┌───────────────────┐  ┌───────────────────┐
                 │  WI-42 Crash      │  │  WI-45 Docs pass   │  (can start anytime)
                 │  reporting (M)    │  │       (S)          │
                 └─────────┬─────────┘  └─────────┬─────────┘
                           │                      │
                           ▼                      ▼
                 ┌───────────────────┐  ┌───────────────────┐
                 │  WI-43 Plugin     │  │  WI-46 Beta kick  │  (starts after 41+42+45)
                 │  curation (S)     │  │       (ops)        │
                 └─────────┬─────────┘  └─────────┬─────────┘
                           │                      │
                           ▼                      ▼
                 ┌───────────────────┐  ┌───────────────────┐
                 │  WI-44 Marketplace│  │  2-week beta +    │
                 │       (M)         │  │  go/no-go + v1.0.0│
                 └───────────────────┘  └───────────────────┘
                           │                      │
                           └──────── v1.0 ────────┘
```

**Parallelism cheat-sheet:**

- **Track A (release infra):** WI-41 → WI-42. ~2 weeks. Blocks beta.
- **Track B (docs + curation):** WI-43 + WI-45 in any order, then optionally WI-44. ~1.5 weeks. Can run in parallel to Track A.
- **Track C (beta/GA ops):** WI-46. Starts after Track A finishes (so the beta can install and report crashes). 2+ weeks calendar.

**Single-engineer serial:** ~5–6 calendar weeks (certs lead blocks start; beta is a sunk calendar cost).

**Single-engineer with parallel cert procurement:** ~4 weeks. While waiting for certs, do WI-42 (Sentry wiring tolerates no certs yet), WI-43 (curation is just TypeScript), WI-45 (markdown), and pre-stage WI-44.

**Two-engineer:** ~3 weeks. Engineer A on WI-41 + WI-46 triage; Engineer B on WI-42 + WI-44 + WI-43 + WI-45. WI-46 go/no-go stays a single decision-maker.

**Phase 4 coordination (resolved — Phase 4 shipped 2026-04-24):**
- WI-38 (Phase 4, commits `d22b5b6`/`99e9bc8`/`1a7649d`) unified `nexus` binary + added `plugin install|list|remove` subcommand stubs. WI-44 turns the `install` stub into a real marketplace fetcher.
- WI-39 (Phase 4, commits `b83d37f`/`ebe1ee2`) shipped `--template script` scaffold + `docs/writing-your-first-plugin.md` tutorial. WI-45 reconciles that tutorial with the older `shell/docs/writing-a-plugin.md` (see §5.1.3).
- WI-37 (Phase 4, commits `38a1e82`/`e7c6c5a` + `74b098f` follow-up) retired `crates/nexus-app` and `app/`. WI-45 README rewrite simply removes the freeze-notice block; nothing to point at.

---

## 7. Risks & mitigations (cross-WI)

Per-WI risks are in §3–§5. Cross-WI and project-level risks:

| Risk | Severity | Mitigation |
|---|---|---|
| Apple/Microsoft code-signing cert not secured before Phase 5 starts | High | **Action §9 step 1 — procure now.** If blocked, the whole phase slides. |
| `tauri-plugin-updater` has a showstopper issue on one platform (e.g., macOS Apple Silicon) | Medium | Test on all 3 platforms in the first week. Have a manual-download fallback if auto-update is broken for one platform in v1. |
| Sentry free-tier events-per-month cap hit during beta | Medium | See WI-42 risk. Rate-limit client side. Upgrade tier if it's close. |
| Marketplace PR-review-by-first-party process becomes a bottleneck as volume grows | Low for v1 | v1 expects <10 marketplace entries. If volume grows post-v1, automate with a repo-owner bot. |
| Beta reveals a cross-platform bug that costs multiple days and slips the v1 tag | High | The rubric's "5 business days without S0" is the buffer. If a bug takes 3 days to fix, ship is delayed 3 days past that fix + the 5-day stability window. That's accepted. |
| Docs drift — the WI-45 pass happens early, then WI-41/42/44 code lands and docs re-stale before v1 | Medium | WI-45 runs in Week 3–4, after the code-work WIs settle. Do a final audit pass in WI-46 Week 0 as part of "pre-beta readiness." |
| v1.0 ships with a telemetry default-on bug and users complain | High | Tests enforce default-off. Manual first-launch test is part of the go/no-go rubric. |
| Post-v1 support burden underestimated | Medium | Plan explicitly acknowledges v1.1 as a patch-release cycle (§6 Q6 partial defer). |
| `@nexus/extension-api` package lacks an npm publish path; marketplace plugins can't easily `npm install @nexus/extension-api` | Medium | §6 Q7 surfaces the decision. Default: publish to npm as `@nexus/extension-api` with version matching Nexus release. Coordinate in WI-41. |
| PRD phase numbering (Phase 4 = AI, Phase 5 = MCP) conflicts with migration phase numbering (Phase 5 = v1 polish) | Medium | WI-45 README rewrite resolves by dropping the migration-phase language from user-facing docs. Internal planning docs keep their numbering but live in `docs/planning/`. |
| An engineer new to the project in v1.x cannot cut a release | Medium | `docs/RELEASE-RUNBOOK.md` (WI-41) must be complete enough that a second engineer can cut a release without the original author's help. This is tested during WI-46 (a second engineer cuts the beta tag as a dry run). |

---

## 8. Open questions for user before execution

These decisions materially shape the phase; they should be resolved at Phase 5 kickoff, before WI-41 engineering starts. Defaults in this plan apply if otherwise.

### Q1 — Code-signing certs: in hand, in progress, or not started?

Options:
- **In hand (Apple Developer ID + Windows cert).** Great; WI-41 engineering can start day 1 with no lead time.
- **In progress.** Proceed with WI-42/43/45 engineering while certs finalize; WI-41 wiring lands last.
- **Not started.** **Action item: start procurement before Phase 5 kicks off.** Apple: 1–5 business days. Windows EV: 3–7 days. Otherwise the plan's 5–6 week calendar becomes 7–9 weeks.

**Recommendation:** confirm status today. If anything less than "in hand", **step zero of Phase 5 is the procurement kickoff**, in parallel with code work on WI-42/43/45.

### Q2 — Telemetry scope for v1

Options:
- **Sentry crash-only, opt-in, default-off** (the plan's default). Minimum viable. No feature analytics. ~5 engineer-days.
- **Sentry + feature-use counters** (which plugin opened, command executed). Opt-in, default-off. Adds a meaningful signal for product decisions but introduces a bigger "what do we capture" surface to audit. ~2 extra engineer-days.
- **Self-hosted crash reporter** (GlitchTip, or a minimal S3 bucket + PagerDuty-style alert). Full data sovereignty. Higher setup cost. ~1 extra engineer-week.
- **Email-only "send crash dump" button.** No third-party SaaS. User gets a modal on crash; they can paste into an email. Zero network telemetry. Low signal volume. ~2 engineer-days.

**Recommendation:** Sentry crash-only opt-in for v1 (option 1). File-as-truth rule is already non-trivial to enforce; keeping the scope tight helps. Broader analytics is a v1.x conversation.

### Q3 — Default-on plugin curation

The proposed split (§4.1.3) is 19 default-on, 14 default-off. Open to override. Specific calls to flag:

- **Backlinks, tags, outgoing-links:** Obsidian users expect these by default. Moving them to default-off saves panel real estate but hurts feature-parity positioning. Recommend: keep default-on.
- **AI, agent:** require user provider config (Anthropic/OpenAI API key or local model). Default-on means users see "AI Chat" with no config and get a "setup required" state on open. Recommend: default-off.
- **Canvas, bases:** heavy visual plugins; a first-launch gets simpler without. Recommend: default-off but aggressively market them in docs + launch.
- **Graph (global):** demo magnet. Recommend: default-off, but add a "See your notes as a graph" nudge in the welcome view.

**Recommendation:** ship the conservative cut (§4.1.3). Measure telemetry post-launch to see which default-off get enabled most; promote to default-on in v1.1 based on data.

### Q4 — Marketplace scope for v1

Options:
- **Minimal (plan default):** one JSON file in main repo, one tarball per entry on GitHub Releases, author submits via PR. ~5 engineer-days.
- **Ambitious:** separate `nexus-plugins` repo, CI-verified entries, explicit `v1.0.0` tag for the marketplace schema. ~2 engineer-weeks.
- **Deferred:** no marketplace in v1; users drop dirs manually. Post-v1 feature. **Saves ~1 week.**

**Recommendation:** minimal (option 1). The §6 Q6 scope-cut recommendation is to defer this *if* the phase is running hot — users can still manually install.

### Q5 — Docs site vs. markdown-in-repo for v1

Options:
- **Markdown-in-repo (plan default).** GitHub's viewer is the "site." No build pipeline. Docs live where code lives. Zero calendar cost.
- **VitePress / Docusaurus site.** Polished navigation, search, versioning, code-highlighting. Adds a CI build + deploy. ~3–5 engineer-days setup + ongoing maintenance.

**Recommendation:** markdown-in-repo. Docs site is a v1.x item once we have 10+ user-facing docs and an audience that demands navigation. Ship v1 without the site drag.

### Q6 — If Phase 5 runs hot, which WI is the natural defer?

Options:
- **Defer WI-44 Marketplace.** Users drop plugin dirs manually (current state). ~1 week saved. Most natural defer — the underlying mechanism already works.
- **Defer WI-43 Curation.** Ship 38 plugins on first-launch. Users get a fuller-but-busier initial experience. ~3 days saved. Reasonable but a worse first-launch impression.
- **Defer WI-46 beta window.** Cut v1.0.0 without an external beta. 2 weeks saved but *enormously* risky — unfound ship-blocker bugs ruin the launch. **Not recommended.**
- **Defer nothing; slip the ship date.** Treat v1 as "ships when ready." ~2–4 week slip expected.

**Recommendation:** if forced to choose, defer WI-44 to a v1.0.1 patch release. Everything else is either essential (41, 42, 45, 46) or very cheap (43).

### Q7 — `@nexus/extension-api` npm publish for v1?

Today `packages/nexus-extension-api` is a workspace package consumed via `workspace:*` in `shell/package.json`. External plugin authors writing outside this repo can't `npm install @nexus/extension-api` because nothing is published.

Options:
- **Publish to npm at v1.0.0** (matching Nexus release). Authors get a clean `npm install --save-dev @nexus/extension-api@1.0.0` experience. ~1 engineer-day setup, ongoing release coordination.
- **Defer to v1.1.** Authors for now vendored-copy the `.d.ts` files. Acceptable for the ~10 marketplace entries expected in v1.
- **Publish under a dev scope** (e.g., `@nexus-dev/extension-api@1.0.0-preview`) until the API is considered stable post-v1.

**Recommendation:** publish at v1.0.0 (option 1). The API has been stable through Phases 1–3; Phase 1 WI-20 + Phase 3 WI-30 were the big churn moments. Ship alongside v1.0.0 as part of the WI-41 release workflow.

### Q8 — Beta channel: public pre-release tag or private email list?

Options:
- **Public pre-release:** `v1.0.0-beta.N` tag on GitHub Releases with "pre-release" flag. Anyone can download. Public bug reports.
- **Private email list:** testers get an unlisted installer URL. Reports come in via a private channel. Signal-to-noise is higher but diversity is lower.
- **Hybrid:** public pre-release for install, private channel for coordinated communication. **Recommended.** Casual testers self-serve; invited testers get dedicated support.

**Recommendation:** hybrid.

---

## 9. What this plan does NOT cover

- **Anything in Phase 4** — frontend unification, `crates/nexus-app` retirement, nexus binary unification, plugin scaffold command, MCP parity. Separate plan, shipped WI-36 through WI-40 on 2026-04-24.
- **A public docs website.** §6 Q5 defers to v1.x.
- **Plugin-signing / trusted-publisher model.** Phase 6.
- **Auto-update for community plugins.** WI-44 plan has this as an explicit non-goal for v1.
- **Analytics / feature-use telemetry.** §6 Q2 defaults to "no" for v1.
- **Enterprise / self-hosted update endpoint.** Mentioned as a WI-41 mitigation but the feature itself is v1.x.
- **Localization / i18n.** Post-v1.
- **Accessibility audit.** A partial pass happens during the beta (via reports); a formal audit is v1.x.
- **Migration tooling for v0.x → v1.0 users.** We expect ~zero external v0.x users; the beta group is the first real surface.
- **v1.1 roadmap.** WI-46 commits to drafting it after v1.0 ships; content is not this plan.
- **Resource budgets / per-plugin CPU/memory limits** (UI F-8.3.x). Was v1-stretch per PARITY-CHECKLIST; deferred to v1.x.
- **Crash dump uploader UI outside the settings opt-in.** Basic flow only in v1.
- **Automated release tests** beyond what the release workflow already runs. v1 ships with manual verification of each installer on each platform.

---

## 10. Next action

1. **Resolve §8 Q1 (certs) today.** This is the single biggest calendar risk; anything less than "in hand" means day zero of Phase 5 is cert procurement.
2. **Resolve §8 Q2 (telemetry scope).** This shapes WI-42 design day one.
3. **Start WI-42 and WI-45 engineering in parallel** — both are independent of the cert lead.
4. **Start WI-41 engineering as soon as the Apple/Microsoft enrollment is confirmed or provisional.** The code can land unsigned, signing is a configuration step later.
5. **Schedule beta tester recruitment** (WI-46 Week 0 step 1) immediately so outreach is underway before Phase 5a engineering wraps.
6. ~~Coordinate with Phase 4 agent~~ — Phase 4 shipped 2026-04-24; WI-37, WI-38, WI-39, WI-40 are all live and informing WI-44 (marketplace stubs to fill in) and WI-45 (two plugin tutorials to reconcile).
7. **Draft `docs/TRIAGE-RUBRIC.md`** in Phase 5 Week 0. It's cheap and it anchors the go/no-go decision in WI-46.
8. **Decide §8 Q7 (`@nexus/extension-api` npm publish) before WI-41 lands** so the release workflow knows whether to do `npm publish`.
9. **Land the `CHANGELOG.md` scaffold** in the WI-41 first commit. Populating it is then a habit, not a last-minute rush.

Each WI's commit plan is self-contained (§3–§5); land them incrementally per the phase-0 workflow. The v1.0.0 tag is the terminal commit; everything else is a step toward it.
