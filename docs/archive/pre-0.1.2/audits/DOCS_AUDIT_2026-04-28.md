# Docs Implementation Audit — 2026-04-28

> Audit of every PRD, ADR, and architecture doc against `crates/**` and `shell/src/**`.
> Six parallel `Explore` agents performed the initial pass; findings were cross-checked against actual code before being recorded here.
>
> **Companion docs:**
> - Per-PRD status tiers: [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md)
> - Open work queue: [BACKLOG.md](BACKLOG.md) (new BL-010..BL-029 items added in this pass)
> - Open issues by ID: [../OPEN-ITEMS.md](../OPEN-ITEMS.md)
> - Formal-release scope: [../REQUIRED-FOR-FORMAL-RELEASE.md](../REQUIRED-FOR-FORMAL-RELEASE.md)

## TL;DR

- **PRDs 01–04a, 06, 07** (Phase 1 + 2 foundation): ✅ Complete. No new gaps. Two follow-ups for PRD-07 (touch gestures, native chrome) and one for PRD-04a (test mocks) filed as small backlog items.
- **PRD-05 (CLI):** 🟢 — two real gaps: `nexus ai chat` (interactive REPL, §3.5.2) and `nexus ai complete` (§3.5.3) are spec'd but not implemented; CLI surface is `ask | embed | status | config` only. Filed BL-010 / BL-011.
- **PRD-08 (Editor):** 🟢 — only outstanding spec gap is database query blocks `[[{db:query}]]` (§8.1). Already noted in IMPLEMENTATION_STATUS.md gaps; promoted to BL-012.
- **PRD-09 (Terminal):** 🟢 — §11 plugin-IPC event subscription is library-only (in-process `mpsc`); promoted to BL-013. §10.1 ad-hoc CRUD over IPC and §19.3 FTS5 scrollback already noted.
- **PRD-10 (Database):** 🟢 — `nexus db` CLI never specified (§11 referenced but absent), soft-delete trash UI mentioned in BACKLOG.md narrative but never given a BL ID. Filed BL-014 / BL-015. Cross-database rollup/lookup resolver **does** exist at `crates/nexus-storage/src/bases/relation.rs:73` (`compute_rollup`) — agent flagged as missing; verified shipped.
- **PRD-11 (Git):** 🟢 — IMPLEMENTATION_STATUS.md detail entry (line 120) is stale and contradicts its own summary. The detail says "no `nexus git` CLI, no GitWorker, no git events to bus, no core plugin"; all four exist:
  - `nexus git` CLI: 16 subcommands at `crates/nexus-cli/src/commands/git.rs` (378 LoC).
  - `GitWorker`: `crates/nexus-git/src/worker.rs:80`.
  - `com.nexus.git` core plugin: 10 IPC handlers at `crates/nexus-git/src/core_plugin.rs:32-54`.
  - Git events: `com.nexus.git.{state,branch_changed,commit,dirty_changed}` published from `core_plugin.rs:316-354`.
  Outstanding gap: merge/rebase conflict-resolution UI (large; deferred). PRD-11 detail rewritten in this pass.
