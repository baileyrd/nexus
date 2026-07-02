# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow the workspace version in `Cargo.toml`. Started 2026-06-10 (V18,
`docs/0.1.2/audits/repo-review-2026-06-10.md`) — history before that date
lives in the git log and in `docs/0.1.2/audits/`.

## [Unreleased]

### Added
- **Per-user relay credentials** (`nexus-collab`, gap-analysis §1.4) —
  new `TokenSet`: named tokens with constant-time, full-scan
  verification that returns *which* credential authenticated, so joins
  are attributable in the relay log and one user's token can be
  rotated/revoked without re-keying every peer.
  `RelayServer::new_with_tokens(TokenSet)` alongside the unchanged
  Phase-1 `new(Token)` (now a one-entry set named `default`). TLS
  remains deferred — front the relay with a TLS-terminating proxy.
- **Prometheus exit path for kernel metrics** (BL-093 closure) —
  `MetricsSnapshot::to_prometheus_text()` renders the registry in the
  text exposition format (counters, the queue-depth gauge, and
  p50/p95/p99 summaries in seconds; sorted/deterministic output;
  label escaping per spec), exposed as
  `com.nexus.security::metrics_prometheus` (handler id 10, unrestricted
  read-only) so any frontend or a scrape sidecar can reach it via
  `ipc_call`.
- **Linux + macOS release pipelines** — `release-linux.yml` (`.deb` /
  `.rpm` / `.AppImage`) and `release-macos.yml` (aarch64 + x86_64
  `.dmg`s) mirror the Windows workflow: tag-triggered, artifacts +
  `SHA256SUMS-<platform>-<tag>.txt` checksums attached to one shared
  draft Release, `workflow_dispatch` dry-runs. The Windows workflow
  gains the same checksum sidecar. Auto-updater key-handling steps are
  documented in `RELEASE.md` (owner-generated secrets; no updater code
  yet).
- **Hybrid forge search** (`com.nexus.storage::hybrid_search`, handler id
  76) — reciprocal-rank fusion (`k=60`, matching `nexus-memory`'s recall)
  of the Tantivy BM25 arm and the vector-store cosine arm, with 4×
  per-arm oversampling so blocks outside one arm's window can still win
  on fused rank. The caller supplies query text + embedding (storage
  does not embed — D-1). Reachable end-to-end via
  `com.nexus.ai::semantic_search` with `"hybrid": true`; either arm
  degrades gracefully when empty.
- **Cognitive-store persistence (memory Phase 5)** — `MemoryStore` (the
  episodic / semantic / procedural facade in `nexus-memory`) gains optional
  SQLite write-through: `MemoryStore::open(forge_root)` loads prior state
  from `.forge/memory/memory.db` (new `episodic_log` / `semantic_facts` /
  `procedural_skills` tables alongside the plugin's `memories` table) and
  persists every subsequent mutation, so agent memory survives process
  restarts. `MemoryStore::new()` keeps the original in-memory semantics; the
  API surface is unchanged, exactly as the Phase-1 docs promised.
- **Common plugin contract + conformance gates** (#187 / V9) —
  `@nexus/extension-api`'s `NexusPluginContext` is re-derived as the subset
  both live runtimes satisfy (with `MaybePromise` bridging sync/async);
  compile-only conformance tests lock the in-process `PluginAPI` (which now
  carries a host-asserted `pluginId`) and the sandbox
  `SandboxedPluginContext` to it. `ScriptPlugin` is deprecated (removal
  0.2.0) and the package re-cut `1.0.0` → `0.1.0`.
- **Native memory engine at full `remind_me` parity** (`com.nexus.memory`,
  #188) — promoted from a staging library to a wired service plugin with 21
  IPC handlers: CRUD/list/stats, FTS5 + hybrid-vector recall (RRF), SPO facts
  + entity graph, tags, ACT-R vitality, `auto_capture`/`get_capture`/
  `consolidate`, LLM `wiki_*` synthesis, `export`, and cross-instance `sync`
  against the new standalone `nexus-memory-hub` server. Plus passive event-bus
  capture. Reachable from CLI, TUI, MCP (`nexus_memory_*`), and the shell
  Memory Dashboard. See [`docs/0.1.2/memory.md`](docs/0.1.2/memory.md).
- **OS process sandbox** (Phase 4 F1/F2) — a Codex-style `SandboxPolicy`
  (`read-only` / `workspace-write` / `danger-full-access`) in `nexus-types`,
  enforced on Linux via Landlock (filesystem) + seccomp-bpf (network
  off-by-default), composed by `confine_current_thread`, and applied to
  spawned children through the single-threaded `nexus-sandbox` helper.
  Permissioned download broker for approved egress under a network-off policy.
  Configured via `.forge/sandbox.toml`, reachable over IPC
  (`com.nexus.security::sandbox_policy` / `download`), MCP, the `nexus sandbox`
  CLI, and a shell panel; opt-in per terminal session. See
  [`docs/0.1.2/os-sandbox.md`](docs/0.1.2/os-sandbox.md).
- `security.audit.read` capability gating `query_audit_log` (previously
  unrestricted; cross-plugin telemetry is reconnaissance surface).
- `cargo-deny` supply-chain gate in CI (`deny.toml`): advisories,
  license allowlist, duplicate bans, registry provenance.
- One-shot operator warning when a remote AI provider is configured with
  credentials but without TLS pinning.
- Tauri command-boundary guard now runs on every PR
  (`crates/nexus-bootstrap/tests/tauri_command_boundary.rs`).
- 22 characterization tests over linkpreview's OG/Twitter-card parsing.

### Changed
- Outbound HTTP clients carry timeouts: 10s connect + 300s read backstop
  for AI providers, 10s/30s for notification webhooks.
- Storage knowledge-graph reads recover from lock poison instead of
  aborting the process (`panic=abort`) — #199 tier-1 policy.
- Linkpreview pins each fetch hop to its SSRF-validated IP, closing the
  DNS-rebinding TOCTOU.
- `scripts/` reduced to the five portable value-add helpers; the
  single-machine cargo wrappers were removed.
- Shell chrome no longer imports workspace-plugin internals: new
  `WorkspaceHostSurface` seam (plugin registers at activation), with the
  host→plugin import direction now test-enforced.
- Shell test stubs are structurally type-checked (`stubPluginAPI`);
  zero `as any` remain in shell test files.
- Kernel `context_impl.rs` split into focused modules (pure code motion).

### Security
- See Added/Changed: audit-log read gating, supply-chain CI gate,
  DNS-rebinding fix, HTTP timeouts. Advisory RUSTSEC-2025-0068
  (`serde_yml`, unsound/unmaintained) is acknowledged and tracked in #248.
