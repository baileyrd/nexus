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
| A2 | Remaining `let _ = bus.publish*` sites (storage, notifications, mcp, bootstrap) | _this sweep_ |

Drive-by in `0eb8bcc0`: re-exported `in_flight_sync_dispatches` from `nexus-kernel` (function was added in `64237761` for "future metrics surfaces" but unreachable outside the crate; rustc was flagging it dead-code).

---

## A. High-priority gaps

### A1. Shell plugin catalog ↔ on-disk mismatch (7 entries)
The plugin catalog in `shell/src/plugins/catalog.ts` declares itself the source of truth, but disk reality diverges:

| # | Catalog says | Disk says | Effect |
|---|---|---|---|
| 1 | `nexus.activity` (no dir) | — | Catalog entry never loads |
| 2 | `nexus.osObservability` → `./observability/` | folder is `observability/` | Name mismatch |
| 3 | `nexus.notionImport` → `./notion/` | folder is `notion/` | Name mismatch |
| 4 | (not listed) | `shell/src/plugins/nexus/activityTimeline/` | Orphaned on disk |
| 5 | (not listed) | `shell/src/plugins/nexus/notion/` | Orphaned |
| 6 | (not listed) | `shell/src/plugins/nexus/observability/` | Orphaned |
| 7 | `graph.global` | not found | Phantom entry |

Plus 6 plugins with `index.ts` but only stub content: `bookmarks`, `debugger`, `fileProperties`, `healthPanel`, `searchPanel`, `statusBar`.

### A2. Silent error swallowing across event bus — ✅ Closed (`0eb8bcc0` + this sweep)
`let _ = bus.publish_plugin(...)` is the dominant pattern in plugin code. This violates the principle "never silently swallow exceptions". Sites:
- ✅ `crates/nexus-security/src/core_plugin.rs:121,154,166` — **audit events** (fixed in `0eb8bcc0`)
- ✅ `crates/nexus-storage/src/core_plugin.rs` — 6 state-change events in `publish_event` now routed through a `publish_storage_event` helper that warn-logs on bus failure.
- ✅ `crates/nexus-notifications/src/core_plugin.rs:558` (inbox.appended) and `:681` (ai-runtime toast republish) — now warn-log on failure with the relevant context (inbox id / source).
- ✅ `crates/nexus-mcp/src/core_plugin.rs:312` — `mcp.host.started` lifecycle event now warn-logs on failure.
- ✅ `crates/nexus-bootstrap/src/crdt_publisher.rs:293` — orphan tmp-file `remove_file` failure now warn-logs with the tmp path (was the "tmp-file leakage hidden" case).
- ✅ `crates/nexus-bootstrap/src/plugins/mod.rs:253` — `plugin_lifecycle_timeout` event now warn-logs on failure so observers know they missed the skip.

The notifications watcher `tx.send(res)` at `:949` is intentionally left as-is: it is a `std::sync::mpsc` send from a `notify` callback to the same thread that owns `rx`; the only failure mode is `rx` having been dropped, which means the watcher thread has already exited. Logging there cannot reach anyone.

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

### A6. Direct Tauri `invoke()` from non-host code (≥10 sites)
Per ADR 0011, only host code may call the Tauri bridge directly; plugins must route through `PluginAPI.kernel`. Bypasses:
- `shell/src/core/capabilityPrompt/requestConsent.ts:19`
- `shell/src/core/settings/SettingsPanelView.tsx:8` (3 calls)
- `shell/src/plugins/nexus/workspace/index.ts:3` + `useConnectionState.ts:7` (also `listen`)
- `shell/src/plugins/nexus/launcher/launcherState.ts:1`
- `shell/src/plugins/nexus/pluginsMgmt/index.ts:1`
- `shell/src/plugins/nexus/notifications/index.ts:12`
- `shell/src/plugins/nexus/debugger/LaunchConfig.tsx:30`
- `shell/src/plugins/nexus/ai/marginApi.ts`, `marginSuggest.ts` (multiple)

