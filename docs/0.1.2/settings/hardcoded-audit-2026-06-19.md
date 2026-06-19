# Hardcoded Values / Settings Audit — 2026-06-19

> **As of:** 2026-06-19. A point-in-time sweep for hardcoded variables/settings
> across the Rust backend (`crates/*`) and the shell/packages TypeScript. Method:
> three scans (existing-tracking baseline, Rust, shell) plus **direct source
> verification of the top findings** — which corrected two over-stated candidates
> (see [§B](#b-corrections--verified-false-positives-not-findings)).
>
> Companion to the living trackers [`hardcoded-rust.md`](hardcoded-rust.md),
> [`hardcoded-shell.md`](hardcoded-shell.md), and
> [`plugin-manifest-defaults.md`](plugin-manifest-defaults.md). This page records
> what is **newly found**, **corrected**, and **systemic**; it defers to those
> docs for the full row-level catalogue.

## Headline

Settings discipline is **strong**. Most tunables already live in per-service
`Config` structs with `#[serde(default)]`, the shell has excellent named-constant
hygiene, and the tracking docs are broadly current. The genuinely **untracked**
gaps are small and concentrated — the most significant is one subsystem
(`nexus-linkpreview`) that has **no config struct at all**.

## A. Verified NEW findings (untracked, genuine settings candidates)

| # | Location | Value(s) | Controls | Verdict |
|---|----------|----------|----------|---------|
| 1 | `crates/nexus-linkpreview/src/lib.rs:38,42,45,49` | `FETCH_TIMEOUT=5s`, `MAX_BODY_BYTES=512KB`, `USER_AGENT`, `MAX_REDIRECTS=5` | Canvas link-preview fetch (SSRF/parse safeguards + slow-CDN tolerance) | **Strongest gap.** No `LinkPreviewConfig` exists — all tunables are module consts, absent from the settings config-struct index. Operator-relevant. |
| 2 | `shell/src/host/sandbox/SandboxOrchestrator.ts:141-143` | `DEFAULT_HANDSHAKE_TIMEOUT_MS=5_000`, `DEFAULT_PING_INTERVAL_MS=10_000`, `DEFAULT_MAX_MISSED_PONGS=2` | Community-plugin handshake + crash detection | Named consts; defaults for optional ctor params; **no user override path.** Advanced/dev-config. |
| 3 | `shell/src/host/sandbox/router.ts:148`; `shell/src/host/ExtensionHost.ts:208` | `30_000`ms call timeout; `1000`ms deactivation cap | Sandbox method-call hang; clean-shutdown budget | Dev-config. Surface under "Advanced → Plugin isolation" if anywhere. |
| 4 | `crates/nexus-collab/src/reconnect_client.rs:82-86` | `initial_delay=1s`, `buffer_capacity=256` | Relay reconnect/buffering | **Partial gap** — `[collab]` already exposes `backoff_factor`/`max_delay_ms` but not these two. Complete the struct. |
| 5 | `crates/nexus-memory/src/sync.rs:21`; `crates/nexus-memory/src/capture_pipeline.rs:21` | `30s`; `120s` | memory-hub sync HTTP; capture IPC timeouts | Low priority (sync server / internal pipeline). |
| 6 | `shell/src-tauri/tauri.conf.json:9`; `shell/vite.config.ts:26` | `localhost:1420` | Vite dev port/URL | **Build-time only**, not a runtime setting; env-var-friendly at most (CI/containers). |

**Window dimensions** (`shell/src-tauri/tauri.conf.json:16-19`, `1280×800` /
min `600×400`) are **first-run defaults only** — the shell already persists and
restores window geometry via `tauri-plugin-window-state`, so this is low
priority (off-screen recovery is a separate concern, not a settings gap).

## B. Corrections — verified false positives (NOT findings)

These were flagged by the automated scans but **direct source reads show they are
already handled** — recorded here so the audit isn't padded with non-issues:

- **`crates/nexus-agent/src/auto_notify.rs:25`** (`DEFAULT_THRESHOLD_S = 30`) — it
  is **already a setting**: `[agent].auto_notify_threshold_s` in `config.toml`
  (default 30s; `0` disables the subscriber). The module doc-comment and the
  `AgentSection` serde struct confirm the override path. Not a gap.
- **`crates/nexus-ai/src/handlers/predict.rs:39`** (`COMPLETION_CHAR_CAP = 2048`)
  — a documented **render-safety guardrail** bounding ghost-text size. The
  user-facing knob is `AiConfig::predict_max_tokens` (default 64, set via
  `[ai] predict_max_tokens` in `ai.toml`), which already exists. Leave as an
  intentional constant.

## C. Already tracked & still open (known — in the living docs)

No rediscovery needed; these are carried in the trackers:

- **User-config:** P2-05 TLS connect/read timeouts (`nexus-security/src/tls.rs`);
  P2-07 notifications webhook timeout (`nexus-notifications/src/lib.rs`).
- **Dev-config:** ~29 live inline timeout/limit literals (CLI TTY reads, terminal
  drainer pump, LSP/ACP/DAP/MCP request+register timeouts, agent tool/chat
  timeouts, storage watcher channel bound, …) and ~11 already-named consts flagged
  for surfacing — see [`hardcoded-rust.md`](hardcoded-rust.md).
- **Shell:** notification durations (`templates/index.ts`, `notion/index.ts`),
  popout debounce, canvas chunk size — see [`hardcoded-shell.md`](hardcoded-shell.md).

## D. Systemic / hygiene findings (higher-value than any single literal)

1. **Stale tracking rows.** The 2026-05-22 "B3 sweep" found **~72 struck rows**
   (promotions that skipped the convention's *"delete the row"* step) plus **~5
   drifted line cites** (e.g. `nexus-editor/src/core_plugin.rs:42`,
   `nexus-skills/src/core_plugin.rs:188`) pointing at moved code. The tracker is
   accurate today but drifts whenever a promotion forgets step 5 of
   [`README.md`](README.md#how-to-add-a-setting).
2. **Rust↔shell value duplication.** AI model defaults, `max_tokens`,
   `temperature`, and `com.nexus.*` plugin-id strings are hardcoded on **both**
   sides with no single source. The docs' own recommendation — a generated shared
   constants file (ts-rs → `pluginIds.ts`, shared AI defaults) consumed by both —
   is unimplemented.
3. **No system-wide WASM resource ceiling.** `WasmConfig` defaults (16 MB / 10 M
   fuel / 5000 ms) are per-plugin overridable, but no `KernelConfig` ceiling caps
   what a manifest can request.
4. **Fully-hardcoded shell plugin priorities.** ~17 activity-bar and ~13
   view/overlay priorities have no override path (keybindings, by contrast, are
   registry-overridable). See [`plugin-manifest-defaults.md`](plugin-manifest-defaults.md).

## E. Recommended priority order

1. **Add `LinkPreviewConfig`** — `[linkpreview]` with `fetch_timeout_secs`,
   `max_body_bytes`, `max_redirects`, `user_agent`. The one subsystem with zero
   config surface; operator-relevant. *(A.1)*
2. **Complete `[collab]` reconnect** — add `initial_delay_secs` + `buffer_capacity`
   to the existing section. *(A.4)*
3. **Promote the two open user-config timeouts** already tracked — P2-05 TLS,
   P2-07 webhook. *(C)*
4. **Hygiene** — re-sync `hardcoded-rust.md` (clear struck rows, fix the ~5
   drifted cites) and add the generated shared-constants file to kill the
   Rust↔shell duplication. *(D.1, D.2)*
5. **Optional/advanced** — surface the sandbox orchestrator timeouts and a
   WASM resource ceiling for power users. *(A.2, A.3, D.3)*

Each promotion follows the convention in
[`README.md` → How to add a setting](README.md#how-to-add-a-setting): add a
`#[serde(default)]` field to the owning crate's `Config`, register a
`SettingsSchema` if user-facing, document it, and delete the corresponding
tracker row.
