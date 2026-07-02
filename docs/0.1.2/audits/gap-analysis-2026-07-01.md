# Gap Analysis — What Could / Should Be Added

> **As of:** 2026-07-01. A full-workspace sweep (42 crates, shell, packages,
> CI) hunting for incomplete work *as evidenced in the code itself*: phase
> markers, deferred-scope comments, stub handlers, unwired flags, and
> structural gaps. Method: direct source reads plus parallel exploration
> passes over kernel/storage, AI/agent, editor/dev-tooling, shell/bootstrap,
> staging crates, and CI/test posture. When this doc disagrees with the code,
> the code wins.
>
> Companion audits: [`repo-review-2026-06-10.md`](repo-review-2026-06-10.md)
> (V-series findings referenced below),
> [`expert-review-2026-05-31.md`](expert-review-2026-05-31.md), and the living
> [`../architecture-adherence.md`](../architecture-adherence.md).

## TL;DR

The workspace is unusually disciplined — only 9 `TODO`/`FIXME` markers across
~236k lines of Rust; gaps are instead tracked as explicit phase markers
(`BL-*`, `WI-*`, `DG-*`, `OI-*`) in doc comments. The highest-leverage work is
not new features but **finishing four explicitly half-done systems**:

| # | System | Evidence | Why it matters |
|---|--------|----------|----------------|
| 1 | Extension-API contract (three divergent shapes) | `packages/nexus-extension-api/CONTRACT_STATUS.md`, #187 / V9 | Blocks the community-plugin ecosystem |
| 2 | Memory persistence (Phase 5) | `crates/nexus-memory/src/lib.rs:12-15` | Memories lost on restart |
| 3 | Plugin marketplace (Phase 5 stub) | `crates/nexus-cli/src/commands/plugin.rs:499` (WI-44) | No install/update/version-check flow |
| 4 | Collab + hub security | `crates/nexus-collab/src/auth.rs:1-7`, `crates/nexus-memory-hub` | Shared token, plaintext relay — LAN-only today |

Beyond those, the biggest genuinely-missing pieces: auto-update, Linux/macOS
release packaging, retrieval quality (hybrid search / reranking), and an
exporter for the kernel metrics that are already collected.

---

## 1. Finish what the code says is unfinished

Gaps the code itself declares, with the declaring comment cited.

### 1.1 Extension-API contract unification (#187 / V9) — HIGH

Three plugin-context shapes coexist:

| Shape | Source | Status |
|-------|--------|--------|
| `NexusPluginContext` | `packages/nexus-extension-api/src/index.ts` | Aspirational — implemented by no runtime |
| `PluginAPI` | `shell/src/types/plugin.ts` | Live — in-process first-party plugins |
| `SandboxedPluginContext` | `packages/nexus-extension-api/src/sandbox/context.ts` | Live — iframe community plugins |

Field-level divergences are catalogued in
`packages/nexus-extension-api/CONTRACT_STATUS.md` (settings shapes
incompatible, `ipc` verb names differ, sandbox lacks `editor` / `workspace` /
`configuration` / `ai` entirely — `sandbox/context.ts:41` carries the
`configuration` TODO). The package is versioned as if stable while its own
header disclaims structural freeze, and there are **zero conformance tests**
binding any runtime to the declared contract. The repo review (V9) recommends:
freeze the canonical sandbox shape, delete or clearly quarantine the
unimplemented target, and add conformance tests.

### 1.2 Memory persistence — Phase 5 (HIGH)

`crates/nexus-memory/src/lib.rs:12-15`: all three stores are in-memory in
Phase 1. Episodic is a bounded ring (default 1024) that silently drops the
oldest entry (`episodic.rs:9`); semantic search is keyword/prefix hash-map
matching (`semantic.rs:6`); procedural triggers are substring matches. The
code plans SQLite persistence and embedding-based recall. Until then, memory
does not survive a restart.

### 1.3 Plugin marketplace — WI-44 Phase 5 stub (MEDIUM)

`crates/nexus-cli/src/commands/plugin.rs:499`: `nexus plugin install <id>`
returns a stub for marketplace ids; only local-directory install works.
Missing: registry/fetch flow, update checks, and enforcement of manifest
`api_version` (advisory only today). Ed25519 manifest verification already
exists shell-side, so the trust foundation is in place.