- **PRD-12 (AI):** 🟢 — 4 real gaps: tool registration for LLM function-calling (§8.1), PII/secret egress filter (§15.1), token budget enforcement (§12), local embeddings (§9.1). Filed BL-016..BL-019.
- **PRD-13 (Skills):** 🟠 — three confirmed gaps: REGISTRY.json persistence (lib.rs:25 acknowledges this), `depends_on` composition resolver (§5), in-app editor UI (§16). Filed BL-020..BL-022.
- **PRD-14 (MCP):** 🟢 — five gaps: WebSocket / HTTP+SSE transport (§4.2.2-3), reconnect+pool (§4.2.4), authentication (§8), resource enumeration (§7). Filed BL-023..BL-026.
- **PRD-15 (Agent):** 🟠 → effectively 🟢 — IMPLEMENTATION_STATUS line 40 already documents 7 handlers, archetypes, MCP discovery, history, streaming, stepwise approval, history panel. Tier marker should be bumped. Real gaps: multi-agent orchestration (§10), SQLite MemoryStore (§5), reactive observation rules (§6.3), debugger/replayer (§12). Filed BL-027 (orchestration only — others deferred per "Future directions").
- **PRD-16 (Workflow):** 🟠 — webhook / git_event / mcp_event triggers, parallel scheduling, retry/backoff, AI step types, templates library are all unbuilt. Already enumerated in IMPLEMENTATION_STATUS gaps; promoted to BL-028.
- **PRD-17 (Cross-Platform):** 🟢 desktop-only (correct). Web + mobile + multi-window + native chrome are explicitly deferred. Filed BL-029 (multi-window) since detachable panels are spec'd in §6 but not in REQUIRED-FOR-FORMAL-RELEASE.md.
- **ADRs:** 15 of 16 fully honoured. ADR-0009 (keyring hard-fail) escape hatch `NEXUS_NO_KEYRING=1` mentioned in the ADR but bootstrap enforcement is unverified — added as a check-in note (no BL ID, low risk). ADR-0013 (macOS menu bar plugin promised in Phase 4) — Phase 4 closed 2026-04-24 without shipping it; the v1 palette-only stance still holds, so this is correct as deferred.
- **Other docs:** ARCHITECTURE.md, OPEN-ITEMS.md, leaf-architecture.md, ipc-schemas.md, legacy-shell-retirement.md all current. `editor-transaction-architecture.md` and `notion-block-ux-plan.md` align with the PRD-08 amendment (commit 6f3b36d). No stale commitments to track.

## False positives the audit caught

These were flagged by the parallel agents but **disproved on cross-check**. Listed here so the next reviewer doesn't refile them.

| Claim | Reality |
|-------|---------|
| "PRD-03 fuzzy wikilink resolution missing" | BL-004 closed 2026-04-18; 3-tier cascade lives at `crates/nexus-storage/src/index.rs:1119`. |
| "PRD-11 `nexus git` CLI absent" | 378-LoC CLI with 16 subcommands at `crates/nexus-cli/src/commands/git.rs:14-322`. |
| "PRD-11 no GitWorker" | `crates/nexus-git/src/worker.rs:80` (`GitWorker` + `GitWorkerHandle`). |
| "PRD-11 no git core plugin" | `com.nexus.git` registered at bootstrap with 10 handlers (`core_plugin.rs:32-54`). |
| "PRD-11 no git events on bus" | 4 event types emitted from `core_plugin.rs:316-354`. |
| "PRD-10 cross-database rollup not wired" | `compute_rollup` at `crates/nexus-storage/src/bases/relation.rs:73` (5 aggregations + tests). |
| "PRD-13 `nexus skill render` missing" | Handler id 6 + `nexus skill render` CLI shipped this session per IMPL_STATUS line 140. |
| "PRD-15 only 🟠 scaffold shipped" | 7 handlers + UI panel + history + streaming events all shipped (IMPL_STATUS line 40). Tier marker stale. |

## Documentation bugs to fix in this pass

1. **IMPLEMENTATION_STATUS.md PRD-11 detail (line 118-121)** — gaps section contradicts the summary table and the actual code. Rewrite to list only the real outstanding gaps (merge/rebase conflict UI). **Fixed in this PR.**
2. **IMPLEMENTATION_STATUS.md PRD-15 status tier** — marked 🟠 in section header (line 155) but everything in §15 is shipped. Summary row line 40 says "Library + plugin + 7 handlers + archetypes + MCP discovery + history + streaming + Chat stepwise approval + AgentHistoryPanel". Bump to 🟢. **Fixed in this PR.**

## New backlog items added in this pass

See [BACKLOG.md](BACKLOG.md) — entries BL-010 through BL-029 under the new "## Audit findings (2026-04-28)" section. Effort estimates and target crates included.
