# Nexus — Gap & Inconsistency Review

> **As of:** 2026-05-21. Scope: full workspace (35 crates, shell, packages, docs). Findings grouped by severity. Each item is file:line-grounded so you can jump straight to the work. The existing [`architecture-adherence.md`](../architecture-adherence.md) (2026-05-17) already covers the four microkernel invariants; this review covers what that audit explicitly didn't.
>
> **Status legend:** items closed during the 2026-05-21 sweep are marked **✅ Closed** with the commit SHA. Open items are unmarked.

## Closed during this review

| Item | Title | Commit |
|------|-------|--------|
| A2 | Silent error swallowing across event bus (audit plugin) | `0eb8bcc0` |
| A4 | Unbounded channels in LSP/DAP/ACP protocol clients | `22aa9f88` |
| A3 | Silent `unwrap_or_default()` on serialization / deserialization / lock-poisoning | `88f990cd` |
| A2 | Silent error swallowing — storage/notifications/mcp/bootstrap remaining sites | `47f582a` |
| D3 | Handlers don't log on error return (chokepoint + 5 audit-flagged files enriched) | `a186308` |
| A1 | Shell plugin catalog ↔ on-disk mismatch (incidentally closed; regression guard added) | `ef1a163` |
| A6 | Direct Tauri `invoke()` from non-host code (partial drain — 3 of 10 closed) | `a0da5b0` |
| B1 | `ipc-handlers.md` counts stale (drift script added) | `53638d3` |
| B2 | `audit-flags.md` largely stale (drift script added) | `53638d3` |
| B4 | `settings/README.md` forge layout missing ≥9 paths | `53638d3` |
| D1 | tokio::spawn orphans in `nexus-remote` server + `nexus-workflow` webhook | `4e989c6` |
| D2 | `Mutex::lock().expect()` poisoning panics in `nexus-collab` relay | `4e989c6` |
| D4 | 5 un-flagged tunable constants + workflow digests `IPC_TIMEOUT` shared-const migration | _this commit_ |

Drive-by in `0eb8bcc0`: re-exported `in_flight_sync_dispatches` from `nexus-kernel` (function was added in `64237761` for "future metrics surfaces" but unreachable outside the crate; rustc was flagging it dead-code).

---

## A. High-priority gaps

### A1. Shell plugin catalog ↔ on-disk mismatch — ✅ Closed
The plugin catalog in `shell/src/plugins/catalog.ts` declared itself the source of truth, but disk reality diverged on 2026-05-21:

| # | Catalog said | Disk said | Status |
|---|---|---|---|
| 1 | `nexus.activity` (no dir) | — | ✅ catalog now imports `./nexus/activityTimeline` with `legacyPluginIds: ['nexus.activityTimeline']`. |
| 2 | `nexus.osObservability` → `./observability/` | folder is `observability/` | ✅ catalog now points at `./nexus/observability`. |
| 3 | `nexus.notionImport` → `./notion/` | folder is `notion/` | ✅ catalog now imports `./nexus/notion` with `legacyPluginIds: ['nexus.notion']`. |
| 4 | (not listed) | `shell/src/plugins/nexus/activityTimeline/` | ✅ wired in as the `nexus.activity` entry's module. |
| 5 | (not listed) | `shell/src/plugins/nexus/notion/` | ✅ wired in as the `nexus.notionImport` entry's module. |
| 6 | (not listed) | `shell/src/plugins/nexus/observability/` | ✅ wired in as the `nexus.osObservability` entry's module. |
| 7 | `graph.global` | not found | ✅ catalog now imports `./nexus/graph/globalIndex`, which exists. |

The six "only stub content" plugins (`bookmarks`, `debugger`, `fileProperties`, `healthPanel`, `searchPanel`, `statusBar`) all now have substantial implementations (71–389 lines).

These were closed incidentally by refactoring during the intervening weeks (`legacyPluginIds` aliases, renamed imports). A regression guard was added in `shell/tests/catalog-disk-consistency.test.ts` to prevent the catalog and disk from silently drifting back: it fails the test suite if any catalog `import()` is phantom or if any `shell/src/plugins/nexus/<dir>/` is orphaned (with the `_lib/` shared-utility namespace excluded).

