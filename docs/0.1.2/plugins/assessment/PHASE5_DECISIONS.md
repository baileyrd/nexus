# Phase 5 — Strategic Decisions

Companion to [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) §Phase 5. Each item below was flagged as needing product / architecture sign-off rather than engineering judgement. This document records the current state, the options that were considered, and the resolution (or the open question if one is still required).

## Status roll-up

| Item  | Topic                              | Resolution                          | Lands in            |
|-------|------------------------------------|-------------------------------------|---------------------|
| 5.1   | Ship or cut `com.nexus.acp`        | **Keep as experimental**            | This commit (doc + crate header) |
| 5.2   | `com.nexus.audio` local default    | **Platform default**                | This commit             |
| 5.3   | `com.nexus.dap` / `com.nexus.lsp`  | **Both keep — original concern was wrong** | This commit (doc only) |
| 5.4   | `agent ↔ skills` cycle intent      | **Intentional cycle — documented**  | Phase 4.9 (`635ac11c`) |
| 5.5   | Drop or repurpose `nexus.sidebar`  | **Dropped**                         | Phase 2 (`cbc6871f`) |
| 5.6   | Fold or keep `nexus.linkSuggest`   | **Kept**                            | Phase 2 (commit msg) |

## 5.1 — `com.nexus.acp`: keep as experimental

**State observed at 0.1.2:**
- The crate is fully implemented, registered, and unit-tested (`crates/nexus-bootstrap/tests/acp_*.rs`).
- IPC surface: 8 handlers (`list_agents`, `initialize`, `propose`, `accept`, `reject`, `register_server`, `unregister_server`, `disconnect`).
- **No shell plugin imports it.** Grepping `shell/src/` for `com.nexus.acp` returns one match — the id constant in `shell/src/types/pluginIds.ts:14`. No `api.kernel.invoke('com.nexus.acp', …)` call exists in any shell plugin.
- The only user-facing entry point is the inbound `nexus acp serve` CLI subcommand (`crates/nexus-cli/src/commands/acp.rs`).
- The `first-party-acp-echo` example plugin exercises the outbound contribution wiring.

**Options considered:**
1. **Cut** — remove `nexus-acp` from `register_all` and behind a feature gate. Saves ~3000 LOC compile + boot time.
2. **Keep + gate** — add a `--experimental` bootstrap flag that opt-in enables ACP. More surface area for the same end-state.
3. **Keep as-is + document the status** — leaves the plugin loaded but with clear "no consumer yet" signage so the next reader doesn't waste time tracing dead paths.

**Resolution:** option 3. ACP is small (lifecycle::NONE — request-driven only) and the cost of leaving it loaded is near-zero. The cost of cutting it would be losing the test coverage that validates the contribution wiring (which the next consumer will rely on). The crate's `core_plugin.rs` header now carries an explicit "Status (0.1.2): experimental — no in-tree consumer" note pointing here.

**Trigger to revisit:** when a shell plugin actually invokes `com.nexus.acp::*`, drop the experimental tag.

## 5.2 — `com.nexus.audio` local backend default: open

**State observed:**
- `AudioConfig::default()` (`crates/nexus-audio/src/config.rs:127`) sets both `stt_backend` and `tts_backend` to `AudioBackendName::Local`.
- The shipped build does NOT enable the `local-whisper` cargo feature, so the `Local` variant resolves to a stub backend that returns `BackendNotEnabled` on first dispatch.
- The doc comment on `AudioBackendName::Local` (line 22-25) already acknowledges this — "flip the config to `provider` if you haven't built with the feature on."
- `Provider` works but requires `OPENAI_API_KEY` (or `provider_api_key` in `[audio]`). No key → `Misconfigured` error.
- `Platform` (Web Speech API) works out-of-the-box in WebView2/WebKit (BL-118) — no key, no model download.

**Options considered:**
1. **Switch default to `Platform`** — works out-of-the-box, no setup. But quality is browser-vendor-dependent and may surprise users expecting Whisper.
2. **Switch default to `Provider` and document the key requirement** — predictable quality but every fresh forge fails until the user configures a key.
3. **Build the shipping release with `local-whisper` enabled** — the original BL-117 intent. Adds binary size + model-download UX. Requires audit of the Whisper licence + redistribution.
4. **Keep `Local` default and improve the error message** — current behaviour; cheap but bad first impression.

**Resolution: Platform default.** Both `stt_backend` and `tts_backend` defaults flipped from `Local` to `Platform` in `AudioConfig::default()` (`crates/nexus-audio/src/config.rs:128-129`). Rationale: of the three backends, Platform has the lightest setup ask — no API key, no model download, no cargo-feature build. The Rust side still ships a stub, but the `nexus.audio` shell plugin contributes a Web Speech adapter at runtime via BL-113, so once a user enables that plugin, audio works without any further configuration. Operators on backed-up internet or who prefer on-device transcription can still flip `[audio] stt_backend = "local"` (with a `local-whisper` build) or `"provider"` (with `OPENAI_API_KEY`).