This is the same class as `App.tsx:8` (already flagged in AA-04) but spread further.

---

## B. Documentation drift

### B1. `docs/0.1.2/ipc-handlers.md` is stale by 52 handlers
Doc claims "~280 handlers"; `cap_matrix.toml` now has **332** (`grep -c "^\[\[handler\]\]"`).

| Plugin | Doc | Actual | Δ |
|---|---:|---:|---:|
| storage | 60 | **72** | +12 |
| ai | 28 | 26 | -2 |
| terminal | 32 | 28 | -4 |
| agent | 17 | 18 | +1 |
| mcp.host | 11 | 12 | +1 |
| acp | 7 | 8 | +1 |

The `cap_matrix_complete` test enforces handler→matrix but not matrix→doc; no drift script exists. Suggest extending `scripts/check_ipc_drift.sh` or `cap_matrix_complete` to compare doc counts.

### B2. `docs/0.1.2/reference/audit-flags.md` largely stale
Of 17 candidate rows, **only 3** still reflect uncapped handlers in `cap_matrix.toml` (`workflow::run`, `workflow::run_digest`, `ai::resolve_credentials`). The other 14 (e.g. `security::set_secret`, `git::push`, `terminal::send_input`, `collab::start_relay`) are already cap-gated. Delete the stale rows; add `mcp.host::call_tool` (currently has `# AUDIT:` at `cap_matrix.toml:861` but no doc entry).

