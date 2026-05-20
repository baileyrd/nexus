# Phase 5 — Strategic Decisions

Companion to [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) §Phase 5. Each item below was flagged as needing product / architecture sign-off rather than engineering judgement. This document records the current state, the options that were considered, and the resolution (or the open question if one is still required).

## Status roll-up

| Item  | Topic                              | Resolution                          | Lands in            |
|-------|------------------------------------|-------------------------------------|---------------------|
| 5.1   | Ship or cut `com.nexus.acp`        | **Keep as experimental**            | This commit (doc + crate header) |
| 5.2   | `com.nexus.audio` local default    | **Open** — needs product call       | Documented, not changed |
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

**Resolution:** **Deferred — needs product call.** Each option trades off a different axis (binary size vs setup friction vs feature parity). The engineering team can land any of them in <30 minutes once the choice is made.

If no decision is made, **recommendation:** option 1 (Platform default). Web Speech is the only backend that works with zero setup AND zero build-time changes; users on backed-up internet or who prefer on-device can still flip to `Provider` or build with `local-whisper`.

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

## What remains genuinely open

Only **5.2** (audio default backend) requires a decision-maker's input. The right call is product, not engineering; this doc captures the trade-offs so whoever picks it up can act in minutes rather than re-do the analysis.
