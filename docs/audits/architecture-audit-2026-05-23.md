# Architecture Audit: Nexus
**Date:** 2026-05-23
**Author:** JARVIS
**Scope:** All workspace crates, shell, and plugin ecosystem
**Commit SHA:** (current)

---

## 1. Structural Overview

- **39 workspace crates** (Rust), **1 TS package** (`nexus-extension-api`), **4 plugins** (2 first-party, 2 sample), **Tauri 2 desktop shell** (`shell/` excluded from Cargo workspace).
- **1,825 total commits** across **155 branches**, single tag `v0.1.0`.
- **Active sprint phase** — recent commits from April/May 2026 focus on IPC cancellation (Track A), shell-state race fixes, capability aggregation, and CRDT/colab work.
- **Workspace version:** `0.1.0` — still pre-1.0.

### Workspace Members (39)

| Crate | Scope |
|---|---|
| `nexus-acp` | Agent Communication Protocol |
| `nexus-agent` | Agent service |
| `nexus-ai` | Provider traits (Claude/ OpenAI/ Ollama/ llama.cpp), embeddings, RAG |
| `nexus-ai-runtime` | AI runtime executor |
| `nexus-audio` | Audio service |
| `nexus-bootstrap` | Runtime assembly: **27 service crates**, the assembly-line `main()` |
| `nexus-cli` | `nexus` binary — headless CLI |
| `nexus-collab` | Live collaboration WebSocket relay server |
| `nexus-comments` | Comments service |
| `nexus-crdt` | Operation-based CRDT layer (BL-074, PRD-08\8) — RGA text sync |
| `nexus-dap` | Debug Adapter Protocol |
| `nexus-database` | Database service |
| `nexus-editor` | Editor service |
| `nexus-formats` | Format handling |
| `nexus-fuzz` | Fuzzing harness (with corpus) |
| `nexus-git` | Git service |
| `nexus-kernel` | Event bus, plugin lifecycle, capability enforcement, IPC dispatcher |
| `nexus-kv` | Key-value service |
| `nexus-linkpreview` | Link preview service |
| `nexus-lsp` | Language Server Protocol |
| `nexus-mcp` | MCP server library — 15 nexus\_* tools for forge operations |
| `nexus-notifications` | Notifications service |
| `nexus-panic-log` | Panic logging |
| `nexus-plugin-api` | Plugin SDK surface |
| `nexus-plugins` | WASM sandbox (wasmtime), plugin manifests, hot-reload |
| `nexus-remote` | Remote service |
| `nexus-security` | Security: OS keyring, audit logging, path validation |
| `nexus-skills` | Skills service (with builtins) |
| `nexus-storage` | File watcher, SQLite/FTS index, knowledge graph |
| `nexus-templates` | Templates service |
| `nexus-terminal` | Terminal/PTY service |
| `nexus-theme` | Theming engine: CSS variables, theme packages, layout |
| `nexus-tui` | `nexus-tui` binary — ratatui interface |
| `nexus-types` | Shared type definitions (leaf) |
| `nexus-workflow` | Workflow service |
| `nexus-extension-api` | TS package for shell extensions |

### Shell (excluded from workspace)

| Directory | Contents |
|---|---|
| `shell/` | Tauri 2 desktop shell, plugin-first |
| `shell/src-tauri/` | `nexus-shell` crate |

### Plugin Ecosystem (4)

| Name | Type |
|---|---|
| `first-party-acp-echo` | First-party ACP echo service |
| `first-party-dap-python` | First-party Python debugger bridge |
| `hello-js` | Sample/dummy |
| `hello-nexus` | Sample/dummy |

---

## 2. Dependency Graph Analysis

### Critical Mass: `nexus-bootstrap`

`nexus-bootstrap` imports **27 service crates**. This is both a signal of healthy composition and a warning: every service crate change recompiles the entire bootstrap surface.

### Layer Discipline