Doc comment on `AudioBackendName::Platform` and the `AudioConfig` struct updated to reflect the new default. Test `load_returns_defaults_when_no_config_file` (line 311-315) asserts Platform. `forge-config.md` sample TOML updated.

## 5.3 — `com.nexus.dap` / `com.nexus.lsp`: both have consumers, keep

The original concern (raised in `IMPLEMENTATION_PLAN.md` §5.3) was that DAP might be unused in-tree. **Direct evidence disproves this:**

- **`com.nexus.lsp`** is consumed by `nexus.diagnostics` (publishes `com.nexus.lsp.textDocument.publishDiagnostics`).
- **`com.nexus.dap`** is consumed by `nexus.debugger` — see `shell/src/plugins/nexus/debugger/debuggerIpc.ts:11` (`const PLUGIN_ID = 'com.nexus.dap'`) and `shell/src/plugins/nexus/debugger/index.tsx:22-28` (7 `com.nexus.dap.*` topic subscriptions).
- Both have BL-113 contribution wiring + integration tests under `crates/nexus-bootstrap/tests/{lsp,dap}_contribution_wiring.rs`.

**Resolution:** **Keep both.** No action needed; this entry was a false positive in the original assessment. The DAP debugger plugin is default-off but fully wired end-to-end.

## 5.4 — `agent ↔ skills` cycle intent: intentional

Resolved in Phase 4.9 (commit `635ac11c`). Both `crates/nexus-agent/src/core_plugin.rs` and `crates/nexus-skills/src/core_plugin.rs` carry mirrored module docs explaining that the cycle is functional (async, lock-free) and required for either plugin to fully function. Boot order loads skills before agent so the load-time half of the cycle is broken; only the runtime half remains.

## 5.5 — `nexus.sidebar` stub: dropped

Resolved in Phase 2 (commit `cbc6871f`). The stub was removed, every other plugin's `dependsOn: ['nexus.sidebar']` declaration was stripped, and the catalog entry was deleted.

## 5.6 — `nexus.linkSuggest` shim: kept

Resolved in Phase 2 (decision recorded in `cbc6871f` commit message). The shim hosts two user-facing settings; removing it would orphan those settings in existing forges with no behaviour upside. Kept as-is.

## 4.3 — Consolidate links plugins: revised scope

Not strictly a Phase 5 item, but the same "needs direction" tag applies. Captured here so the original plan's recommendation can be revisited with evidence.

**The plan said:** Merge `outgoingLinks` + `tags` + `backlinks` + per-file `graph` into a single `nexus.links` panel with tabs.

**On closer reading, three findings change the cost/benefit:**

1. **The 4 plugins are already rendered as a tab cluster in the right panel.** Each emits `rightPanel:registerTab` at activate-time; the right-panel host collects them and renders Backlinks / Outgoing / Tags / Graph as sibling tabs. Users today already see them as a unified panel. The visible UX gain of "consolidate into one tabbed panel" is small to zero.

2. **The kernel calls are not shared the way the plan implied.** The plan assumed all four read the same `backlinks` / `outgoing_links` handlers. Actual:
   - `nexus.backlinks` → `com.nexus.storage::backlinks` + `backlinks_to_block`
   - `nexus.outgoingLinks` → `com.nexus.storage::outgoing_links`
   - `nexus.tags` → `com.nexus.storage::query_tags` + `read_frontmatter`
   - `nexus.graph` → mix of `backlinks` + `outgoing_links` (per-file view)
   So `tags` is the odd one out, and the "shared call" simplification doesn't survive contact with the code.

3. **Implementation cost is high relative to benefit.** Each plugin has its own zustand store, request-id race guard, kernel-availability guard, active-file subscriber, and on-change refresh hook. Total ~1,600 LOC across 7 files (`outgoingLinks/index.tsx` 184, `tags/index.tsx` 226, `backlinks/{index.ts, BacklinksView.tsx}` 623, `graph/{index.ts, GraphView.tsx, GraphPaneView.tsx}` 571). A merge that preserves behavior means re-implementing all of that subscriber wiring in one plugin — substantial regression surface for 4 default-off features at once.

**Resolution — implemented as `nexus.noteContext` accordion** (commits `ff530f49` through `5d38d09b`, 6 steps).

UX inputs received from the user:
- **Shape**: B — vertical accordion, each section collapsible, multiple can be expanded.
- **Name**: "Note Context".
- **Lazy-load**: hard — each section's data subscriber starts when expanded, stops when collapsed.
- **Per-file `nexus.graph`**: keep as a standalone plugin too; embedded as the Graph section via direct `<GraphView />` reuse (no duplicate subscriber).

Lands the Phase 4.1a `useActiveFileQuery` hook as part of the migration: outgoingLinks/tags/backlinks sections all use it. Graph section reuses `nexus.graph`'s existing `useGraphStore` + `GraphView`.

