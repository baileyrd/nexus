# Hardcoded Values — Shell Side

> **As of:** 2026-05-17. Companion to [`hardcoded-rust.md`](hardcoded-rust.md) and [`plugin-manifest-defaults.md`](plugin-manifest-defaults.md) (manifest-baked defaults). This refreshes the prior `shell/HARDCODED_SETTINGS_AUDIT.md` (dated 2026-04-25). For values already named/consolidated since then, see [`#verified-and-unchanged`](#verified-and-unchanged).
>
> Carry-over status from prior audit is preserved under [`shell/HARDCODED_SETTINGS_AUDIT.md`](../../../shell/HARDCODED_SETTINGS_AUDIT.md) — that file is the source-of-truth catalogue; this 0.1.2 page is the delta + nexus-extension-api extension.
>
> **See also:** [`hardcoded-audit-2026-06-19.md`](hardcoded-audit-2026-06-19.md) — the latest point-in-time audit (verified NEW shell findings: sandbox orchestrator timeouts, dev port, deactivation cap).

## Verified-and-unchanged

These items from the original 2026-04-25 audit still apply; many have been promoted to named constants in the interim.

| Original entry | Current file:line | Status |
|----------------|-------------------|--------|
| `core/zoom/index.ts:15-18` — zoom step/min/max/default | unchanged; schema wired | ✓ verified |
| `nexus/terminal/TerminalInstance.tsx:246,250` — `fontSize` 13, `scrollback` 5000 | promoted to schema keys `terminal.fontSize` / `terminal.scrollback` in `nexus/terminal/index.ts` (2026-06-19); font family is theme-driven via `--font-monospace`. (Prior `core/terminal/index.ts` cite was stale — that plugin no longer exists.) | ✓ promoted |
| `nexus/terminal/savedCommandsStore.ts:85` — `2_000` ms auto-restart | unchanged | ✓ verified |
| `nexus/canvas/renderer.ts:32` — `250×60` default text-node size | now `DEFAULT_TEXT_NODE_SIZE` at line 33 | ✓ named |
| `nexus/canvas/renderer.ts:36,40,292,294` — handle geometry | now named constants at lines 37-41 | ✓ named |
| `nexus/search/searchRuntime.ts:19` — `50` max results | now `MAX_SEARCH_RESULTS = 50` line 20 | ✓ named |
| `nexus/commandPalette/match.ts:9` — `50` commands | inline (no change) | ✓ verified |
| `nexus/canvas/CanvasOverlay.tsx:69,74` — `32 KB` + `250 ms` | named (`TERMINAL_NODE_BUFFER_CAP`, `TERMINAL_NODE_POLL_MS`) | ✓ named |
| `nexus/canvas/CanvasOverlay.tsx:588` — `64 KB` file preview cap | `FILE_PREVIEW_TEXT_CAP = 64 * 1024` | ✓ named |
| `nexus/processes/processesStore.ts:45` — `500` events cap | `PROCESS_EVENTS_CAP = 500` | ✓ named |
| `nexus/canvas/canvasStore.ts:36` — `200` undo cap | consolidated to `nexus/constants.ts` `UNDO_HISTORY_CAP = 200` | ✓ consolidated |
| `nexus/bases/basesStore.ts:89` — `200` history cap | now reads shared `UNDO_HISTORY_CAP` | ✓ consolidated |
| `nexus/canvas/renderer.ts:21-27,34` — CSS hex fallback tokens | still present (`FALLBACK_THEME`) | ✓ verified |
| `nexus/canvas/Minimap.tsx:77-79` — CSS hex fallback tokens | still present | ✓ verified |
| `nexus/graph/forceLayout.ts:34,35,37` — physics constants | inline (not in audit scope) | ✓ verified |
| `nexus/canvas/autoLayout.ts:56` — `250` iterations | now `AUTO_LAYOUT_ITERATIONS = 250` at line 23 | ✓ named |
| `nexus/search/searchRuntime.ts:76` — `150` ms debounce | `SEARCH_DEBOUNCE_MS = 150` at line 78 | ✓ named |

## Already resolved (no longer in tree)

| Original entry | Resolution |
|----------------|------------|
| Canvas/bases undo history split (`200` entries each) | Consolidated to `UNDO_HISTORY_CAP` in `nexus/constants.ts` (commit `cef4f6a`) |
| `EVENTS_CAP` (unqualified) | Renamed `PROCESS_EVENTS_CAP` for clarity |
| AI request timeout `60_000` ms | Changed to `300_000` ms (5 min) in `aiRuntime.ts:64` for local Ollama cold-start |
| Timeout consolidation TODO | `LONG_RUNNING_OP_TIMEOUT_MS` + `SERVICE_CONNECT_TIMEOUT_MS` now live in `nexus/constants.ts` |
| Notification duration split entries | Schema keys registered in plugin manifests; three new plugin-specific ones still open (below) |

## Newly identified (delta since 2026-04-25)

### User Config

| File | Line | Value | Suggested setting key |
|------|------|-------|----------------------|
| `shell/src/plugins/nexus/templates/index.ts` | 114 | `5000` ms | `ui.templateNotificationDurationMs` |
| `shell/src/plugins/nexus/notion/index.ts` | 103 | `6000` ms | `ui.notionSuccessNotificationMs` |
| `shell/src/plugins/nexus/notion/index.ts` | 110 | `8000` ms | `ui.notionErrorNotificationMs` |

### Dev Config

| File | Line | Value | Constant name |
|------|------|-------|--------------|
| `shell/src/shell/PopoutShell.tsx` | 90 | `300` ms | `POPOUT_BOUNDS_DEBOUNCE_MS` |
| `shell/src-tauri/src/bridge.rs` | 62 | `30_000` ms | `DEFAULT_INVOKE_TIMEOUT_MS` (already named) |
| `shell/src/plugins/nexus/canvas/CanvasOverlay.tsx` | 636 | `0x8000` (32 KB chunk) | `BYTES_TO_DATA_URL_CHUNK_SIZE` |

## `packages/nexus-extension-api/`

The TypeScript SDK carries no hardcoded limits at v0.1.2; timeouts flow as optional parameters from callers.

| File | Line | Value | Note |
|------|------|-------|------|
| `packages/nexus-extension-api/src/sandbox/context.ts` | 149 | `timeoutMs?: number` | optional param, no default |
| `packages/nexus-extension-api/src/sandbox/runtime.ts` | 234 | `timeoutMs?: number` | optional param, no default |
| `packages/nexus-extension-api/src/generated/ipc/DelegateArgs.ts` | 27 | `approval_timeout_secs: bigint \| null` | optional, no default |

## Suggested rollups

- **`shell/src/plugins/nexus/constants.ts`** — already exists; the right home for any remaining shell-wide cap/threshold.
- **`shell/src/shell/constants.ts`** — doesn't exist yet; would be the right home for `POPOUT_BOUNDS_DEBOUNCE_MS` and other shell-chrome timing.
- **`packages/nexus-extension-api/src/constants.ts`** — doesn't exist yet; could centralize SDK-side default timeouts once any are added.

## Cross-references with Rust side

Values that appear on **both** sides and should be unified rather than duplicated — currently the shell and Rust both carry their own copy:

- AI default model / max_tokens / temperature — Rust: `nexus-formats/src/config/ai.rs:35-39`; Shell: various places under `nexus/ai/`.
- IPC plugin id strings (`"com.nexus.storage"` etc.) — Rust: `nexus-mcp/src/server.rs:29-41`; Shell: `shell/src/types/plugin.ts` and dispatcher code.
- Notification timeout durations — split between Rust `nexus-notifications` and shell `notificationService`.

Recommendation: derive these in one place (e.g. the IPC ts-rs generator emits a `pluginIds.ts` constants file) and consume from both sides.
