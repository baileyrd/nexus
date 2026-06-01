# Nexus 0.1.2 — Architecture & Settings Audit

> **As of:** 2026-05-17. Workspace at commit `9382b13a` (HEAD of `main`).
> **Source of truth:** the code under `crates/`, `shell/`, `packages/` — **not** the archived docs under [`../archive/pre-0.1.2/`](../archive/pre-0.1.2/).

## Why this exists

The pre-0.1.2 doc set was a 9 MB curated mix of architecture, PRDs, ADRs, audits, plans, and reference material accreted over the project's life. By 0.1.2 the code had moved past parts of it. This directory is a fresh, code-derived audit:

1. **Inventory.** Every crate, plugin, IPC handler, capability, settings surface, and known hardcoded value documented in one place.
2. **No hidden settings.** Every config knob — file, env var, default — listed. Every hardcoded value not yet promoted to a setting flagged with a remediation suggestion.
3. **No layer skipped.** Microkernel, service crates, frontends (CLI / TUI / MCP / Tauri shell), shell plugins, TypeScript SDK, Tauri bridge — all covered.

## Map

```
docs/0.1.2/
├── README.md                  ← you are here
├── architecture.md            ← workspace shape, invariants (from dep test), boot order
├── crates.md                  ← table of all 35 Rust crates
├── shell.md                   ← shell/ + shell/src-tauri/ + packages/nexus-extension-api/
├── architecture-adherence.md  ← audit of whether code lives up to microkernel + shell invariants
├── implementation-plan.md     ← phased remediation plan for every audit finding (39 items)
├── ipc-handlers.md            ← every IPC handler (~280), grouped by plugin
├── capabilities.md            ← every security capability (30), every use site
├── amplifier-laundering.md    ← #189 threat model + accepted residual risk for amplifier plugins
├── application-capabilities.md ← what Nexus does, by feature domain
├── plugin-capabilities.md     ← per-plugin: what each of the 96 plugins provides
├── settings/
│   ├── README.md              ← config-surface index
│   ├── forge-config.md        ← .forge/config.toml schema
│   ├── plugin-manifests.md    ← plugin.toml fields
│   ├── mcp-config.md          ← .forge/mcp.toml (external MCP servers)
│   ├── env-vars.md            ← every NEXUS_*, RUST_LOG, etc. read at runtime
│   ├── hardcoded-rust.md      ← Rust crates: hardcoded values + suggested settings keys
│   ├── hardcoded-shell.md     ← shell/packages: hardcoded values (carryover from shell/HARDCODED_SETTINGS_AUDIT.md, refreshed)
│   └── plugin-manifest-defaults.md ← values baked into plugin manifests + scaffolds (closes the plugin-internal gap)
├── plugins/
│   ├── core.md                ← in-tree CorePlugin impls
│   └── community.md           ← WASM/script community plugin contract
└── reference/
    ├── audit-flags.md         ← every AUDIT: row in cap_matrix.toml (candidates for cap elevation)
    ├── todos.md               ← every TODO/FIXME/coming-soon/stub marker, categorized
    └── glossary.md
```

## Reading order

- **Architecture orientation:** start at [`architecture.md`](architecture.md), then [`crates.md`](crates.md).
- **Building a plugin:** [`plugins/community.md`](plugins/community.md) → [`capabilities.md`](capabilities.md) → [`ipc-handlers.md`](ipc-handlers.md).
- **Adding a setting (the right way):** [`settings/README.md`](settings/README.md) → [`settings/hardcoded-rust.md`](settings/hardcoded-rust.md) / [`settings/hardcoded-shell.md`](settings/hardcoded-shell.md) for already-flagged candidates.
- **Security review:** [`capabilities.md`](capabilities.md) → [`amplifier-laundering.md`](amplifier-laundering.md) → [`reference/audit-flags.md`](reference/audit-flags.md).

## What's not here yet

- **Per-crate deep dives.** This pass produces one consolidated [`crates.md`](crates.md) table rather than 35 individual files. If a specific crate needs a full sub-page, file it under `crates/<name>.md` in a follow-up.
- **Diagrams.** The pre-0.1.2 C4 diagrams under [`../archive/pre-0.1.2/architecture/C4.md`](../archive/pre-0.1.2/architecture/C4.md) remain a useful sketch even though the prose is archived.
- **PRD-style narratives.** PRDs in [`../archive/pre-0.1.2/PRDs/`](../archive/pre-0.1.2/PRDs/) include shipped-vs-gap detail per subsystem. They're stale but not wrong; cross-reference when the audit feels too terse.
