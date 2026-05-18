# Implementation Plan — Audit Findings Remediation

> **As of:** 2026-05-17. Synthesizes every actionable item from the v0.1.2 audit set: [`settings/hardcoded-rust.md`](settings/hardcoded-rust.md), [`settings/hardcoded-shell.md`](settings/hardcoded-shell.md), [`settings/plugin-manifest-defaults.md`](settings/plugin-manifest-defaults.md), [`reference/todos.md`](reference/todos.md), [`reference/audit-flags.md`](reference/audit-flags.md), [`architecture-adherence.md`](architecture-adherence.md).
>
> **None of these items block a 0.1.2 release.** All are hardening, UX polish, or stub completion. Phase ordering reflects risk reduction (security first, infrastructure second, surface polish third).

## Effort scale

| Level | Hours | Suitable for |
|------|------:|-------------|
| **XS** | <1 | one-line / one-file fix |
| **S** | 1–4 | one PR, no design |
| **M** | 1–3 days | multi-PR, may need micro-RFC |
| **L** | 1–2 weeks | needs design doc + review |
| **XL** | 2+ weeks | needs ADR + sequenced subtasks |

## Aggregate

| Phase | Items | Effort estimate | Risk reduction |
|-------|------:|-----------------|----------------|
| 0 — Quick wins (done or trivial) | 8 | ~6 h | low (cosmetic) — **all 8 shipped 2026-05-17** |
| 1 — Security hardening (P0) | 9 | 1–2 weeks | **high** (cap elevation closes hostile-plugin surface) — **8/9 shipped 2026-05-17; only P1-08 (RFC) deferred** |
| 2 — Settings infrastructure (P1) | 7 | 2–3 weeks | medium (user UX + future-proofing) |
| 3 — Architecture hardening (P1) | 5 | 1 week | medium (closes theoretical regression paths) — **all 5 shipped 2026-05-18** |
| 4 — Stub completion (P2) | 6 | 2–4 weeks | low (UX surface) |
| 5 — Constants centralization (P3) | 4 | 1 week | low (maintainability) |
| **Total** | **39 items** | **~7–11 weeks one engineer** | |

---

## Phase 0 — Quick wins

> **Phase 0 completed 2026-05-17.** All 8 items shipped — see per-row links below.

Items already shipped during the audit, or trivial enough to bundle into any nearby PR.

| ID | Item | Status | Effort |
|----|------|--------|--------|
| **AA-05** | Document "host-platform primitive" pattern in `shell.md` | ✅ shipped 2026-05-17 | XS |
| **AA-09** | Patch shell.md + CLAUDE.md 25→29 Tauri command count | ✅ shipped 2026-05-17 | XS |
| **P0-01** | Rationale comment on `nexus-crdt → nexus-editor` dep in Cargo.toml (AA-06) | ✅ shipped 2026-05-17 | XS |
| **P0-02** | Open ADR formalizing the 3 CLI-scope IPC bypasses (AA-03 — `nexus-terminal`/`nexus-collab`/`nexus-security` in CLI) — [ADR 0031](../archive/pre-0.1.2/adr/0031-cli-scope-exceptions-to-ipc-only.md) | ✅ shipped 2026-05-17 | XS |
| **P0-03** | Fix `crates/nexus-audio/src/local_backend.rs:86` — thread `forge_root` through `AudioConfig::local_model_dir` (kills the chdir requirement) | ✅ shipped 2026-05-17 | S |
| **P0-04** | Fix `crates/nexus-audio/src/local_backend.rs:29` — bare AIFF passthrough on macOS `say` (drop the WAV-wrap) — resolved by aligning the stale doc-comment with the actual `--data-format=LEF32@22050` WAV path; no AIFF/hound wrap exists in code. | ✅ shipped 2026-05-17 | S |
| **P0-05** | Fix `crates/nexus-audio/src/provider_backend.rs:316` — route audio reqwest through the `nexus-ai::http_client::build_client` TLS-pinning gate (TODO(BL-102 follow-up)). Shared `build_pinned_client` moved into `nexus-security::tls`; `nexus-ai` re-exports it, `nexus-audio` calls it directly. | ✅ shipped 2026-05-17 | S |
| **P0-06** | Promote `crates/nexus-ai/src/handlers/predict.rs:33-37` `DEFAULT_MAX_TOKENS = 64` to `ai.predict_max_tokens` setting (FIM completions) — field added to `AiConfig`; handler reads it (defaults to 64). | ✅ shipped 2026-05-17 | XS |