Migration:
- `nexus.backlinks`, `nexus.outgoingLinks`, `nexus.tags` deleted (~1474 LOC net removal across 9 files).
- `nexus.noteContext` catalog entry declares `legacyPluginIds: ['nexus.backlinks', 'nexus.outgoingLinks', 'nexus.tags']` so existing forges with any of those enabled get noteContext auto-enabled on next boot.
- Legacy focus command ids (`nexus.backlinks.focus`, `nexus.outgoingLinks.focus`, `nexus.tags.focus`) registered as aliases that focus the panel and expand the matching section — keybindings + palette muscle memory survive.

Known minor regression — captured as a follow-up:
- The "X backlinks" indicator in `RightPanelFooter` and `statusBar/FileStats` was driven by `useBacklinksStore`, which the retired plugin populated. Re-adding it means re-introducing an always-on subscriber outside the accordion's lazy-load contract. Both surfaces drop the column cleanly for now (words/chars/sync still shown).

Skipped features (each can be picked up later as small follow-ups):
- BL-049 phase 4 block-filter mode (toggle between `backlinks` and `backlinks_to_block` IPCs to narrow to a specific block id).
- On-edit silent refresh — the legacy plugin's own comment noted this was "largely a no-op" because editing file A doesn't change file A's incoming backlinks, so the skip is low-cost.

## 4.8 — storage compile-time deps: withdrawn

Originally flagged in DEPENDENCIES.md as a "compile-time leak past the IPC seam." Code-level review during execution found the framing was wrong:

- `nexus-formats/src/lib.rs:5` explicitly declares itself a pure-logic parsers library with "No runtime services; no SQLite."
- `nexus-database/src/lib.rs:3-8` explicitly declares itself "pure-logic — it does not touch `rusqlite`. The SQL-backed query engine ... moved into `nexus-storage`."
- `crates/nexus-bootstrap/tests/dep_invariants.rs::FORBIDDEN` enforces the layering from the other side — `nexus-database` is forbidden from linking `rusqlite`.

The IPC seam is for cross-PLUGIN dispatch, not cross-CRATE library reuse. `nexus-storage` legitimately depends on these pure-logic crates as bottom-tier libraries — that's the intended layering, not a leak. **No code change.** Doc claims in DEPENDENCIES.md, `_extract-rust-deps.md`, and IMPLEMENTATION_PLAN.md §4.8 updated to reflect the corrected analysis.

## 4.3 — links-panel consolidation

Landed end-to-end as `nexus.noteContext` across 6 steps (commits `ff530f49` → `5d38d09b`). User-provided UX inputs: accordion shape, "Note Context" name, hard lazy-load, keep `nexus.graph` standalone.

The three known regressions from step 6 have all been restored:

- **Backlinks-count indicator** (commit `9f825ac5`) — `RightPanelFooter` + `FileStats` re-add the "X backlinks" column, now reading from `noteContext`'s shared `useBacklinksDataStore`. An always-on subscriber in `noteContext.activate` populates it (deliberate deviation from hard-lazy; documented in `backlinksLoader.ts` header).
- **BL-049 block-filter** (commit `7fccb290`) — clicking a `^<block-id>` fragment chip narrows the panel via `backlinks_to_block`; chip with `×` clears it. Heading-anchor fragments stay non-interactive.
- **On-edit silent refresh** (commit `1db6eece`) — subscribes to `sessionManager.onChanged`, filters to active file, rAF-coalesces bursts, re-fetches without flashing loading.

## 4.1 — all four singletons narrowed

After the Phase 4.1a prototype (`notificationsSettings`), the user asked to do the remaining three as well. They landed in two commits:

- **`searchRuntime` + `recallApi`** (commit `54de2f15`) — same adapter pattern as 4.1a. Stored handle is a typed `SearchKernel` / `RecallApi` interface listing every kernel + config call the plugin makes. `recallApi`'s test stub rewritten to assert against the narrow shape directly (drops the `as any` casts).
- **`themePicker`** (commit `51200d06`) — wider surface, narrowed in two layers: `themeStore` actions now take `KernelAPI` instead of `PluginAPI`, and `pickerRuntime` stores `Pick<PluginAPI, 'kernel' | 'platform'>`. Call sites in `ThemePicker.tsx`, `SettingsPanelView.tsx`, `themeService/index.ts`, and the `themeStore` tests pass `.kernel` where they used to pass the full PluginAPI.

The original assessment listed `pickerRuntime` as a fifth singleton — that was conflated with `themePicker/pickerRuntime.ts`. The `nexus.pick` plugin uses a published-late-binding pattern (host's `api.input.pick` lazy-imports `requestPick`), which is intentional and not a singleton.

## What remains genuinely open

Nothing. The assessment cycle (Phases 0–5 plus the follow-ups landed in this round) is functionally complete; every item has either landed or has a documented reason for no action.
