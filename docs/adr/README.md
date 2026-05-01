# Architecture Decision Records

Each ADR captures one decision: the context that forced it, the choice made,
and the consequences. ADRs are immutable once accepted — if a decision is
later revised, write a new ADR that supersedes the old one.

## Conventions

- **Numbering** is monotonic. Don't reuse numbers, even if an ADR is rejected
  or superseded.
- **Status field** appears at the top of every ADR. Valid values:
  - `Proposed` — written but not yet accepted; open for discussion.
  - `Accepted` — the decision is in force. Most ADRs sit here.
  - `Rejected` — the decision was considered and not adopted. Kept for the
    record so future readers know the option was on the table.
  - `Superseded by ADR-NNNN` — replaced; read the new one. The original
    stays in place so historical references keep resolving.
- **One file per decision.** Don't bundle multiple decisions in one ADR.
- **Filenames** use the form `NNNN-short-kebab-title.md`.

## Index

| # | Title | Status |
|---|---|---|
| [0001](0001-cargo-workspace-with-prd-crates.md) | Cargo workspace with one crate per PRD | Accepted |
| [0002](0002-hierarchical-capability-strings.md) | Hierarchical capability strings | Accepted |
| [0003](0003-storage-owns-file-watcher.md) | Storage owns the file watcher | Accepted |
| [0004](0004-crate-boundaries-and-ownership.md) | Crate boundaries and ownership map | Accepted |
| [0005](0005-single-dispatch-handler-ids.md) | Plugin calling convention: single dispatch + handler IDs | Accepted |
| [0006](0006-kv-backed-plugin-state.md) | KV-backed, plugin-managed hot-reload state | Accepted |
| [0007](0007-closed-event-enum-with-custom-variant.md) | Closed event enum with `Custom` variant | Accepted |
| [0008](0008-tech-stack-defaults.md) | Tech stack defaults | Accepted |
| [0009](0009-keyring-hard-fail-policy.md) | Keyring hard-fail policy | Accepted |
| [0010](0010-no-plugin-signing-in-m1.md) | No plugin signature verification in M1 | Accepted |
| [0011](0011-adopt-plugin-first-shell.md) | Adopt plugin-first shell, retire legacy `app/` | Accepted (executed v0.4.0) |
| [0012](0012-drop-named-layout-presets.md) | Drop named layout presets in v1 | Rejected |
| [0013](0013-menu-bar-strategy.md) | Palette-first menu-bar strategy | Accepted |
| [0014](0014-ribbon-vs-activity-bar-api-alignment.md) | Ribbon vs activity-bar API naming alignment | Accepted |
| [0015](0015-iframe-sandbox-plugin-runtime.md) | Iframe sandbox as the community-plugin runtime | Accepted |
| [0016](0016-microkernel-native-vs-wasm-plugin-split.md) | Microkernel: native vs WASM plugin split | Accepted |
| [0017](0017-block-id-stability.md) | Block-ID stability via lazy inline stamping | Accepted |
| [0018](0018-embedding-backend.md) | Local embedding backend (fastembed-rs) | Accepted |
| [0019](0019-obsidian-base-format.md) | Read-only support for Obsidian `.base` format | Accepted |
| [0020](0020-popout-window-architecture.md) | Popout window architecture (BL-029 Phase 2) | Accepted |
| [0021](0021-ipc-handler-versioning.md) | IPC handler versioning convention | Accepted |

## How ADRs relate to PRDs and BACKLOG

- **PRDs** (`../PRDs/`) describe *what* a subsystem should do. Long-lived
  product specifications.
- **ADRs** describe *how* a tricky decision was resolved. Long-lived design
  records, scoped to a single choice.
- **BACKLOG.md** (`../PRDs/BACKLOG.md`) tracks *open work*. Short-lived;
  closes against shipped or rejected items.

If a PR's motivation can't be expressed as "delivers a piece of PRD-X" or
"applies the decision in ADR-NNNN," that's worth noticing. Either the PRD
or the ADR is missing, or the work is undermotivated.

## Template

When writing a new ADR, copy this skeleton:

```markdown
# ADR-NNNN: <Decision title>

- **Status:** Proposed | Accepted | Rejected | Superseded by ADR-XXXX
- **Date:** YYYY-MM-DD
- **Deciders:** <names>
- **Context for:** <PRD-NN, work item, incident, etc.>

## Context

What forced this decision. Constraints, prior art, what's at stake.

## Decision

The choice made, in plain language.

## Consequences

What this enables, what it forecloses, what new costs it imposes.

## Alternatives considered

The other options that were on the table and why they lost.
```