---

## Phase 1 — Security hardening (P0)

> **Phase 1 completed 2026-05-17** (less P1-08, which remains open for an RFC). All cap-elevations, the new caps (`security.write`, `security.audit.write`, `network.bind`), the in-tree-only marker, and the `KernelConfig.wasm_caps` ceiling shipped.

These items close real or potential hostile-plugin surface. Highest priority. Most are AUDIT-flagged handlers in `cap_matrix.toml`.

### Cap-elevation walk

Each row promotes a handler from `unrestricted` to a `caps = [...]` requirement. Process per handler:

1. Add cap to `Capability::ALL` (and risk_level in `nexus-security`) if it's new.
2. Edit `cap_matrix.toml` row.
3. Walk every existing caller's `manifest.capabilities.required` — extend the list.
4. Regenerate `docs/generated/capabilities.md` via `scripts/check_ipc_drift.sh`.
5. Run `cargo test -p nexus-bootstrap --test cap_matrix_complete` to verify.

| ID | Handler | New cap | Effort | Notes |
|----|---------|---------|:------:|-------|
| **P1-01** ✅ | `com.nexus.security::set_secret`, `delete_secret`, `clear_audit_log` | new `security.write` (covers first two), new `security.audit.write` (covers third) | M | Shipped 2026-05-17. Added `SecurityWrite` + `SecurityAuditWrite` to `Capability` enum (HIGH risk), updated `risk.rs`, flipped cap_matrix rows. Cap count 30→33. |
| **P1-02** ✅ | `com.nexus.ai::resolve_credentials` | new "in-tree-only" marker (not a cap) | S | Shipped 2026-05-17. `internal = true` field added to cap_matrix rows; new `IpcDispatcher::is_handler_internal_only` trait method; `KernelPluginContext` carries `caller_trust_level` (defaults Community, bootstrap upgrades core-plugin + invoker contexts to Core); dispatcher rejects calls when marker is set and trust != Core. |
| **P1-03** ✅ | `com.nexus.terminal::send_input`, `send_raw_input`, `run_saved`, `adhoc_promote`, `repl_eval` | existing `process.spawn` | S | Shipped 2026-05-17. Pure cap_matrix.toml edits — all 5 rows. |
| **P1-04** ✅ | `com.nexus.git::push`, `push_tags` | existing `net.http` | XS | Shipped 2026-05-17. Pure cap_matrix.toml edits. |
| **P1-05** ✅ | `com.nexus.linkpreview::fetch` | existing `net.http` | XS | Shipped 2026-05-17. Pure cap_matrix.toml edit. |
| **P1-06** ✅ | `com.nexus.agent::delegate`, `plan` | existing `ai.chat` | S | Shipped 2026-05-17. Pure cap_matrix.toml edits. |
| **P1-07** ✅ | `com.nexus.collab::start_relay` | new `network.bind` cap | M | Shipped 2026-05-17 alongside P1-01. HIGH-risk, registered in `risk.rs`. |
| **P1-08** | `com.nexus.workflow::run`, `run_digest` (issue #77 laundering surface) | **needs design** — workflow runs arbitrary handlers; per-step gating is already in place. The question is whether to add a `workflow.run.requires_<cap>` declarative gate. | L | Open RFC required. May not be solvable cleanly without a new "delegated caller" model. **Deferred — not shipped in this Phase 1 pass.** |

### Other P1 items

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P1-09** ✅ | Add `KernelConfig.wasm_caps: { max_memory_mb, max_fuel, max_execution_ms }` system-wide ceiling that `PluginLoader::load` clamps every per-plugin `WasmConfig` against. Today a hostile plugin can self-declare `fuel = 999_999_999_999`. | M | Shipped 2026-05-17. New `nexus_kernel::WasmCapsCeiling` struct + `[wasm_caps]` TOML section; `PluginLoader::set_wasm_caps_ceiling` + `clamp_wasm_cfg` clamp at load with `tracing::warn!` on every clamp. Defaults: 128 MB / 100M fuel / 30 s — large enough that today's plugins are unchanged. 4 unit tests added. |

### Phase 1 deliverables

- 8 new or upgraded cap-requirement rows in `cap_matrix.toml`
- 2 new capabilities (`security.write`, `security.audit.write`, `network.bind`) — total grows from 30 to 33
- 1 new "in-tree-only" marker mechanism
- `KernelConfig.wasm_caps` ceiling
- Updated `docs/generated/capabilities.md`
- Reduced `# AUDIT:` count in `cap_matrix.toml` from 17 → 0 for the items above
- 1 open RFC for workflow laundering surface (P1-08)

---

## Phase 2 — Settings infrastructure (P1)

These items convert manifest-baked defaults into user-tunable settings. Highest UX value because **45 keybindings + 38 priorities currently have no override path**.

### Settings cascade groundwork

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P2-01** ✅ | Keybindings cascade — every `keybindings: [{ key }]` entry in plugin manifests becomes a `keybindings.<command>` settings schema key with the manifest entry as the default. `core.commandPalette`'s existing `bindStorage/setOverride` already provides the runtime override; this connects it to the persistence layer. | L | Shipped 2026-05-17. `keybindingOverrideStorage` now reads from the per-forge configStore (key `nexus.keybindings.overrides`) when hydrated, falling back to localStorage as boot cache. Writes mirror to both. `main.tsx` re-calls `loadOverrides()` on hydration so a forge-only override takes effect on first keypress. No manifest changes required — the override path is per-command-id, so the cascade applies to every existing entry uniformly. |
| **P2-02** ✅ | Priority/ordering override — `activityBar.order: { "<plugin-id>": <int> }` settings cascade for the 38 hardcoded priority values (activity-bar items + overlay stack). | M | Shipped 2026-05-17. New shared `registry/priorityOverrides.ts` helper exposes `resolveEffectivePriority(scope, entryId, declared)` + `subscribePriorityChanges(scope, fn)`. Wired into all four sort registries: `SlotRegistry`, `ActivityBarStore`, `PanelAreaStore`, `StatusBarRegistry`. Setting key shape: `nexus.priority.<scope>.<entryId> = N`. Live re-sort on config change. `originalPriority` field preserves the plugin's declared baseline so overrides recompute against the right source. |
| **P2-03** ✅ | File extension registration override — `<plugin>.fileExtensions: string[]` for the 4 hardcoded extension lists (`.bases/.base`, `.canvas`, markdown/diff viewTypes). | S | Shipped 2026-05-17. `nexus.bases.fileExtensions`, `nexus.canvas.fileExtensions`, `nexus.editor.fileExtensions` — each read via `api.configuration.getValue(key, default)` at plugin-activate. Live-reload on settings change is a Phase 4 follow-up. |

### High-value Rust→settings promotions

From [`settings/hardcoded-rust.md` §User Config](settings/hardcoded-rust.md#user-config):

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P2-04** ✅ | AI provider model strings → `ai.toml`: add `anthropic_model`, `openai_chat_model`, `openai_embedding_model`, `ollama_chat_model`, `ollama_embedding_model`, `ollama_temperature` fields with sensible defaults. | S | Shipped 2026-05-17. Added to both `nexus_formats::AiConfig` (ai.toml schema) and `nexus_ai::AiConfig` (runtime). `build_ai_provider`/`build_embedding_provider` consult `cfg.model.or(cfg.<provider>_model)`. `OllamaProvider::with_fim_temperature` threads the new `ollama_temperature` through `/api/generate`. Per-provider constants made `pub` as ultimate fallbacks (`nexus_ai::{anthropic,openai,ollama}::DEFAULT_*`). `set_config` payload parses the new fields. |
| **P2-05** ✅ | Network endpoints → settings: `ai.ollama_base_url`, `audio.whisper_model_url`, `audio.openai_api_base_url`, `collab.relay_url`, `collab.default_port`, `collab.bind_address`. | S | Shipped 2026-05-17. `nexus_formats::AiConfig.ollama_base_url`; `[audio].whisper_model_url` (template, `DEFAULT_WHISPER_MODEL_URL_TEMPLATE`); `nexus_audio::provider_backend::DEFAULT_BASE_URL` now `pub`; `nexus collab serve --bind <ip>` (`DEFAULT_BIND_ADDRESS = "0.0.0.0"`). `collab.relay_url`/`collab.default_port` were already user-configurable — no code change, just doc accuracy. |
| **P2-06** ✅ | User-facing timeouts → settings: `mcp.{connect,shutdown,oauth}_timeout_secs`, `collab.{handshake,backoff_max}_timeout_secs`, `git.{poll_interval,auto_commit_tick}_secs`, `ui.theme_debounce_ms`, `ai.indexing_debounce_secs`. | M | Shipped 2026-05-17 in two passes. Pass 1: every literal as `pub const DEFAULT_*`. Pass 2 (follow-up): live config-file knobs — `[git] poll_interval_secs` + `auto_commit_tick_secs` (threaded into `run_poller` / `run_auto_committer`), `[ai] indexing_debounce_secs` (threaded into `IndexingDaemon::start_with_debounce`), `[audio] creds_lookup_timeout_secs`, `[collab] backoff_factor` / `handshake_timeout_secs`, `mcp.toml [timeouts]` schema (consts remain operational; per-client thread-through deferred). Theme watcher already takes `debounce_ms` as a constructor arg; production caller not yet wired. |
| **P2-07** ✅ | Notification limits → `notifications.{telegram_max_bytes, inbox_max_rows, inbox_max_age_days}` settings (currently in `nexus-notifications/src/inbox.rs`). | XS | Shipped 2026-05-17. `TelegramChannel.max_bytes: Option<usize>` added (default `DEFAULT_TELEGRAM_MAX_BYTES = 4096`); `[inbox].max_rows`/`max_age_days` were already pluggable — rows struck from `hardcoded-rust.md`, documented in `forge-config.md`. |

### Phase 2 deliverables

- Keybindings cascade pattern documented + 46 plugin manifests migrated
- Priority override mechanism + 38 priorities migrated to defaults
- File-extension override for 4 plugins
- ~30 Rust hardcoded user-config items promoted to settings (settings/hardcoded-rust.md User Config section shrinks)
- Rows in `settings/hardcoded-rust.md` deleted as promoted
- `settings/forge-config.md` updated with new fields

---

## Phase 3 — Architecture hardening (P1)

> **Phase 3 completed 2026-05-18.** All 5 items shipped — see per-row links below.

Closes theoretical regression paths in the invariant enforcement. Items from [`architecture-adherence.md`](architecture-adherence.md) §Recommended Remediation.

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **AA-01 / P3-01** ✅ | Extend `dep_invariants.rs::FORBIDDEN` to forbid `nexus-mcp`/`nexus-acp`/`nexus-remote` from `nexus-terminal`, `nexus-editor`, `nexus-git`, `nexus-database`. None link them today; this encodes the intent. | XS | Shipped 2026-05-18. 12 new `(consumer, forbidden_dep)` rows appended to `crates/nexus-bootstrap/tests/dep_invariants.rs::FORBIDDEN`. Test passes (`cargo test -p nexus-bootstrap --test dep_invariants`). |
| **AA-02 / P3-02** ✅ | Add parallel `dep_invariants` test for `shell/src-tauri/Cargo.toml`. Today the shell crate sits outside `[workspace]` so the existing test can't see it. | S | Shipped 2026-05-18. New sibling `crates/nexus-bootstrap/tests/dep_invariants_shell.rs` reads the shell manifest at `<workspace>/shell/src-tauri/Cargo.toml` via a `CARGO_MANIFEST_DIR`-relative walk and enforces a 25-crate `FORBIDDEN_FOR_SHELL` list (every subsystem engine; `nexus-remote` retained per BL-140). Covers `[dependencies]` and `[target.'cfg(...)'.dependencies]`. |
| **AA-04 / P3-03** ✅ | Invert the `shell/src/shell/App.tsx:8` import of `plugins/nexus/workspace/workspaceStore`. Expose `rootPath` via a shell-owned slot the plugin publishes to. | M | Shipped 2026-05-18. `App.tsx` now reads `rootPath` via `useContextKey('nexus.workspace.rootPath')` — a shell-owned `ContextKeyService` key the workspace plugin already publishes from `setRoot` (and declares in its manifest's `contributes.contextKeys`). Empty-string sentinel normalised to `null` to preserve the file's existing nullable shape. No new slot type required — the existing context-key surface is the right dependency inversion. Stale Phase-1 follow-up: `shell/tests/capability-info.test.ts` updated to enumerate the 33-cap set (was 29). |
| **AA-07 / P3-04** ✅ | Extend `scripts/check_ipc_drift.sh` to cover `nexus-security` and `nexus-collab` (both have IPC types not yet ts-exported). | XS | Shipped 2026-05-18. Security's `ts-export` markers already existed; collab gained an `[features] ts-export` plus `TS + JsonSchema` markers on the 5 IPC types in `core_plugin.rs` (`LocalPeer`, `PublishPresenceArgs`, `PublishPresenceReply`, `StartRelayArgs`, `RelayStatus`) and the 2 bus-payload types in `presence.rs` (`PresenceCursor`, `PresenceEvent`). Both crates added to `nexus-bootstrap/Cargo.toml::ts-export` feature set and two new `cargo test --features ts-export --tests` lines in `scripts/check_ipc_drift.sh`. 7 new TS files emitted to `packages/nexus-extension-api/src/generated/ipc/`. |
| **AA-08 / P3-05** ✅ | **Iframe-sandbox security audit** — focused red-team-style pass on the `shell/src/host/sandbox/` escape paths, pluginId boundary-binding (F-8.1.2), WASM `host_fns.rs` error paths, and the path between `notify_desktop` and `tauri_plugin_notification`. | L | Shipped 2026-05-18. Report at [`audits/sandbox-security-2026-05-18.md`](audits/sandbox-security-2026-05-18.md). Overall verdict: sandbox is structurally sound (pluginId binding verified clean, capability-gating order correct, `notify_desktop` unreachable from null-origin iframe). **2 High findings**: `kernel.on` has no capability gate or topic-prefix sanitization (broad event-bus sniffing); `host::read_file` uses inline canonicalize+prefix-check instead of `ForgePathValidator` (regression risk of F-5.3.1). **5 Medium / 4 Low / 3 Informational** also catalogued. None are exploitable today against an unmodified core plugin set; all are surface-hardening for community plugins. |

### Phase 3 deliverables

- 2 new dep-invariant test surfaces (P3-01 micro-RFC; P3-02 shell test)
- 1 dependency-inversion fix in App.tsx (eliminates the only empty-by-default smell)
- 2 crates added to drift-check coverage
- 1 security audit report (separate doc — would land at `docs/0.1.2/audits/sandbox-security-<date>.md`)

---

## Phase 4 — Stub completion (P2)

> **Phase 4 complete 2026-05-18.** All 10 items shipped. P4-06 SettingsPanelView shows zero "Coming soon" placeholders — every control is either a value-persisting `Wired*` primitive, a wired action button (Tauri invoke / external URL / file dialog), or returns a transparent "not yet built" notification when the underlying subsystem doesn't exist (file-recovery snapshots). P4-07's 12 editor tab actions all dispatch to real backends, backed by three new IPC handlers (`workspace.splitLeaf` primitive, `storage::write_frontmatter` with 6 unit-tested YAML-splice cases, `git::file_log` over the existing `GitEngine::log_file`). Larger UX follow-ups remain available as their own dedicated PRs (3-pane CodeMirror merge view, snapshot subsystem, community theme browser modal), but none are blocking Phase 4 acceptance.

UX surface work — the stubs are honest (every "coming soon" surfaces a toast) but they add up to a lot of user-visible friction. From [`reference/todos.md`](reference/todos.md).

### Whole-plugin stubs

| ID | Plugin | Backend handler to use | Effort |
|----|--------|------------------------|:------:|
| **P4-01** ✅ | `nexus.allProperties` — list every frontmatter property of active note | `com.nexus.storage::read_frontmatter` + entity index | S |
| **P4-02** ✅ | `nexus.tags` — surface active note's tags | `com.nexus.storage::read_frontmatter` (frontmatter `tags`) + `com.nexus.storage::query_tags` for per-tag usage drill-down | S |
| **P4-03** ✅ | `nexus.fileProperties` — show file properties | `com.nexus.storage::read_frontmatter` + `query_files` for index metadata (size, mtime) | S |
| **P4-04** ✅ | `nexus.bookmarks` — list saved bookmarks | KV-backed via the existing per-forge `configStore` (`nexus.bookmarks.entries`); no new Rust handler required. Adds `nexus.bookmarks.toggleActive` command. | M |
| **P4-05** ✅ | `nexus.outgoingLinks` — list outgoing links from current buffer (tab + command stubbed) | `com.nexus.storage::outgoing_links` | S |

### Settings panel + tab actions

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P4-06** ✅ | Wire the **57 "Coming soon" controls** in `SettingsPanelView.tsx` to real backend handlers. Group by tab: General (5), Editor (21), Files & links (14), Keychain (1), Canvas (8), Backlinks (1), Daily notes (3), File recovery (4), Note composer (4). **Now also includes** the P2-01/02/03 cascade-key exposure (keybinding UI already exists via WI-04; priority + fileExtensions need new UI). | XL | **Shipped 2026-05-18.** Added 5 reusable `Wired*` primitives (`WiredToggle`, `WiredSelect`, `WiredNumberRange`, `WiredText`, `WiredNumber`) plus `WiredAccentColor` and `CustomAppIconChooser` that read/write per-forge `configStore` under `nexus.settings.*` keys (round-tripping through `<forge>/.forge/app.toml [settings]`). Every "Coming soon" placeholder has been resolved: 78 stub controls migrated to value-persisting Wired primitives across 12 tabs; 12 action buttons wired (Help & Sync sign-up/log-in → external URLs; Rebuild forge cache → `storage::rebuild_index`; Add secret → `security::set_secret`; Custom app icon → `openDialog`; Accent color/fonts/excluded-globs/ribbon-commands → configStore inputs; Manage themes → community-themes URL); View/Clear file-recovery snapshots return clear "not yet built" notifications instead of empty toasts; the cosmetic Spellcheck ⚙ gear button was removed because the toggle + languages select cover its function. `useComingSoon` helper deleted — file has zero remaining "Coming soon" handlers. |
| **P4-07** ✅ | Wire the **12 editor tab-action stubs** (`nexus.editor.stub.*`): splitRight, splitDown, openInNewWindow, openLinkedView, rename, moveTo, bookmark, addProperty, backlinksInDocument, versionHistory, mergeFile, exportPdf. Each is a discrete handler. | L | **Shipped 2026-05-18.** All 12 wired: `bookmark` (→ `nexus.bookmarks.toggleActive`), `rename`/`moveTo` (→ `storage::rename_entry`), `backlinksInDocument` (→ `nexus.backlinks.focus`), `openInNewWindow` (→ `popoutWindowBridge.popoutLeaf`), `openLinkedView` (→ `storage::backlinks` + `api.input.pick`), `versionHistory` (new `com.nexus.git::file_log` IPC + commit picker copying hash to clipboard), `addProperty` (new `com.nexus.storage::write_frontmatter` IPC with unit-tested YAML splice), `splitRight`/`splitDown` (new `workspace.splitLeaf(leafId, direction)` primitive), `exportPdf` (preview-mode + `window.print()` — OS Save-as-PDF affordance), `mergeFile` (`git::conflict_files` + `api.input.pick` to route the user to the conflicted file — editor renders `<<<<<<<` markers inline for manual resolution; a dedicated 3-pane CodeMirror merge view is the larger follow-up but the v1 surface is honest and functional). |

### Other stubs

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P4-08** ✅ | CLI `nexus sync` subcommand — implement against storage / git plugins (mirror clone/pull behaviour for forge-to-forge sync). | M | Shipped 2026-05-18 as a thin convenience over `nexus git fetch|pull|push` with `--remote`/`--branch`/`--no-push` flags; no separate `commands::sync` module needed. `StubArgs` enum variant gone; the `stubs` module deleted. |
| **P4-09** ✅ | CLI `nexus run` subcommand — implement as a thin wrapper over `com.nexus.workflow::run` or `com.nexus.skills::invoke`. | S | Shipped 2026-05-18. `Run { name }` dispatches to `commands::workflow::run(&mut app, &name)` — an alias for `nexus workflow run <name>`. |
| **P4-10** ✅ | `PluginAPI.ts::defineSlot` SDK surface — implement dynamic slot definition. Today it warns instead of doing anything (`PluginAPI.ts:641`). | M | Shipped 2026-05-18. `SlotStore.slots` widened from `Record<SlotId, …>` to `Record<string, …>` so dynamic ids don't need to extend the static `SlotId` union; new `slotRegistry.defineSlot(id)` is idempotent and seeds an empty array. `register()` auto-creates the slot if absent. PluginAPI's `defineSlot` now calls through instead of warning. |

### Phase 4 deliverables

- 5 whole-plugin stubs filled in (5 plugins migrate from "Not yet implemented" to functional)
- 9 settings tabs wired through (`SettingsPanelView.tsx` "Coming soon" labels go to zero)
- 12 editor tab-action stubs implemented or removed if outscoped
- 2 CLI stub subcommands implemented
- `defineSlot` SDK surface live
- `reference/todos.md` shrinks dramatically

---

## Phase 5 — Constants centralization (P3)

Maintainability — collapses duplicate timeouts / endpoint strings / plugin id literals into shared constants.

| ID | Item | Effort | Notes |
|----|------|:------:|-------|
| **P5-01** | Create `nexus-constants` crate (or `nexus-types::constants` module) with shared `Duration` constants. Collapse the ~30 per-CLI-subcommand `Duration::from_secs(30/60/120)` literals into shared `IPC_TIMEOUT_SHORT_SECS`, `IPC_TIMEOUT_NORMAL_SECS`, `IPC_TIMEOUT_LONG_SECS`. | M | Touches every `crates/nexus-cli/src/commands/*.rs`. |
| **P5-02** | Add `nexus-types::plugin_ids` module exposing every `com.nexus.<id>` as a `pub const &str`. Replace the literal-strings in `nexus-mcp/src/server.rs:29-41`, `nexus-notifications/src/core_plugin.rs:57`, `nexus-crdt/src/wire.rs:37`, and shell-side `shell/src/types/plugin.ts`. | S | Cross-language consistency — could emit the TS version via ts-rs. |
| **P5-03** | Unify shell + Rust AI defaults — the model string / max_tokens / temperature appear in both `nexus-formats/src/config/ai.rs:35-39` and `shell/src/plugins/nexus/ai/`. Pick one source of truth (the Rust side via ts-rs export). | S | Stop hand-syncing. |
| **P5-04** | Promote audit log retention (`90` days hardcoded) and `commandPalette.maxResultsLimit` (`50` in two places) to top-level constants then to settings. | XS | Trivial. |

### Phase 5 deliverables

- 1 new shared-constants module
- ~30 duplicate timeout literals collapsed
- Plugin-id constants module bridging Rust + TS
- AI defaults unified
- Final pass of `settings/hardcoded-rust.md` Dev Config section — shrinks ~40%

---

## Cross-cutting infrastructure changes (not phase-specific)

These touch the kernel/contract layer and should be designed once and used by multiple phases.

### CI-01 — Capability enum evolution

Adding new capabilities (Phase 1) requires:
1. New `Capability` variant in `crates/nexus-kernel/src/capability.rs`.
2. `risk_level()` mapping in `crates/nexus-security/src/risk.rs`.
3. Regeneration of `docs/generated/capabilities.md` (covered by drift check).
4. ts-rs export to `packages/nexus-extension-api/src/generated/ipc/Capability.ts`.
5. Optionally a CHANGELOG / migration note for existing plugins whose manifests should opt-in.

### CI-02 — Settings cascade pattern

Phase 2 introduces two cascades (keybindings + activity-bar order). Pick a uniform shape:

```
1. plugin manifest declares default
2. shell schema declares schema key + caption
3. <forge>/.forge/app.toml [keybindings] / [activityBar] table holds overrides
4. core.commandPalette / core.activityBar registry merges (override > default)
```

A single ADR ("Plugin-declared settings cascade") would standardize this so future cascades (panel widths, slot priorities, etc.) reuse the pattern.

### CI-03 — "In-tree-only" handler marker (P1-02 dependency)

`com.nexus.ai::resolve_credentials` (and possibly others) should be reachable only by core plugins, not community plugins. Today there's no enforcement; the kernel checks the manifest's `trust_level` only at load time.

Suggested shape: add `internal = true` flag to `cap_matrix.toml` row. The kernel dispatcher rejects calls when `caller_manifest.trust_level != Core`.

### CI-04 — Drift check expansion

Phase 3 adds `nexus-security` + `nexus-collab` to `scripts/check_ipc_drift.sh`. Long-term, the script could autodiscover any crate with `[features] ts-export = [...]` rather than the current hardcoded list.

---

## Suggested release sequencing

- **0.1.3 (security release, ~1 week)** — Phase 0 + Phase 1 P1-01 through P1-05. Closes the highest-severity AUDIT flags. New capabilities + cap_matrix updates.
- **0.1.4 (architecture hardening, ~1 week)** — Phase 1 P1-06, P1-07, P1-09 + Phase 3. Adds `KernelConfig.wasm_caps`, extends dep_invariants, ships drift-check expansion, ships the sandbox security audit.
- **0.1.5 (settings + UX, ~3 weeks)** — Phase 2 + Phase 5. Settings cascade lands; keybindings + priorities become tunable; constants centralized.
- **0.1.6 (stub completion, ~3 weeks)** — Phase 4. Settings panel fully wired; whole-plugin stubs implemented; CLI sync + run shipped.
- **Defer (no release dependency)** — Phase 1 P1-08 (workflow laundering RFC) — needs design before code.

Total: **~8 weeks** from start of 0.1.3 to end of 0.1.6 for one engineer, or **~4 weeks** with two-engineer parallelism (Phase 2 settings + Phase 4 stubs are largely independent).

---

## Acceptance per phase

Each phase is "done" when:
- **Phase 1:** zero `# AUDIT:` flags remaining in `cap_matrix.toml` for items P1-01..P1-07; `docs/generated/capabilities.md` shows ≥33 caps; `KernelConfig.wasm_caps` defaulted.
- **Phase 2:** `settings/hardcoded-rust.md` User Config rows reduced by ≥30; `settings/plugin-manifest-defaults.md` priorities/keybindings tables show "✅ tunable via settings cascade" for every row.
- **Phase 3:** `dep_invariants.rs::FORBIDDEN` includes the 12 new pairs; shell-side dep test added; `App.tsx` no longer imports from `plugins/nexus/workspace/`; sandbox security audit report committed.
- **Phase 4:** `reference/todos.md` §4 (settings panel) shows zero "Coming soon" entries; §5 (plugin-internal) reduced to ≤3 deliberate stubs; §6 (whole-plugin stubs) shows zero unfulfilled stubs; CLI stubs.rs has zero callers.
- **Phase 5:** `settings/hardcoded-rust.md` Dev Config rows reduced by ≥40%; `nexus-types::plugin_ids` module exists and is consumed by ≥5 crates.

## Tracking

This plan is the canonical roadmap for audit-driven work. As items ship:
1. Mark the row ✅ in this file.
2. Cross out the corresponding row in the source audit doc (e.g., `~~| nexus-ai/src/ollama.rs | 15 | http://localhost:11434 | ai.ollama_base_url |~~`).
3. Reference the implementing commit/PR.

When a phase completes, add a `> **Phase N completed in 0.1.X (commit abc1234)**` note at the top of its section so the audit trail survives.

When all phases complete and the audit docs are mostly struck-through, file the next-version audit pass (`docs/0.1.7/` or whatever's current).

---

## Out of scope

These show up in the audit set but are deliberately not on this plan:

- **CSS variable defaults** (~547 theme tokens) — visual design surface, not a settings problem. Owned by themes work.
- **i18n / translation strings** — no i18n layer at v0.1.2. Would warrant its own track.
- **Bundled skill / template content** — content, not settings.
- **Auto-updater + Sentry** (legacy PRD-17 backlog) — deferred per personal-tool scope.
- **Marketplace + signed plugin distribution** — would land alongside auto-updater.
- **Cloud sync** — file-as-truth means git/syncthing/dropbox are the answer. No first-party sync.

If any of these become priorities, file a separate plan; don't tack them onto this one.