### A2. Silent error swallowing across event bus — ✅ Closed (`0eb8bcc0` + `47f582a`)
`let _ = bus.publish_plugin(...)` is the dominant pattern in plugin code. This violates the principle "never silently swallow exceptions". Worst sites:
- ✅ `crates/nexus-security/src/core_plugin.rs:121,154,166` — **audit events** (fixed in `0eb8bcc0`)
- ✅ `crates/nexus-storage/src/core_plugin.rs:960,972,993,998,1009,1018` — 6 state-change events; now routed through `publish_storage_state_event` helper that `warn!`s on failure (`47f582a`).
- ✅ `crates/nexus-notifications/src/core_plugin.rs:556,679,947` — inbox-appended + ai-runtime republish now warn; the notify-watcher `tx.send` is latched-warn (one log per disconnect episode) so callback storms don't spam (`47f582a`).
- ✅ `crates/nexus-mcp/src/core_plugin.rs:312` — `mcp.host.started` lifecycle event now warns on publish failure (`47f582a`).
- ✅ `crates/nexus-bootstrap/src/crdt_publisher.rs:293` — tmp-file cleanup-after-rename-failure now logs the leak instead of swallowing it (`47f582a`).
- ✅ `crates/nexus-bootstrap/src/plugins/mod.rs:253` — `plugin_lifecycle_timeout` telemetry event publish failure now warns; this is the only out-of-band signal that the kernel degraded its plugin set (`47f582a`).

### A3. Data-loss `unwrap_or_default()` on serialization — ✅ Closed (`88f990cd`)
- ✅ `crates/nexus-storage/src/bases/mod.rs:71,85` — now propagate `StorageError::CorruptFile`.
- ✅ `crates/nexus-notifications/src/inbox.rs:558` — now logs warn with entry id + raw json + parse error before defaulting.
- ✅ `crates/nexus-notifications/src/core_plugin.rs:406,425` — test-only constructors now `.expect()` instead of papering over.
- ✅ `crates/nexus-workflow/src/digests.rs:178` — documented soft-fail contract preserved; now logs warn.
- ✅ `crates/nexus-workflow/src/core_plugin.rs:517,538` — now recovers via `into_inner()` and logs (with per-loop latch to avoid spam).

### A4. Unbounded channels in protocol clients — ✅ Closed (`22aa9f88`)
Three external-process clients held unbounded mpsc channels — a chatty LSP/DAP server could OOM the process. Same class of bug as the storage watcher overflow (commit `0bd9eabe`).
- ✅ `crates/nexus-lsp/src/client.rs:226` (LSP `notif_tx`) — bounded at 1024 with `try_send` + latched warn.
- ✅ `crates/nexus-dap/src/client.rs:253` (DAP `event_tx`) — same.
- ✅ `crates/nexus-acp/src/client.rs:198` (ACP `notif_tx`) — same.

`try_send` over `send().await` chosen deliberately: the reader task also delivers responses through the pending-request map, so an awaited stall would wedge every outstanding request on the client.

### A5. Workflow `run`/`run_digest` capability laundering (issue #77 still open)
`cap_matrix.toml:1625,1631` — both unrestricted. A caller with no caps can compose a workflow that internally chains capability-gated handlers; each step is checked, but the *aggregation* of side effects is not. Track A audit flag still live.

### A6. Direct Tauri `invoke()` from non-host code — ⚠️ partially drained
Per ADR 0011, only host code may call the Tauri bridge directly; plugins must route through `PluginAPI.kernel`. The audit listed ten bypass sites; the project's policy is the per-file allowlist in `shell/tests/plugin-import-hygiene.test.ts`, which fails the test suite on any *new* direct `@tauri-apps/*` import outside that list. Each allowlist entry carries a comment explaining why it stays.