```
nexus-types (leaf types)
    ↓
nexus-plugin-api (plugin SDK)
    ↓
nexus-kernel (event bus + IPC + capabilities)
    ↑
nexus-plugins (WASM sandbox)
    ↑
service crates (storage, ai, editor, git, ... each depends on kernel + plugins)
    ↑
nexus-cli / nexus-tui / nexus-shell (binaries composing the services)
```

**`nexus-kernel` → `nexus-plugin-api` → `nexus-plugins` → `nexus-kernel`** forms the microkernel core loop. This is the pattern itself, but both `kernel` and `plugins` pull heavily on `plugin-api` and `types`, making them very hard to compile independently.

**No cycles** detected at depth 3. `nexus-types` and `nexus-plugin-api` form a clean base layer.

### Notable internal dependency chains

| Crate | Dependencies | Notes |
|---|---|---|
| `nexus-bootstrap` | 27 crates | Assembly-line; must import everything |
| `nexus-cli` | 20 crates | Heavy linker — binary pulls many services |
| `nexus-ai` | 5 crates | AI provider layer touches storage, security |
| `nexus-storage` | 4 crates | Core data layer: kernel, plugins, types, plus database & formats |
| `nexus-tui` | 4 crates | Pulls `nexus-bootstrap` directly — notable; usually binaries compose, don't inherit |
| `nexus-crdt` | 4 crates | CRDT layer depends on `nexus-editor::Operation` — correct |
| `nexus-collab` | 2 crates | Minimal: just kernel + plugins |
| `nexus-security` | 3 crates | Minimal: kernel, plugins, types |

---

## 3. Crate Health

### Crates with NO tests (15 of 39)

These crates lack a `tests/` directory:

| Crate | Correctness-Sensitive? |
|---|:---:|
| `nexus-crdt` | **HIGH** — RGA merge convergence |
| `nexus-collab` | **HIGH** — WS protocol, presence, reconnect |
| `nexus-lsp` | **HIGH** — protocol compatibility |
| `nexus-mcp` | **HIGH** — external protocol correctness |
| `nexus-dap` | **HIGH** — debugger protocol |
| `nexus-acp` | **HIGH** — agent protocol |
| `nexus-agent` | **MED** — agent execution logic |
| `nexus-ai` | **MED** — provider routing |
| `nexus-ai-runtime` | **MED** — cancellation logic |
| `nexus-security` | **HIGH** — path validation, TLS, auth |
| `nexus-remote` | **MED** |
| `nexus-kv` | **LOW** |
| `nexus-comments` | **LOW** |
| `nexus-skills` | **LOW** |
| `nexus-templates` | **LOW** |

### Crates with integration tests (6)

`nexus-ai`, `nexus-bootstrap`, `nexus-cli`, `nexus-dap`, `nexus-database`, `nexus-editor`, `nexus-formats`, `nexus-fuzz`, `nexus-git`, `nexus-kernel`.

### Crates with benches (3)

- `nexus-kernel` — IPC dispatch
- `nexus-plugins` — WASM sandbox benchmarking
- `nexus-terminal` — PTY I/O benchmarking

---

## 4. ADR / Architecture Documentation

### Status: **MISSING**

The AGENTS.md references `ADR 0011` and `ADR 0026`, but **`docs/adr/` does not exist**. ADRs are scattered across:

- `docs/archive/pre-0.1.2/audits/architecture-audit-2026-05-01.md`
- `docs/archive/pre-0.1.2/adr/0020-popout-window-architecture.md`
- `docs/archive/pre-0.1.2/architecture/editor-transaction-architecture.md`
- `docs/archive/pre-0.1.2/shell/architecture.md`

**ADR 0026** lives only in `nexus-crdt/` module docs (Phase 3/4 deferred features).

### ADR Gap Analysis

