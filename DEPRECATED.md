# Deprecated — Nexus Extension API

Tracks deprecations in the host-exposed plugin API (`NexusPluginContext`,
contribution DTOs, manifest fields). Entries are grouped by the release in
which the deprecation was announced and the release in which the symbol will
be removed. When the `@nexus/extension-api` TypeScript package (UI F-2.1.1)
ships, each entry here gets a matching `@deprecated` JSDoc tag so IDEs surface
the warning at author time.

## Policy

- **Deprecation window**: one minor release minimum between announcement and
  removal (e.g. announced in `0.5`, removable in `0.6`).
- **Runtime warning**: the host emits a single `console.warn` per plugin per
  deprecated API it calls, tagged with the plugin id. Repeated calls are
  suppressed for the lifetime of the loaded plugin.
- **Author-time warning**: when `@nexus/extension-api` lands, `@deprecated`
  JSDoc tags do the heavy lifting — the runtime warning is a fallback for
  plugins shipping from JS without the typed import path.
- **Migration guide**: every entry names the replacement API so plugin
  authors have a 1:1 mapping to act on.

## Currently deprecated

### IPC payloads with extra fields are now rejected

**Announced:** 2026-05-01 (audit P0-1, PR following #108).
**Effective immediately** — no deprecation window because every payload
that previously sent unknown fields was already a latent bug; the
serializer just silently dropped them.

**What changed.** Every IPC arg/reply struct in the workspace now
carries `#[serde(deny_unknown_fields)]`. A plugin or shell caller that
sends `{ "file_path": "x", "file_pathh": "typo" }` to
`com.nexus.comments::list` (and analogous typos against any other
typed handler) now gets `IpcError::PluginCrashedDuringCall` instead of
silently round-tripping with `file_path` defaulted to `""`.

**Migration.** Inspect any IPC payload your plugin builds. If it
contains fields not listed in the corresponding `*Args` / `*Reply`
struct under `crates/nexus-<service>/src/{ipc,core_plugin}.rs`, remove
them. Run `scripts/check_ipc_drift.sh` and consult the regenerated
JSON schemas under `crates/nexus-bootstrap/schemas/ipc/` for the
authoritative shape (`additionalProperties: false` is now asserted by
`crates/nexus-bootstrap/tests/ipc_schema_emit.rs`).

**Out of scope of this rollout** (handlers that bypass typed structs,
deserialize fields ad-hoc, and therefore cannot enforce strict
shapes): `com.nexus.storage::*` (uses `path_arg` helper on raw
`serde_json::Value`), `com.nexus.git::*`, `com.nexus.mcp::*`. These
are tracked by a follow-up to refactor them to
`parse_args::<TypedStruct>(...)`.

## Trust policy — Script (JS) plugins

Script plugins execute in the Tauri WebView as ES modules loaded via a Blob
URL `import()`. Today they bypass the WASM capability sandbox entirely and
have access to whatever the Tauri allowlist exposes.

Until **UI F-8.1.1** (iframe-sandbox for JS plugin execution) and
**UI F-2.2.1** (capability-gated `NexusPluginContext`) both land, script
plugins are **first-party / core only**:

- Community `[script]` plugins remain loadable in development builds for
  dogfooding, but must not be shipped through a public marketplace.
- Nexus-authored plugins (`plugins/hello-js`, future core-owned script
  extensions) are the only script plugins approved for general release.

The sandbox + capability work reopens the door for community script
plugins under the same capability model WASM plugins already honour.

## Historical — removed

### `EditorKeybinding.when` — removed in pre-1.0

Reserved-but-never-parsed field on `contributions.registerEditorKeybinding`
contributions. Plugins setting `when: "editorTextFocus"` or similar were
misled into believing the runtime scoped the binding; in fact editor
keybindings were always active while the CodeMirror surface had focus.

Removed without a deprecation cycle because (a) no consumer parsed it, (b)
the API had not reached 1.0, and (c) keeping it in the TypeScript shape
actively encouraged bugs. Plugins that need conditional activation should
register the binding and have the dispatched command branch on state, or
wait for the future when-clause evaluator (tracked as UI F-4.1.2 follow-up).