Status of the original ten sites:
- ✅ `shell/src/plugins/nexus/ai/marginApi.ts` + `marginSuggest.ts` — both files no longer import from `@tauri-apps/*`; all calls route through `api.kernel.invoke`. (Closed before this audit was reviewed.)
- ✅ `shell/src/plugins/core/settings/SettingsPanelView.tsx` — the three `invoke('kernel_invoke', …)` calls were migrated to `api.kernel.invoke(...)` (`api` was already in scope at all three sites via `props.api` / `FilesLinksTab` / `KeychainTab`). The file stays in the allowlist only because of the unrelated `openDialog` import from `@tauri-apps/plugin-dialog` (theme-file picker — drains when `PlatformDialog.open()` is added).
- ⚠️ `shell/src/plugins/core/capabilityPrompt/requestConsent.ts` — `get_plugin_granted_capabilities` / `set_plugin_granted_capabilities`. Pre-kernel-boot consent storage; legitimately shell-internal (consent must be resolved before the kernel can be booted with the granted-cap set).
- ⚠️ `shell/src/plugins/nexus/workspace/index.ts` — `boot_kernel` / `init_forge` / `shutdown_kernel` / `boot_remote`. Shell-lifecycle ops with no kernel equivalent.
- ⚠️ `shell/src/plugins/nexus/workspace/useConnectionState.ts` — `kernel_connection_state` + `listen('kernel:connection-state')`. State lives in the bridge layer; a kernel IPC wrapper would just forward to the same Tauri command.
- ⚠️ `shell/src/plugins/nexus/launcher/launcherState.ts` — `get/write/forget_shell_state` for the recents list. Shell-managed metadata; no kernel surface.
- ⚠️ `shell/src/plugins/nexus/pluginsMgmt/index.ts` — `set_plugin_enabled`. Shell-managed plugin enable/disable; pre-load.
- ⚠️ `shell/src/plugins/nexus/notifications/index.ts` — `notify_desktop` for OS-level notifications. Could drain when `api.platform.notifications` or equivalent surface is added.
- ⚠️ `shell/src/plugins/nexus/debugger/LaunchConfig.tsx` — `scan_plugin_directory` + `readTextFile` for resolving launch config schemas. Could drain when `api.plugins.dir` is added.

Two of ten sites closed outright; the remaining seven are documented exceptions enforced by `plugin-import-hygiene.test.ts`. Further drain blocked on adding API surface to `@nexus/extension-api` (PlatformDialog, PlatformNotifications, PluginsDir) — separate design work tracked under WI-25.

---

## B. Documentation drift

### B1. `docs/0.1.2/ipc-handlers.md` is stale by 52 handlers — ✅ Closed
Doc claimed "~280 handlers"; matrix has **332**. Per-plugin counts diverged on `storage` (+12), `ai` (−2), `terminal` (−4), `agent` (+1), `mcp.host` (+1), `acp` (+1). Doc now reflects the live matrix and `scripts/check_ipc_docs_drift.sh` fails the build on regression (the counts table and every `## com.nexus.<plugin> (N)` section header are both checked). The drift script is wired into `scripts/check_ipc_drift.sh` so CI runs it on every PR.

### B2. `docs/0.1.2/reference/audit-flags.md` largely stale — ✅ Closed
Of 17 candidate rows in the 2026-05-21 doc, only 4 still reflect uncapped handlers in the matrix today (`workflow::run`, `workflow::run_digest`, `ai::resolve_credentials`, `mcp.host::call_tool`). The other 14 are already cap-gated — moved to a new "Closed since the 2026-05-21 audit" section in the same doc so promotion history stays visible. `mcp.host::call_tool` (newly present with an `# AUDIT:` comment) added to the live table. `scripts/check_ipc_docs_drift.sh` enforces: (a) every matrix handler whose row carries `# AUDIT:` or `internal = true` AND `unrestricted = …` appears in the live table; (b) no row in the live table is missing from the matrix's audit set; (c) no row in the closed-since table is still unrestricted in the matrix.

