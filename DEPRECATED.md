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

_(none — this file is seeded alongside the DEPRECATED policy. Entries will
be added here as the API evolves.)_

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