| Missing ADR | Referenced In |
|---|---|
| ADR 0011 — Adopt Plugin-First Shell | README.md |
| ADR 0026 — CRDT sync architecture | AGENTS.md, `nexus-crdt::` docs |
| ADR on IPC cancellation (Track A) | Commit messages |
| ADR on security model (BL-099, BL-101, BL-102) | Cargo.toml comments |
| ADR on shell-state serialization | Commit `1f3a0b93` |
| ADR on capability system | AGENTS.md, `nexus-security::` code |
| ADR on collaboration transport (BL-143) | `nexus-collab::` docs |

### README Architecture Section

**Outdated.** README says "Cargo workspace has **24** members". Workspace has **39**. No service crates are listed in the architecture table. The parsing returned zero crate names, indicating the table structure has also drifted or uses non-standard formatting.

---

## 5. Security Architecture

### `nexus-security` — Surface

| Export | Purpose |
|---|---|
| `SecurityCorePlugin` | Core plugin registration |
| `CredentialVault` | OS keyring access |
| `ForgePathValidator` | Safe path validation |
| `RiskLevel` enum + `risk_level()` | Risk classification |
| `tls::` module | TLS configuration |
| `tls_pins::` module | Root cert pinning |
| `ipc::` module | Inter-process security |

### Security Model (documented in Cargo.toml comments)

| Feature | Mechanism | Tracker |
|---|---|---|
| Manifest signing | `ed25519-dalek` | BL-099 |
| Grant cap at-rest encryption | `chacha20poly1305` for `granted_caps.json` | BL-101 |
| TLS pinning | `rustls` + `webpki-roots` — no native-tls / openssl | BL-102 |
| Audit logging | `nexus_kernel::audit` — unified event logging | ongoing |

### TLS Configuration

- All crates using TLS (`rustls`, `rustls-pki-types`, `webpki-roots`) pinned to versions matching `reqwest 0.12`
- `lettre` SMTP with `rustls-tls`, no native-tls
- `wasmtime` sandbox isolation with WASI capability

### Security Concerns

1. **`nexus-collab` WebSocket relay: bare `ws://` only.** TLS is deferred. This is noted in Cargo.toml but needs tracking.
2. **Agent codebases don't have an ADR on runtime sandboxing.** WASI confinement exists but no documented threat model.

---

## 6. Shell Architecture

### Structure

| Directory | Contents |
|---|---|
| `shell/HARDCODED_SETTINGS_AUDIT.md` | Proactive settings audit |
| `shell/README.md` | Shell docs |
| `shell/src-tauri/` | `nexus-shell` Tauri 2 crate |

### Key Features

| Feature | Status |
|---|---|
| Plugin-first shell | Implemented (ADR 0011) |
| Tauri 2 integration | Implemented |
| Per-window cancel | Implemented |
| Shell-state serialisation | Implemented (multi-window race fix) |
| Catalog ↔ disk consistency guard | Implemented (test `ef1a163e`) |
| Legacy tri-pane removal | Removed 2026-04 |

### Shell-Outside-Workspace Issues

1. **No `cargo check` integration** — shell changes don't validate main workspace
2. **Version drift** — shell crate version is independent of workspace `0.1.0`
3. **Type contract between `nexus-extension-api` and shell** — monitored but no compile-time enforcement

---

## 7. CI / Infrastructure

### GitHub Workflows (2)

| File | Purpose |
|---|---|
| `ipc-drift-check.yml` | IPC interface drift detection |
| `release-windows.yml` | Windows release CI |

**Gap:** No Linux/macOS build CI found. No test suite CI.

### Script Catalog (30+)