### B3. `docs/0.1.2/settings/hardcoded-rust.md` Dev-Config table is stale
~15 rows describe constants that are already promoted to named `const`s in code (the "delete the row on promotion" workflow wasn't applied to refactor-driven promotions). Examples:
- `nexus-mcp/core_plugin.rs:551` → moved to line 570, already named `MAX_TOOL_RESPONSE_BYTES`.
- `nexus-editor/core_plugin.rs:2220` → moved to `handlers/transaction.rs:25`, already named.
- `nexus-collab/{client,server}.rs` MAX_FRAME_BYTES / BROADCAST_CAPACITY — already named.
- `nexus-ai/{vectorstore,rag,enrichment,indexing_daemon}.rs` — all already `pub const`.
- `nexus-tui/app.rs:1820,1825` — already `AGENT_IPC_TIMEOUT` / `MODAL_AUTO_REJECT_TIMEOUT`.

The genuinely-still-inline items (4 `for _ in 0..N` loops, the three `term.rs` durations, the `bge` model id) remain valid.

### B4. `docs/0.1.2/settings/README.md` forge layout missing ≥9 paths — ✅ Closed
README now lists the nine missing paths flagged by the audit (`.forge/comments/`, `.forge/templates/`, `.forge/agents/`, `.forge/digests/last_fired.json`, `.forge/skills/`, `.forge/ai-activity.log`, `.forge/.audio/models/`, `.forge/.editor/undo/{sha}.json`, `.forge/.gitignore` + `.gitattributes`) with the owning crate and file noted on each row.

The `.forge/acp.toml` row was struck through with an explicit "intentionally absent — adapters arrive via `com.nexus.acp::register_server` (ADR 0027 §Phase 4)" annotation. The audit's flag about two `config.toml` files was addressed by adding an explicit note in the persistent-config-files table: kernel loads `<forge>/.nexus/config.toml` (`KernelConfig`), audio loads `<forge>/.forge/config.toml` (`AudioConfig`); distinct directories, distinct schemas, intentionally separate.

---

## C. Architectural smells

### C1. Capability vocabulary has 7 singletons + asymmetric pairs
Singletons (used by exactly one handler): `ai.config.write`, `ai.activity.write`, `ai.runtime.submit`, `audio.record`, `audio.synthesize`, `network.bind`, `security.audit.write`. Either too narrow (collapse `audio.record`/`audio.synthesize` → `audio.use`) or too late-bound (e.g. `security.audit.write` exists only for `clear_audit_log`). Worth a vocabulary-design pass.

Read/write asymmetry: `ai.session.read`/`ai.session.write` and `notifications.inbox.read`/`write` are paired, but `ai.config.write`, `ai.activity.write`, `security.audit.write` have no `.read` peer (reads are unrestricted). Inconsistent shape; either consistently pair or document why.

### C2. Storage plugin handler surface (72) is the largest in the system
`com.nexus.storage` is at 72 handlers vs the next-largest `nexus.git` at 38. Bases (15 verbs) + vector store (4) + entity graph (5) account for most growth. Consider splitting Bases into its own service plugin.

### C3. AA-04 still open (shell→plugin dependency inversion)
`shell/src/shell/App.tsx:8` still imports from `plugins/nexus/workspace/workspaceStore`. No fix landed.

### C4. `nexus-workflow` and `nexus-skills` don't follow the `thiserror` convention
Every other service crate has `src/error.rs` with `#[derive(thiserror::Error)]`. These two use ad-hoc error types. Out-of-band.

### C5. Three deferred subsystems explicitly noted in code
- **Workflow webhook trigger engine** — `nexus-workflow/src/core_plugin.rs:45` ("webhook is not yet wired")
- **Workflow run-history indexing** — `core_plugin.rs:88` (BL-054 Phase 4 follow-up; schema exists but events don't populate)
- **Workflow AI-step async executor** — `handlers/run.rs:658` (BL-134 Phase 3)

### C6. Plugin activation: 100% `onStartup`
All 63 catalog entries use `activationEvents: ['onStartup']`. No lazy activation patterns (`onCommand:*`, `onView:*`, etc.). Acceptable for current shell size; precludes startup-time optimization later.

---

## D. Robustness gaps

### D1. `tokio::spawn` orphans (no JoinSet tracking) — ✅ Closed
- ✅ `nexus-remote/src/server.rs` — the three per-request `dispatch_request` spawns (`ipc_call`, `event_subscribe`, `event_unsubscribe`) plus the long-lived subscription forwarder (already tracked via `subscriptions` map). The serve loop now owns a `tokio::task::JoinSet` scoped to the connection; `dispatch_request` spawns through it, the loop drains completed tasks each iteration, and shutdown awaits in-flight handlers for up to 2s before letting JoinSet's `Drop` abort the rest.
- ✅ `nexus-workflow/src/core_plugin.rs` — the `webhook_accept_loop` per-connection spawn now feeds a JoinSet scoped to the accept loop. When the loop's owning `JoinHandle` (in `scheduler_handles`) is aborted on plugin `Drop`, the loop future drops, dropping the JoinSet and aborting every still-pending connection handler.
- N/A `nexus-kernel/src/context_impl.rs:960`, `nexus-ai-runtime/src/core_plugin.rs:863`, `nexus-ai-runtime/src/scheduler.rs:503` — all three sites are inside `#[test]` / `#[tokio::test]` blocks (the audit conflated test-only spawns with production orphans). Test-local spawns die with the test runtime; no fix needed.

### D2. `Mutex::lock().expect()` in long-lived plugins — ✅ Closed
- ✅ `nexus-collab/src/core_plugin.rs` (4 sites in the relay plugin) — all four now route through a `lock_relay()` helper that recovers via `into_inner()` on poison and emits a `tracing::warn!` so the poisoning is observable. The inner data is just `Option<RunningRelay>` (presence/absence; no cross-field invariants), so continuing on the recovered value is safe.
- N/A `nexus-terminal/src/core_plugin.rs:3008,3067` — both sites are inside `mod tests { ... }`, where panicking on poison is the correct test-failure behaviour. No production code change needed.

### D3. Handlers don't log on error return — ✅ Closed (`a186308`)
Spot-checked `crates/nexus-storage/src/handlers/{files,index,notes}.rs` and `nexus-ai/src/handlers/{ask,search}.rs`: **zero** `tracing::error/warn` calls. Errors are `?`-propagated; only the dispatcher logs them, losing handler-specific context.

Fixed by a two-layer change in `a186308`:
1. **Chokepoint**: added a single `tracing::warn!` to `nexus_plugins::dispatch::exec_err`. Every service crate's `define_dispatch_helpers!`-generated `exec_err` delegates to this — one log line per handler error, workspace-wide (22 crates), carrying `plugin_id` and the reason string (which already includes the command name).
2. **Reason-string enrichment** in the five flagged files so the central log carries handler-specific context:
   - ✅ storage/handlers/files.rs — path on all sites, byte count on writes.
   - ✅ storage/handlers/index.rs — path on `obsidian_base_query`; source path on `import_forge` plan/apply.
   - ✅ storage/handlers/notes.rs — path on all `note_append` + `write_frontmatter` sites; key on frontmatter writes.
   - ✅ ai/handlers/ask.rs — question length + limit on rag failure (length only — prompt may be sensitive).
   - ✅ ai/handlers/search.rs — query length + limit + oversample on `semantic_search` and `entity_recall`.

Privacy boundary: forge-relative paths and numeric args are logged; free-text user input (question, query) is logged by length only.

### D4. 5 un-flagged constants that look user-tunable — ✅ Closed
All five constants are now listed in `docs/0.1.2/settings/hardcoded-rust.md` under "Already-named constants worth promoting" (rows 7–11), each annotated with the suggested settings key and rationale. The summary count was bumped from 6 → 11.

The drive-by `nexus-workflow/src/digests.rs:49` `IPC_TIMEOUT = Duration::from_secs(120)` was also migrated to `nexus_types::constants::IPC_TIMEOUT_LONG` per the P5-01 standardisation — same value (120s), same call sites, just routed through the shared constant. The corresponding row in `hardcoded-rust.md`'s IPC-timeouts table was struck through with the migration note.

No `settings.toml` schema work in this PR — promoting a row from "named but hardcoded" to "settings-backed" requires per-subsystem `Config` struct extension + serde defaults + docs, which the hardcoded-rust.md file explicitly tracks as separate downstream work.

### D5. Theming — 87 inline `style={{` files with hardcoded hex
Worst offenders: `diagnostics/DiagnosticsPanelView.tsx` (16 hex codes), `dreamCycle/DreamCycleInboxView.tsx` (12), `templates/TemplatesView.tsx` (11). Most are defensive `var(--token, #fallback)` fallbacks (acceptable), but the absence of a centralized palette means `#3b82f6` accent, `#888` muted, `#2a2a2a` border, `#ef4444` error are duplicated across 5+ plugins.

---

## E. Code-level TODOs worth promoting to issues

| Location | What |
|---|---|
| `packages/nexus-extension-api/src/sandbox/context.ts:41` | `configuration` API deferred — sandboxed plugins have no config bridge |
| `shell/src/host/ExtensionHost.ts:124` | BL-XXX Phase 3.2: kernel-tier `dependsOn` not yet distinguished from shell-tier |
| `shell/src/plugins/nexus/search/index.ts:55` | Ctrl/Cmd+Shift+F → `nexus.searchPanel` not wired |
| `shell/src/registry/SnippetRegistry.ts:11` | Snippet expansion not triggered (collision detection live) |
| `shell/tests/plugin-import-hygiene.test.ts:40-50` | 6 core views allowlisted to bypass plugin API (WI-25 open) |
| `crates/nexus-terminal/src/server.rs:462` | Session restart doesn't preserve pre-command state |
| `crates/nexus-plugins/src/contributions.rs:7-11` | Legacy flat-TOML migration window still open |
| `crates/nexus-remote/src/uri.rs` | Phase 2b client tests only — SSH spawn logic gated |

---

## Recommended punchlist (priority order)

1. ~~**A2 / A3 / D3** — sweep `let _ = .publish*` and `.unwrap_or_default()` on serialize/deserialize; add `tracing::warn!` everywhere data loss is currently silent.~~ ✅ All three closed (A2: `0eb8bcc0` + `47f582a`; A3: `88f990cd`; D3: `a186308`).
2. ~~**A4** — bound the three protocol-client channels; same OOM class as the watcher fix.~~ ✅ Closed (`22aa9f88`).
3. ~~**A1** — reconcile catalog.ts with disk; either delete orphans or wire them up.~~ ✅ Closed (incidentally fixed by `legacyPluginIds` aliases + renamed imports during the intervening weeks; regression guard added in `shell/tests/catalog-disk-consistency.test.ts`).
4. ⚠️ **A6** — sweep direct `invoke()` calls out of plugin code; route through PluginAPI. Partially drained — `marginApi.ts` + `marginSuggest.ts` cleaned before this audit, `SettingsPanelView.tsx`'s three `kernel_invoke` calls migrated to `api.kernel.invoke` in this PR. Seven sites remain as documented shell-internal exceptions in `shell/tests/plugin-import-hygiene.test.ts`; further drain needs new API surface (PlatformDialog.open, PlatformNotifications, PluginsDir).
5. ⚠️ ~~**B1 / B2 / B3 / B4** — refresh the four stale documents; add a `scripts/check_ipc_docs_drift.sh` to prevent regression.~~ B1, B2, B4 closed in this PR; drift script added and wired into `scripts/check_ipc_drift.sh`. B3 (`hardcoded-rust.md`) still open — needs row-by-row code verification (~15 entries).
6. **A5** — issue #77; per-step caps aren't enough — design an aggregation rule for `workflow::run`.
7. ~~**D1 / D2** — wrap orphan spawns in `JoinSet`s; handle `Mutex` poisoning instead of `.expect()`.~~ ✅ Closed in this PR (`nexus-remote` + `nexus-workflow` JoinSets; `nexus-collab` `lock_relay()` helper with poison recovery + tracing).
8. **C1** — capability-vocabulary cleanup pass (singletons, read/write symmetry).

None of A–D are release-blocking; A2/A3/D3 are the most direct correctness/observability wins.

---

## Changelog

- **2026-05-21** — initial audit; A2 (audit plugin sites only), A3, and A4 closed in the same session. See `git log --grep "fix(security)\|fix(lsp,dap,acp)\|fix(storage,notifications,workflow)"`.
- **2026-05-22** — A2 remaining sites closed in `47f582a` (storage 6, notifications 3, mcp 1, bootstrap 2). A2 is now fully closed.
- **2026-05-22** — D3 closed in `a186308` via a chokepoint `warn!` in `nexus_plugins::dispatch::exec_err` plus reason-string enrichment in the five audit-flagged files.
- **2026-05-22** — A1 marked closed: every mismatch in the audit table was reconciled by intervening refactoring (`legacyPluginIds` aliases for `nexus.activityTimeline → nexus.activity` and `nexus.notion → nexus.notionImport`; rename of the `osObservability` import target; addition of `graph/globalIndex.ts`). A regression guard was added at `shell/tests/catalog-disk-consistency.test.ts` to prevent the catalog and disk from silently drifting back.
- **2026-05-22** — A6 partially drained: the three `invoke('kernel_invoke', …)` calls in `SettingsPanelView.tsx` migrated to `api.kernel.invoke(...)` (the component already had `api` in scope at all three call sites). The two AI files flagged by the audit (`marginApi.ts`, `marginSuggest.ts`) had already been cleaned. Seven sites remain in the `plugin-import-hygiene.test.ts` allowlist as documented shell-internal exceptions; further drain needs new API surface (PlatformDialog.open, PlatformNotifications, PluginsDir).
- **2026-05-22** — B1 / B2 / B4 closed: `ipc-handlers.md` counts table + section headers updated against current matrix (332 handlers, 23 plugins); `audit-flags.md` rewritten to list the 4 still-unrestricted handlers (`workflow::run`, `workflow::run_digest`, `ai::resolve_credentials`, `mcp.host::call_tool`) with a separate "Closed since" section for the 14 already-gated ones; `settings/README.md` forge-layout table extended with 9 missing paths plus an explicit note resolving the dual `config.toml` confusion. New `scripts/check_ipc_docs_drift.sh` checks both docs against the matrix and is wired into `scripts/check_ipc_drift.sh` for CI. B3 (`hardcoded-rust.md`) deferred — needs row-by-row code verification.
- **2026-05-22** — D1 + D2 closed. D1: the per-request spawns in `nexus-remote::server::dispatch_request` and the per-connection spawn in `nexus-workflow::webhook_accept_loop` are now tracked in a `JoinSet` scoped to their owning accept loop / connection; drop of the future cascades aborts. D2: the four `nexus-collab` relay-slot `Mutex::lock().expect()` sites now go through a `lock_relay()` helper that recovers via `into_inner()` on poison and emits a `tracing::warn!`. Three of the audit's five D1 flags (`nexus-kernel/context_impl.rs:960`, `nexus-ai-runtime/core_plugin.rs:863`, `nexus-ai-runtime/scheduler.rs:503`) and both terminal D2 flags were inside test code (the audit's `grep -n` conflated test-only spawns/locks with production code) — left as-is.
- **2026-05-22** — D4 closed. Five un-flagged tunable constants (`READ_TIMEOUT_MS`, `WATCHER_CHANNEL_BOUND`, `DRAINER_PUMP_TIMEOUT_MS` / `DRAINER_SLEEP_MS`, `DEFAULT_HISTORY_SAMPLES`, `DESCRIPTION_FALLBACK_CAP`) now appear in `hardcoded-rust.md`'s "Already-named constants worth promoting" section with the suggested settings key on each row. The drive-by `nexus-workflow/src/digests.rs:49` `IPC_TIMEOUT` was migrated to `nexus_types::constants::IPC_TIMEOUT_LONG` per P5-01.
