# Crate Inventory

> **As of:** 2026-05-17. Sourced from each crate's `Cargo.toml` and `src/lib.rs`.

All 39 workspace members in `crates/`. Excluded from this table: `shell/src-tauri/` (Tauri bridge — see [`shell.md`](shell.md)) and `packages/nexus-extension-api/` (TS package — see [`shell.md`](shell.md)).

| Crate | Kind | Purpose | IPC plugin id | Direct nexus-* deps | Notable external deps | Has CorePlugin impl? | Has settings? | Notes |
|-------|------|---------|---------------|---------------------|----------------------|----------------------|---------------|-------|
| nexus-types | lib | Shared types for kernel and plugins | — | — | serde, serde_json, serde_yml, toml, chrono, uuid | No | No | Leaf of dependency graph; ts-rs export opt-in |
| nexus-plugin-api | lib | Stable plugin contract: capabilities, IPC, events | — | — | serde, serde_json, async-trait, uuid, chrono | No | No | Kernel/plugin boundary types only |
| nexus-kernel | lib | Event bus, plugin lifecycle, capability system | — | nexus-plugin-api, nexus-types | tokio, tracing, serde_json, toml, chrono | No | KernelConfig | Core system runtime; audit_store module |
| nexus-kv | lib | SQLite KV store backends for kernel | — | nexus-kernel | rusqlite | No | No | Provides SqliteKvStore implementation |
| nexus-security | lib | Credential vault, audit logging, TLS pinning | com.nexus.security | nexus-kernel, nexus-plugins, nexus-types | keyring, rustls, rustls-pki-types, webpki-roots, sha2 | Yes | ? | CorePlugin; BL-102 TLS pinning |
| nexus-storage | lib | Forge layout, atomic writes, FTS search, watcher | com.nexus.storage | nexus-kernel, nexus-plugins, nexus-types, nexus-database, nexus-formats | rusqlite, r2d2, r2d2_sqlite, tantivy, tree-sitter, comrak, notify | Yes | StorageConfig | Largest service crate; BL-114 code-symbol index |
| nexus-plugins | lib | Plugin system: WASM sandbox, loader, hot-reload | — | nexus-plugin-api, nexus-kernel, nexus-types | wasmtime, jsonschema, semver, chacha20poly1305, ed25519-dalek, keyring, notify | No | Per-plugin settings | Plugin manifest signing (BL-099) |
| nexus-ai | lib | AI engine: provider traits, embeddings, RAG | com.nexus.ai | nexus-kernel, nexus-plugins, nexus-types, nexus-security, nexus-ai-runtime | reqwest, fastembed, dashmap, xxhash-rust | Yes | AiConfig | BL-019 local embeddings optional |
| nexus-ai-runtime | lib | Unified AI/agent event loop, worker pool | — | nexus-kernel, nexus-plugin-api, nexus-plugins, nexus-types | tokio, serde_json | No | No | BL-134 / ADR 0028; task scheduler |
| nexus-mcp | lib | MCP server + host client for forge ops | com.nexus.mcp.host | nexus-kernel, nexus-plugins | rmcp, reqwest (tokio-tungstenite via rmcp) | Yes | McpHostConfig | Dynamic tool registry (DG-39) |
| nexus-lsp | lib | LSP host: spawns servers, bridges JSON-RPC | com.nexus.lsp | nexus-kernel, nexus-plugins | tokio, toml, schemars | Yes | LspHostConfig | BL-113 / ADR 0027 contrib model |
| nexus-dap | lib | DAP host: spawns adapters, bridges protocol | com.nexus.dap | nexus-kernel, nexus-plugins | tokio, toml, schemars | Yes | DapHostConfig | Content-Length framing like LSP |
| nexus-acp | lib | ACP host + inbound server for agents | com.nexus.acp | nexus-kernel, nexus-plugins | tokio, schemars | Yes | AcpHostConfig | Newline-delimited JSON-RPC 2.0 |
| nexus-remote | lib | Remote-forge JSON-RPC server (BL-140) | — | nexus-kernel, nexus-plugin-api | tokio | No | No | Headless kernel exposed over stdio |
| nexus-cli | bin | Nexus CLI binary | — | nexus-bootstrap, nexus-collab, nexus-crdt, nexus-kernel, nexus-security, nexus-plugins, nexus-mcp, nexus-acp, nexus-remote, nexus-git, nexus-terminal, nexus-tui, nexus-templates, nexus-formats | clap, clap_complete, tokio, rustyline, ctrlc | No | reads AppConfig | Primary CLI entry point |
| nexus-tui | lib+bin | Terminal UI | — | nexus-bootstrap, nexus-kernel, nexus-types, nexus-git | ratatui, crossterm, tokio, uuid | No | No | Also invokable from CLI |
| nexus-git | lib | Git integration: status, diff, blame, log | com.nexus.git | nexus-kernel, nexus-plugins, nexus-security, nexus-types | git2, chrono, toml | Yes | No | libgit2-backed |
| nexus-formats | lib | File-format library: markdown, canvas, config | com.nexus.formats | nexus-plugins | comrak, regex-lite, csv, zip, sha2 | Yes | AppConfig + WorkspaceState + AiConfig | Pure-logic; no SQL |
| nexus-database | lib | Database support: property types, formulas, CSV | com.nexus.database | nexus-types, nexus-plugins | regex-lite, csv | Yes | No | Pure-logic; SQL queries delegated to storage |
| nexus-theme | lib | Theming engine: CSS variables, cascade, snippets | com.nexus.theme | nexus-kernel, nexus-plugins | notify, notify-debouncer-mini, schemars | Yes | Theme config | ~100 CSS variable defaults |
| nexus-bootstrap | lib | Runtime bootstrap: kernel + plugin loader assembly | — | every service crate | tokio, anyhow, tracing, rusqlite, zip | No | cap_matrix.toml | Central assembly point; sole linker of every service |
| nexus-editor | lib | Editor engine: block tree, annotations, transactions | com.nexus.editor | nexus-formats, nexus-kernel, nexus-plugins, nexus-types | comrak, sha2, chrono, uuid | Yes | No | PRD-08 §1-5 in-memory domain model |
| nexus-terminal | lib | Terminal & process manager: PTY, output capture, server-side VT grid | com.nexus.terminal | nexus-kernel, nexus-plugins, nexus-types, nexus-vt | portable-pty, regex-lite, tokio, rusqlite | Yes | ? | PRD-09 foundation; signal handling per OS; OSC 133 capture (RFC 0003) |
| nexus-rush | lib+bin | Bundled POSIX-ish shell for *sandboxed* terminal sessions (RFC 0002) | — | (leaf) | rustyline, libc | No | No | Vendored `baileyrd/rush`, refactored to embeddable lib + thin bin; system shell stays the default |
| nexus-vt | lib | Headless VT engine: parser + grid + scrollback + OSC 133 command tracking (RFC 0003) | — | (leaf) | unicode-width, unicode-segmentation | No | No | Vendored `rusty_term` core (GUI-free, no in-band channel); server-side screen model behind `nexus-terminal` |
| nexus-agent | lib | Agent system: traits, Plan/Step, executor | com.nexus.agent | nexus-kernel, nexus-plugins | tokio, rusqlite, regex-lite, futures | Yes | No | PRD-15; transcript search FTS5 |
| nexus-skills | lib | Skills subsystem: .skill.md parser + registry | com.nexus.skills | nexus-kernel, nexus-plugins | serde_yml | Yes | No | PRD-13; YAML frontmatter |
| nexus-templates | lib | Page-template subsystem: .template.md files | com.nexus.templates | nexus-plugins | chrono | Yes | No | Parameter substitution engine |
| nexus-workflow | lib | Workflow subsystem: .workflow.toml parser + cron | com.nexus.workflow | nexus-kernel, nexus-plugins, nexus-types | tokio, regex-lite, toml, futures | Yes | No | PRD-16; trigger/condition/action |
| nexus-linkpreview | lib | Link-preview subsystem: OG/Twitter-card fetch | com.nexus.linkpreview | nexus-plugins | reqwest, regex-lite | Yes | No | 5 s timeout; 512 KB max body |
| nexus-notifications | lib | Multi-channel dispatcher: desktop, Discord, Telegram, email | com.nexus.notifications | nexus-kernel, nexus-plugin-api, nexus-plugins | reqwest, lettre, tokio, notify, rusqlite | Yes | NotificationsConfig | BL-133; async SMTP via lettre |
| nexus-comments | lib | Side-margin comments: persistent threads by block_id | com.nexus.comments | nexus-plugins | uuid, chrono, regex-lite | Yes | No | BL-050; JSON sidecars per file |
| nexus-panic-log | lib | Panic hook: appends to ~/.nexus-shell/logs/panic.log | — | — | chrono, dirs | No | No | 1 MiB rotation; not published |
| nexus-crdt | lib | Operation-based CRDT for collaborative editing | — | nexus-editor, nexus-kernel | tokio, uuid | No | No | BL-074 / PRD-08 §8; RGA for text |
| nexus-fuzz | lib | Security fuzz targets (BL-103) | — | nexus-kernel, nexus-plugin-api, nexus-plugins, nexus-types | rand | No | No | Not published; stable-Rust smoke runner |
| nexus-audio | lib | Audio subsystem: STT + TTS provider traits | com.nexus.audio | nexus-kernel, nexus-plugins | reqwest, base64, whisper-rs, hound | Yes | No | BL-117; optional local-audio feature |
| nexus-collab | lib | Live-collaboration relay: WebSocket transport | com.nexus.collab | nexus-kernel, nexus-plugins | tokio-tungstenite, futures-util, uuid | Yes | No | BL-143 Phase 1; topic-agnostic relay |
| nexus-memory | lib | Native memory engine at full remind_me parity: SQLite + FTS5, hybrid-vector recall (RRF), SPO facts + entity graph, tags, ACT-R vitality, capture+decompose, consolidate, LLM-wiki, import/export, hub sync, bus capture | com.nexus.memory | nexus-plugin-api, nexus-plugins | rusqlite, r2d2, r2d2_sqlite | Yes | No | #188 — wired service plugin (21 IPC handlers); see [`memory.md`](memory.md) |
| nexus-memory-hub | bin | Standalone HTTP sync server for cross-instance memory replication (the hub `nexus-memory`'s `sync` handler pushes/pulls against) | — (deployable binary) | nexus-memory | axum/reqwest, rusqlite | No | No | Deployable sidecar; not a bootstrap plugin |
| nexus-context | lib | Typed context-assembly pipeline (budget-bounded entries) | — | nexus-memory, nexus-plugin-api | serde, serde_json | No | No | Move 6; staging library — consumes nexus-memory; awaits nexus-ai-runtime consumer wiring (#188) |
| nexus-protocol | lib | Speech-act protocol above rmcp transport | — | nexus-plugin-api | serde, serde_json | No | No | Move 7; staging library — zero in-tree consumers; awaits agent-loop adoption of typed messages (#188) |

## Summary counts

| Stat | Count |
|------|-------|
| Workspace members | 39 |
| Crates registering a CorePlugin | 24 |
| Crates exposing a runtime-mutable Config struct | 11 |
| Crates with no `nexus-*` deps (leaves) | 4 (`nexus-types`, `nexus-plugin-api`, `nexus-panic-log`, `nexus-fuzz` for deps the test cares about) |
| Crates allowed to link every service plugin | 1 (`nexus-bootstrap`) |

## Notes

- **`?` in the "Has settings?" column** means the crate has runtime state but no externally-mutable Config struct was found in the lib.rs/Cargo.toml surface; review on a per-handler basis if you need to tune behaviour.
- **The "Notable external deps" column** is curated, not exhaustive — only deps that hint at function are listed. See each crate's `Cargo.toml` for the full graph.
- **`nexus-bootstrap` legitimately depends on every service crate** because it's the sole linker per invariant 2.
- **`nexus-memory` is a service plugin at full remind_me parity** (`com.nexus.memory`, #188) — promoted from a staging library to an IPC-reachable service with 21 handlers: CRUD/list/stats, FTS5 + hybrid-vector recall (RRF), SPO facts + entity graph, tags, ACT-R vitality, import/export, cross-instance `sync` (against the standalone `nexus-memory-hub`), LLM `wiki_*` synthesis, `auto_capture`/`get_capture`/`consolidate`, and passive bus capture. Reachable from CLI, TUI, MCP, and the shell dashboard. Registered by `nexus-bootstrap` right after storage; no longer exempt in `bootstrap_coverage`. Full surface: [`memory.md`](memory.md).
- **Staging libraries (`nexus-context`, `nexus-protocol`)** — two workspace members still ship ahead of their consumers (#188). They are not `CorePlugin`s; their integration path is through a downstream consumer crate (`nexus-ai-runtime` for `nexus-context`, the agent loop for `nexus-protocol`). The bootstrap-coverage test (`crates/nexus-bootstrap/tests/bootstrap_coverage.rs`) exempts them on this basis; removing a row requires either landing the consumer or a deliberate decision to promote the library into an IPC-reachable service (an IPC handler design + cap-matrix entry — as was just done for `nexus-memory`).
