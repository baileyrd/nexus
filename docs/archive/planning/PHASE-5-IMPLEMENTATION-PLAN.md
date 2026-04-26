# Phase 5 Implementation Plan — Personal-Tool Polish

**Status:** Active
**Date:** 2026-04-24 (rescoped from original v1-release plan)
**Scope:** Nexus is a personal tool. Release infrastructure (code-signing, auto-updater, marketplace, telemetry, beta/GA) is deferred to [`REQUIRED-FOR-FORMAL-RELEASE.md`](./REQUIRED-FOR-FORMAL-RELEASE.md).

---

## 1. What Phase 5 actually does now

Three small work items, ~2–3 engineer-days of real work:

| WI | Title | Size | Why it's still worth doing |
|---|---|---|---|
| **WI-43** | Default-on / default-off plugin curation | S (~1d) | The 38-plugin boot set is noisy for personal use. Curating to ~19 default-on with one-click enable for the rest reduces own-dogfood friction. |
| **WI-45-lite** | Local docs reconcile (README de-stale + tutorial merge + planning archive move) | S (~1d) | Top-level `README.md` has stale Phase-N language and a resolved freeze banner. Two plugin tutorials exist and contradict each other. Fix in-repo; skip the polished-for-public pass. |
| **WI-47** | Local panic log (replaces WI-42 Sentry) | XS (~2h) | `panic::set_hook` in `shell/src-tauri` and `nexus-cli` writing a rotating file to `~/.nexus-shell/logs/panic.log`. No network, no opt-in UI — just catch panics locally so they're not lost when the terminal closes. |

**Not in scope (moved to [`REQUIRED-FOR-FORMAL-RELEASE.md`](./REQUIRED-FOR-FORMAL-RELEASE.md)):**
- WI-41 auto-updater + code-signing + release channel
- WI-42 Sentry / network telemetry (replaced locally by WI-47)
- WI-44 marketplace (manual drop into `~/.nexus-shell/plugins/` remains the install path)
- WI-46 beta → GA (no external users)
- Public docs website, CHANGELOG for external versions, RELEASE-RUNBOOK, TELEMETRY-POLICY, PUBLISHING-A-PLUGIN, TRIAGE-RUBRIC, BETA-TESTER-ONBOARDING

---

## 2. WI-43 — Default-on / default-off plugin curation

### Current state
- `shell/src/main.tsx:170-209` registers 38 plugins unconditionally (6 core services + 32 nexus.* plugins).
- `shell/src/plugins/core/*` has 16 directories but only 6 are registered; the rest are template reference UI (see `main.tsx:51-56`).
- Phase 2 WI-19 added activation events (deferred activation) but *registration* still happens for all 38 at boot.

### Design
1. New file `shell/src/plugins/catalog.ts` exports:
   ```ts
   export const DEFAULT_ON_PLUGINS: Plugin[] = [ ... ]
   export const DEFAULT_OFF_PLUGINS: Plugin[] = [ ... ]
   export const ALL_PLUGINS = [...DEFAULT_ON_PLUGINS, ...DEFAULT_OFF_PLUGINS]
   ```
2. `main.tsx` replaces the inline 38-plugin array with `DEFAULT_ON_PLUGINS` + user-enabled entries read from a `settings.json` `plugins.enabled: string[]` field.
3. `pluginsMgmtPlugin` gains an "Available (disabled)" section rendering `DEFAULT_OFF_PLUGINS` with per-row Enable buttons that write to the enabled list. Reload picks up the change.

### Proposed curation

**Default-on (~19):**
6 core services (`configurationService`, `notificationService`, `fileSystemService`, `settings`, `capabilityPrompt`, `themeService`)
+ `workspacePlugin`, `gitStatusPlugin`
+ `activityBarPlugin`, `sidebarPlugin`, `rightPanelPlugin`, `statusBarPlugin`, `launcherPlugin`
+ `filesPlugin`, `editorPlugin`, `outlinePlugin`
+ `commandPalettePlugin`, `confirmPlugin`, `paneModePlugin`
+ `searchPlugin`
+ `pluginsMgmtPlugin`

**Default-off (~14):**
`aiPlugin`, `agentPlugin`, `mcpPlugin`, `workflowPlugin`, `skillsPlugin`, `terminalPlugin`, `processesPlugin`, `graphPlugin`, `graphGlobalPlugin`, `canvasPlugin`, `basesPlugin`, `backlinksPlugin`, `bookmarksPlugin`, `outgoingLinksPlugin`, `filePropertiesPlugin`, `tagsPlugin`, `allPropertiesPlugin`

