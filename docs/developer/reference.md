# Reference

Pointers to the authoritative sources. The pages in this developer
hub explain *how* and *why*; the files below are *what*.

## API surface

| Source | What it covers |
|---|---|
| [`shell/docs/plugin-api.md`](../../shell/docs/plugin-api.md) | Full `@nexus/extension-api` surface: commands, views, events, config, statusBar, IPC. Authoritative for TypeScript/JS plugin authors. |
| [`packages/nexus-extension-api/src/`](../../packages/nexus-extension-api/src/) | The TypeScript source of truth. `index.ts` for the public surface; `generated/` for IPC types and schemas. |
| [`crates/nexus-plugin-api/src/`](../../crates/nexus-plugin-api/src/) | The Rust public API for core plugin authors (`CorePlugin` trait, `PluginContext`, error types). |

## IPC

| Source | What it covers |
|---|---|
| [`docs/ipc-schemas.md`](../ipc-schemas.md) | IPC schema generation policy and drift-check workflow. |
| [`packages/nexus-extension-api/src/generated/ipc/`](../../packages/nexus-extension-api/src/generated/ipc/) | Generated TypeScript types for every cross-boundary IPC payload. |
| [`crates/nexus-bootstrap/schemas/ipc/`](../../crates/nexus-bootstrap/schemas/ipc/) | JSON Schema files for the same payloads. |
| [`scripts/check_ipc_drift.sh`](../../scripts/check_ipc_drift.sh) | Regenerate bindings; CI fails on diff. |

## Capabilities

| Source | What it covers |
|---|---|
| [`crates/nexus-plugin-api/src/capability.rs`](../../crates/nexus-plugin-api/src/capability.rs) | Canonical `Capability` enum: 14 variants, manifest strings, HIGH-risk classification. |
| [`docs/adr/0002-hierarchical-capability-strings.md`](../adr/0002-hierarchical-capability-strings.md) | Design rationale. |
| [Capabilities reference](plugins/capabilities.md) | Plugin-author view. |

## Manifest

| Source | What it covers |
|---|---|
| [Manifest spec](plugins/manifest.md) | Field reference and validation rules. |
| [`shell/src/plugins/community/hello-world/plugin.json`](../../shell/src/plugins/community/hello-world/plugin.json) | Live example: minimal community manifest. |
| [`shell/src/plugins/community/mermaid/plugin.json`](../../shell/src/plugins/community/mermaid/plugin.json) | Live example: a richer manifest with contributions. |
| [`crates/nexus-plugins/templates/script/plugin.json`](../../crates/nexus-plugins/templates/script/plugin.json) | The scaffold's starting manifest. |

## Architecture

| Source | What it covers |
|---|---|
| [`docs/architecture/C4.md`](../architecture/C4.md) | C4 model — system context → containers → components → code. |
| [`docs/architecture/invariants.md`](../architecture/invariants.md) | The four load-bearing rules and how they're enforced. |
| [`shell/docs/architecture.md`](../../shell/docs/architecture.md) | Shell-side four-layer substrate model. |
| [`shell/docs/registry-system.md`](../../shell/docs/registry-system.md) | Plugin / command / slot registries; ownership. |
| [`shell/docs/extension-host.md`](../../shell/docs/extension-host.md) | Load order, dependency resolution, lifecycle. |
| [`shell/docs/event-bus.md`](../../shell/docs/event-bus.md) | Event-bus naming conventions and core topic glossary. |
| [`shell/docs/slot-system.md`](../../shell/docs/slot-system.md) | UI slot ids, overlay pattern, pointer-events handling. |
| [`shell/docs/workspace-layout.md`](../../shell/docs/workspace-layout.md) | `workspace.json` schema. |
| [`shell/docs/context-keys.md`](../../shell/docs/context-keys.md) | Built-in context keys; when-clause grammar. |
| [`shell/docs/core-plugins.md`](../../shell/docs/core-plugins.md) | Catalog of every shipped core plugin. |

## Decisions (ADRs)

The full ADR set is at [`docs/adr/`](../adr/). The ones a plugin
developer most often touches:

| ADR | Topic |
|---|---|
| [0002](../adr/0002-hierarchical-capability-strings.md) | Capability vocabulary |
| [0011](../adr/0011-adopt-plugin-first-shell.md) | Why the shell is plugin-first (historical context) |
| [0015](../adr/0015-iframe-sandbox-plugin-runtime.md) | iframe-JS plugin runtime |
| [0016](../adr/0016-microkernel-native-vs-wasm-plugin-split.md) | Native vs. WASM tier choice |
| [0020](../adr/0020-popout-window-architecture.md) | Pop-out window architecture |

Full ADR index: [`docs/adr/README.md`](../adr/README.md).

## Templates

| Path | What you get |
|---|---|
| `docs/templates/community-plugin/` | Community-plugin scaffold (manifest, entry point, build script, tests). |
| `docs/templates/core-plugin/` | Core-plugin scaffold (`CorePlugin` impl, Cargo.toml, IPC test). |

The `nexus plugin scaffold` command copies these for you — see
[Getting started](getting-started.md).

## Shell plugin reference (deeper)

| Source | What it covers |
|---|---|
| [`docs/shell/writing-a-plugin.md`](../shell/writing-a-plugin.md) | In-depth shell plugin reference (manifest, sandbox, capabilities, slot system, worked example). |
| [`docs/shell/plugin-system.md`](../shell/plugin-system.md) | `Plugin` object shape, manifest anatomy, core vs. community details. |

## When something's missing

The Nexus codebase moves quickly. If a doc disagrees with the source,
**trust the source** and file an issue (or open a PR — docs are in
the repo).

For the absolute current state:

- `git log --oneline crates/nexus-plugin-api/` — recent API changes.
- `git log --oneline shell/docs/` — recent doc updates.
- [`docs/PRDs/IMPLEMENTATION_STATUS.md`](../PRDs/IMPLEMENTATION_STATUS.md)
  — what's shipped vs. in-progress.
