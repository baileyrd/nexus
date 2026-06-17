# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow the workspace version in `Cargo.toml`. Started 2026-06-10 (V18,
`docs/0.1.2/audits/repo-review-2026-06-10.md`) — history before that date
lives in the git log and in `docs/0.1.2/audits/`.

## [Unreleased]

### Added
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