### Acceptance
- Empty `~/.nexus-shell/` boot shows exactly the default-on set.
- Settings > Plugins shows Installed + Available sections.
- Enable on a default-off plugin + reload surfaces it with contributions intact.
- `grep -c "import.*Plugin" shell/src/plugins/catalog.ts` equals 38 (no plugin lost).

---

## 3. WI-45-lite — Local docs reconcile

### Deliverables (3 of the original 5)
1. **`README.md` de-stale.** Remove the `⚠️ Desktop shell freeze` banner (resolved by Phase 4 WI-37). Remove "Phase 4-5 features functional" language (conflicting phase numbering). Keep it terse and personal-tool-framed. Don't write a public pitch.
2. **Plugin tutorial reconcile.** Two tutorials exist:
   - `docs/writing-your-first-plugin.md` (164 lines, scaffold-driven, Phase 4 WI-39)
   - `shell/docs/writing-a-plugin.md` (~290 lines, word-count example, pre-Phase-3)

   Split: keep the first as the quickstart (scaffold path), rework the second into a reference (activation events, `@nexus/extension-api` imports, sandbox model, capability declarations). Cross-link.
3. **Planning archive move.** `git mv` the planning artifacts into `docs/planning/`:
   `PHASE-1-IMPLEMENTATION-PLAN.md`, `PHASE-2-IMPLEMENTATION-PLAN.md`, `PHASE-3-IMPLEMENTATION-PLAN.md`, `PHASE-4-IMPLEMENTATION-PLAN.md`, `PHASE-5-IMPLEMENTATION-PLAN.md` (this file), `INTEGRATION-REVIEW.md`, `UI-AUDIT.md`, `MICROKERNEL-AUDIT.md`, `SHELL-COMPARISON.md`, `PARITY-CHECKLIST.md`. Add `docs/planning/README.md` explaining the directory. `grep -rn` for references before/after; fix broken links.

   Leave current-architecture docs (`ARCHITECTURE.md`, `leaf-architecture.md`, `legacy-shell-retirement.md`, ADRs) in place.

### Deferred (in REQUIRED-FOR-FORMAL-RELEASE.md)
- Docs landing page (`docs/README.md` audience hub) — nice-to-have, not essential for personal use
- ARCHITECTURE.md re-audit — light edit deferred
- Full README rewrite as product landing

### Acceptance
- `rg -n "Phase [0-9]" README.md shell/README.md` returns nothing outside `docs/planning/`.
- Freeze banner gone from `README.md`.
- Two tutorials have clear, non-overlapping purposes with cross-links.
- No broken links to moved planning docs (`rg -n "PHASE-[0-9]-IMPL|INTEGRATION-REVIEW" docs/ shell/ README.md` — all hits should point at `docs/planning/`).

---

## 4. WI-47 — Local panic log (replaces WI-42)

### Design
- `shell/src-tauri/src/main.rs` and `crates/nexus-cli/src/main.rs` install a `panic::set_hook` early.
- Hook writes panic info (message, location, backtrace via `std::backtrace::Backtrace::force_capture()`, timestamp, binary name) to `~/.nexus-shell/logs/panic.log` (append-mode).
- Rotation: if file exceeds 1 MB, rename to `panic.log.1` (overwrite prior). No third rotation level — two files is enough for a personal tool.
- Hook chains to the default hook so panic output still reaches stderr in dev.
- No network, no opt-in prompt, no user-facing UI.

### Files touched
- `shell/src-tauri/src/main.rs` — install hook in `main()` before Tauri builder.
- `crates/nexus-cli/src/main.rs` — install hook in `main()` before command dispatch.
- Possibly a small shared helper `crates/nexus-panic-log/` if the code is non-trivial; otherwise duplicate ~30 LOC.

### Acceptance
- Trigger a test panic in each binary; `~/.nexus-shell/logs/panic.log` contains the entry with backtrace.
- File rotates at >1 MB (verify by writing synthetic entries in a test).
- Panics still surface to stderr in dev.

---

## 5. Execution order

No dependencies between the three; run in parallel.

1. WI-47 panic log (fastest, ~2h).
2. WI-43 curation (~1d).
3. WI-45-lite docs (~1d).

Total: ~2–3 engineer-days.