### B3. `docs/0.1.2/settings/hardcoded-rust.md` Dev-Config table is stale
~15 rows describe constants that are already promoted to named `const`s in code (the "delete the row on promotion" workflow wasn't applied to refactor-driven promotions). Examples:
- `nexus-mcp/core_plugin.rs:551` → moved to line 570, already named `MAX_TOOL_RESPONSE_BYTES`.
- `nexus-editor/core_plugin.rs:2220` → moved to `handlers/transaction.rs:25`, already named.
- `nexus-collab/{client,server}.rs` MAX_FRAME_BYTES / BROADCAST_CAPACITY — already named.
- `nexus-ai/{vectorstore,rag,enrichment,indexing_daemon}.rs` — all already `pub const`.
- `nexus-tui/app.rs:1820,1825` — already `AGENT_IPC_TIMEOUT` / `MODAL_AUTO_REJECT_TIMEOUT`.

The genuinely-still-inline items (4 `for _ in 0..N` loops, the three `term.rs` durations, the `bge` model id) remain valid.

### B4. `docs/0.1.2/settings/README.md` forge layout missing ≥9 paths
Code writes these `.forge/` paths the README doesn't list:
- `.forge/comments/` (`nexus-comments/src/store.rs:62`)
- `.forge/templates/` (`nexus-templates/src/registry.rs:61`)
- `.forge/agents/` (`nexus-agent/src/memory.rs:40`)
- `.forge/digests/last_fired.json` (`nexus-workflow/src/digests.rs:131`)
- `.forge/skills/` (referenced from `nexus-mcp/src/server.rs:1180,1536`)
- `.forge/ai-activity.log` (`nexus-ai/src/activity_log.rs:42`)
- `.forge/.audio/models/` (`nexus-audio/src/config.rs:138`)
- `.forge/.editor/undo/{hex}.json` (`nexus-editor/src/handlers/session.rs:336`)
- `.forge/.gitignore` + `.gitattributes` (`nexus-cli/src/commands/crdt.rs:110,208`)

Also: README mentions `.forge/acp.toml` as reserved but `nexus-acp` doesn't appear to load it. Two distinct `config.toml` files exist (`<forge>/.nexus/config.toml` for kernel, `<forge>/.forge/config.toml` for audio) — flag the collision.

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

### D1. `tokio::spawn` orphans (no JoinSet tracking)
- `nexus-remote/src/server.rs:158,166,179,289` — per-request handler spawns in a long-running JSON-RPC server with no shutdown signal path
- `nexus-workflow/src/core_plugin.rs:1372` — webhook per-peer handlers
- `nexus-kernel/src/context_impl.rs:960` — timer-fire CancelToken task
- `nexus-ai-runtime/src/core_plugin.rs:863`, `scheduler.rs:503`

### D2. `Mutex::lock().expect()` in long-lived plugins
- `nexus-collab/src/core_plugin.rs:317,370,391,421` (4 sites, all in the relay plugin)
- `nexus-terminal/src/core_plugin.rs:3008,3067`

Poisoning will kill the plugin. Either don't `.expect()` or restart-on-poison.

### D3. Handlers don't log on error return
Spot-checked `crates/nexus-storage/src/handlers/{files,index,notes}.rs` and `nexus-ai/src/handlers/{ask,search}.rs`: **zero** `tracing::error/warn` calls. Errors are `?`-propagated; only the dispatcher logs them, losing handler-specific context.

### D4. 5 un-flagged constants that look user-tunable
Not in `hardcoded-rust.md` but probably should be:
1. `nexus-workflow/src/webhook.rs:57` — `READ_TIMEOUT_MS: u64 = 5_000` (drops slow payloads)
2. `nexus-storage/src/watcher.rs:28` — `WATCHER_CHANNEL_BOUND: usize = 1024` (just fixed; should be tunable)
3. `nexus-terminal/src/core_plugin.rs:1102-1109` — PTY pump tuning (`DRAINER_PUMP_TIMEOUT_MS=5`, `DRAINER_SLEEP_MS=10`, …)
4. `nexus-terminal/src/memory.rs:38` — `DEFAULT_HISTORY_SAMPLES: usize = 60` (history depth)
5. `nexus-storage/src/entity_index.rs:865` — `DESCRIPTION_FALLBACK_CAP: usize = 240` (UI-visible)

Also: `nexus-workflow/src/digests.rs:49` `IPC_TIMEOUT = Duration::from_secs(120)` should use the shared `nexus_types::constants::IPC_TIMEOUT_LONG` per the recent P5-01 standardization but doesn't.

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

1. ~~**A2 / A3 / D3** — sweep `let _ = .publish*` and `.unwrap_or_default()` on serialize/deserialize; add `tracing::warn!` everywhere data loss is currently silent.~~ **A2 closed (`0eb8bcc0` + this sweep), A3 closed (`88f990cd`). D3 (handler-side error logging) still open and tracked separately.**
2. ~~**A4** — bound the three protocol-client channels; same OOM class as the watcher fix.~~ ✅ Closed (`22aa9f88`).
3. **A1** — reconcile catalog.ts with disk; either delete orphans or wire them up. ~7-line fix once decided.
4. **A6** — sweep direct `invoke()` calls out of plugin code; route through PluginAPI. Bundle with AA-04.
5. **B1 / B2 / B3 / B4** — refresh the four stale documents; add a `scripts/check_ipc_docs_drift.sh` to prevent regression.
6. **A5** — issue #77; per-step caps aren't enough — design an aggregation rule for `workflow::run`.
7. **D1 / D2** — wrap orphan spawns in `JoinSet`s; handle `Mutex` poisoning instead of `.expect()`.
8. **C1** — capability-vocabulary cleanup pass (singletons, read/write symmetry).

None of A–D are release-blocking; A2/A3/D3 are the most direct correctness/observability wins.

---

## Changelog

- **2026-05-21** — initial audit; A2 (audit plugin sites only), A3, and A4 closed in the same session. See `git log --grep "fix(security)\|fix(lsp,dap,acp)\|fix(storage,notifications,workflow)"`.
- **2026-05-21 (later)** — A2 remaining sites closed: storage `publish_event` (6 events), notifications inbox.appended + ai-runtime toast republish, mcp.host.started lifecycle, bootstrap CRDT tmp-file cleanup, bootstrap `plugin_lifecycle_timeout` event.