### 1.4 Collab + memory-hub security (MEDIUM, gates multi-user)

- Relay auth is a single static shared token, constant-time compared
  (`crates/nexus-collab/src/auth.rs:1-7`); per-user credentials deferred.
- No TLS and no E2E encryption — CRDT ops + presence ship plaintext.
- Single in-memory broadcast channel; hosted/multi-channel relays deferred
  (`crates/nexus-collab/src/server.rs:19-20`).
- `nexus-memory-hub`: single bearer token, any `node_id` accepted, no
  encryption at rest, no change outbox for foreign-authored edits.

### 1.5 Smaller declared gaps

| Gap | Evidence |
|-----|----------|
| AI-runtime run persistence (`runs.db`) not wired | `crates/nexus-ai-runtime/src/scheduler.rs:9` |
| Workflow run-history Phase 4 + `webhook` trigger reserved | `crates/nexus-workflow/src/handlers/run.rs:686,1028` |
| REPL output streaming (WI-12 Phase 2) | `crates/nexus-terminal/src/handlers/repl.rs` |
| CLI `--quiet` / `--config` accepted but unwired | `crates/nexus-cli/src/main.rs:224,228` |
| `nexus db list/show` deferred | `crates/nexus-cli/src/commands/db.rs:9-10` |
| OS sandbox: Linux landlock/seccomp only; macOS seatbelt (F2) + Windows restricted-token (F3) pending | `crates/nexus-security/src/os_sandbox.rs` |
| CRDT `OpLog::prune()` has no caller; structural-delete conflict UI missing | `crates/nexus-crdt/src/log.rs:30`, `lib.rs:28-34` |
| Staging crates complete but unwired (#188 / V19) | `crates/nexus-context`, `crates/nexus-protocol` |
| PowerShell/pwsh shell detection deferred | `crates/nexus-terminal/src/shell.rs:11` |
| MCP: no WebSocket transport, auth refresh-on-401 absent | `crates/nexus-mcp/src/config.rs:351,387` |

Note `nexus-context` (token-budgeted context builder) and `nexus-protocol`
(typed speech-act messages) are implemented and tested (13 + 14 unit tests) —
wiring them into `nexus-ai-runtime` centralizes per-provider context assembly.

## 2. Retrieval quality — cheap wins available

`crates/nexus-storage/src/vectorstore.rs:10-11` states the design point:
similarity search **loads every vector into memory and brute-forces cosine**
— "appropriate for personal-knowledge-base sizes". Ascending-effort ladder:

1. **Hybrid search.** Tantivy BM25 (`search.rs`) and the vector store already
   live in the same crate; reciprocal-rank fusion is a small patch with
   outsized quality gains. `nexus-ai` comments list hybrid as deferred.
2. **Reranking** of top-K results (explicitly deferred in `nexus-ai`).
3. **ANN index** (sqlite-vec / HNSW sidecar) once brute force stops scaling.
4. **Adaptive chunking** — fixed-size chunks today.
5. Product feature on top: **"related notes" / auto-link suggestions** —
   embeddings + knowledge graph both exist; connecting them is a plugin.

## 3. Desktop product completeness

| Gap | Evidence | Note |
|-----|----------|------|
| **Auto-update absent** | no `tauri-plugin-updater` in `shell/src-tauri/Cargo.toml` | Most user-visible missing feature |
| **Windows-only releases** | `.github/workflows/release-windows.yml` only | No `.deb`/AppImage/`.dmg`, no code-signing, no checksums/SLSA (acknowledged in `RELEASE.md`) |
| **No i18n** | no framework, no translation keys; RTL is one editor toggle (`SettingsPanelView.tsx:803-809`) | Retrofit cost grows monotonically |
| **Settings debt inventoried but open** | ~29 live literals in [`../settings/hardcoded-rust.md`](../settings/hardcoded-rust.md); `shell/HARDCODED_SETTINGS_AUDIT.md` queue | Zoom → notification durations → search limits is the audit's own order |
| **9 placeholder settings pages** | `shell/src/plugins/core/settings/SettingsStubPages.tsx` (`cp-stub:*`: sync, quick-switcher, daily-notes, file-recovery, …) | Set expectations the product doesn't meet — implement or remove |
| **Crash reporting local-only** | `nexus-panic-log` → `~/.nexus-shell/logs/panic.log` | Opt-in remote reporter would fit privacy posture |

## 4. Engineering infrastructure

CI is strong (fmt / clippy `-D warnings` / tests / pnpm / cargo-deny /
IPC-drift gates; `clippy::unwrap_used` denied on production code). Remaining:

- **E2E nearly empty:** one 84-line WebdriverIO golden-path spec
  (`shell/e2e/specs/golden-path.spec.ts`), not PR-gated, vs 263 unit-test
  files. Plugin manager, settings panel, sandbox lifecycle have zero E2E.
- **No integration-test dirs** for `nexus-terminal`, `nexus-agent`,
  `nexus-ai` (three of the largest crates); `panic-log`, `git`, `lsp`, `dap`
  below test-density median (V15).
- **Metrics have no exit path:** kernel registry computes latency histograms
  + p50/p95/p99 (`crates/nexus-kernel/src/metrics.rs`) but there is no
  Prometheus/OTLP endpoint and no shell health panel (BL-093 closure pending).
- **Log rotation** exists only for the panic log.
- **Coverage-guided fuzzing** operator-side only; a scheduled CI job for the
  four stable targets (plus the laid-down `fuzz_wasm_instantiation` shim)
  would be cheap (`crates/nexus-fuzz/src/lib.rs`).

## 5. New directions the architecture invites

Not in the code's plans, but the microkernel makes them unusually cheap:

- **Web frontend.** `nexus serve` exposes the whole kernel IPC + event bus
  over JSON-RPC with reconnect + subscription replay (`crates/nexus-remote`,
  `crates/nexus-bootstrap/src/reconnect.rs`) — a web shell is a fourth
  frontend over an existing wire. Same wire enables a Tauri 2 mobile companion.
- **Publish a forge as a static site** — the formats/export plumbing (Notion
  export) gives it a home.
- **Per-note version-history UI** — git `AutoCommitter` already runs; a
  time-travel view is pure frontend.
- **Attachment intelligence** — attachments are indexed but never parsed;
  PDF/OCR extraction into the block index would make search cover them.
- **Forge-level sync service** — `nexus sync` wraps git and the memory hub
  already does LWW sync; a first-class forge sync server is the convergence
  (the `cp-stub:sync` settings page already anticipates it).

## 6. Suggested priority order

> **Status as of 2026-07-02** — worked in order via PRs #348–#351;
> per-item state below. The unfinished slices of item 5 are the live
> backlog before item 6 completes.

| # | Item | Why first | Status |
|---|------|-----------|--------|
| 1 | Extension-API unification + conformance tests (#187) | Blocks the entire community-plugin ecosystem | ✅ #348 |
| 2 | Memory SQLite persistence (Phase 5) | Core product promise currently lost on restart | ✅ #348 |
| 3 | Hybrid search (BM25 + vector RRF) | Days of work; both engines already in one crate | ✅ #348 |
| 4 | Auto-update + macOS/Linux release pipelines | Distribution table stakes for a desktop app | ✅ #350 (pipelines + checksums; updater blocked on owner keys — steps in `RELEASE.md`) |
| 5 | Collab/hub auth + TLS | Prerequisite for any real multi-user story | 🔶 #351 landed the relay core (named `TokenSet` + attribution). **Remaining:** (a) `nexus-memory-hub` per-node tokens over the same `TokenSet` shape, (b) `nexus collab token add/remove/list` issuance verbs, (c) TLS (interim: TLS-terminating proxy, documented) |
| 6 | E2E suite + terminal/agent/ai integration tests | Regression safety before the surface grows | ⏭️ next up |
| 7 | Metrics exporter + log rotation | Data already exists; needs an exit path | ✅ #350 (exporter; rotation descoped — no file-based tracing exists) |
| 8 | Marketplace (WI-44), then i18n groundwork | Ecosystem growth once #1 lands | queued |

## Strengths to preserve (unchanged from prior audits)

Invariant enforcement (`dep_invariants.rs`, cap-matrix completeness,
bootstrap coverage, IPC strictness + drift gate), unconditional capability
checks at dispatch entry, WASM fuel/epoch/memory limits + Ed25519 signing,
OS-keyring-backed secrets, and the `unwrap_used` production lint lock.