Category | Scripts
---|---
Checks | `check_all.sh`, `check_ai.sh`, `check_cli.sh`, `check_git.sh`, `check_mcp.sh`, `check_plugins.sh`, `check_term.sh`, `check_token_usage.sh`
Tests | `test_all.sh`, `test_agent.sh`, `test_ai.sh`, `test_api.sh`, `test_cli.sh`, `test_db.sh`, `test_git.sh`, `test_mcp.sh`, `test_skills.sh`, `test_term.sh`, `test_ts.sh`, `test_types.sh`, `test_workflow.sh`
Benchmarks | `bench_build.sh`, `bench_check.sh`, `bench_run.sh`, `bench_term.sh`
Git ops | `git_check.sh`, `git_commit.sh`, `git_do_commit.sh`, `git_restore.sh`, `git_status.sh`
Seeding | `seed_fixtures.sh`, `seed_notes.sh`
Migration | `migrate-shell-state.ts`

---

## 8. Notable Implementation Decisions

### Done Well

1. **Cooperative IPC cancellation** (Track A) — `CancellationToken` for IPC, `CancelGate` in ai-runtime, channel back-pressure (drop-on-full) across LSP/DAP/ACP.
2. **CRDT/colab stack** — `nexus-crdt` (op-log + RGA) + `nexus-collab` (WebSocket relay + presence) is a clean separation of concerns.
3. **Security model** is explicit, not implicit. TLS pinning, path validation, capability-based access, encrypted storage, and audit logging are all wired.
4. **Build profiles** are well-tuned — release with `opt-level=z` + `fat LTO` + `strip=symbols`, plus a `release-fast` profile as an escape hatch.

---

## 9. Issues & Recommendations

### High Severity

| Issue | Details |
|---|---|
| **ADR system missing** | Architecture decisions live in git log. No dedicated `docs/adr/`. ADRs 0011, 0026 exist only as references. Future contributors have no decision trail. |
| **No CI for Linux/macOS** | Only `release-windows.yml` and `ipc-drift-check.yml`. No test suite, no build validation. |

### Medium Severity

| Issue | Details |
|---|---|
| **README architecture section out of date** | Says "24 members", workspace has 39. No service crates in table. |
| **15/39 crates have no tests** | Especially `nexus-mcp`, `nexus-crdt`, `nexus-collab`, `nexus-lsp`, `nexus-dap`, `nexus-security`. |
| **Shell outside workspace** | No cargo check integration. Version drift possible. |
| **155 branches, no staging strategy** | Many `claude/` and `bl-*/` branches. Some are stale agent explorations. |

### Low Severity

| Issue | Details |
|---|---|
| **Only 2 non-sample plugins** | Plugin ecosystem is the core value proposition — but still early. |
| **Cargo check timeout** | Workspace is large; `cargo check --workspace` timed out at 120s. |
| **nexus-collab bare `ws://`** | TLS is deferred but needs tracking. |

---

## 10. Branch Inventory (Selected)

| Branch | Purpose |
|---|---|
| `bl-081-dap-debugger` | DAP debugger work |
| `bl-134-ai-runtime` | AI runtime updates |
| `bl-144-145-acp` | ACP protocol |
| `core/a1-catalog-disk-reconcile` | Catalog reconciliation |
| `core/a5-workflow-cap-aggregation` | Workflow capability aggregation |
| `core/a6-invoke-sweep` | Invoke sweep |
| `core/add-claude-md` | Config addition |
| **Note:** Many more `claude/` branches appear to be agent-driven explorations, not tracked PRs |

---

## 11. Action Items

### Immediate

1. **Create `docs/adr/`** and migrate existing ADRs from `docs/archive/` and commit messages
2. **Update README architecture table** with all 39 crates
3. **Add CI pipelines** for Linux/macOS testing

### Near-term

4. **Add tests to critical crates:** `nexus-security`, `nexus-crdt`, `nexus-collab`, `nexus-mcp`, `nexus-lsp`, `nexus-dap`, `nexus-acp`
5. **Triage old branches** — merge or prune
6. **Add `TLS` backlog ticket for `nexus-collab` relay**

### Long-term

7. **Consider moving shell into workspace** (or document why exclusion is intentional)
8. **Plugin onboarding** — increase ecosystem beyond first-party
9. **Document the capability system** as a proper ADR 