# Manifest

Every plugin has a `plugin.json` at its root. The kernel reads it
before loading the plugin's code; everything from capability gates to
the settings UI keys off these fields.

## Minimal manifest

```json
{
  "id": "com.example.hello",
  "name": "Hello",
  "version": "1.0.0",
  "main": "index.js",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": []
}
```

That's enough to load. Add fields as you need them.

## All fields

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Reverse-DNS unique identifier (`com.example.foo`). Stable across versions. |
| `name` | string | yes | Human-readable display name. Shown in the plugins panel and command palette. |
| `version` | string | yes | Semver. Used by the loader to detect upgrades. |
| `main` | string | yes | Path to the entry point relative to the plugin root. `index.js` for iframe-JS, `plugin.wasm` for WASM. |
| `apiVersion` | integer | yes | The `@nexus/extension-api` major version this plugin targets. Currently `1`. |
| `sandboxed` | boolean | yes | `true` for community plugins (always). `false` is reserved for core plugins; the loader rejects `false` for installed plugins. |
| `capabilities` | string[] | yes (may be empty) | Capability strings the plugin requests. See [Capabilities](capabilities.md). |
| `description` | string | no | Short description shown in the plugins panel. |
| `author` | string | no | Author name or org. |
| `homepage` | string | no | URL shown in the plugins panel. |
| `enabled` | boolean | no | Default-enabled state on first install. Defaults to `true`. |
| `activation` | string[] | no | Activation events. Defaults to `["onStartup"]`. See [Lifecycle](lifecycle.md). |
| `contributes` | object | no | Static contributions (commands, views, settings schemas). See below. |
| `dependencies` | object | no | Other plugins required to be present and enabled. `{ "com.x.foo": "^1.2.0" }`. |
| `apiCompat` | object | no | Reserved for forward-compatibility hints (e.g. `{ "minNexus": "0.5.0" }`). |

## `contributes`

Static declarations the kernel reads without loading your code. Useful
because they show up in the palette / settings UI before activation
fires.

```json
"contributes": {
  "commands": [
    { "id": "hello.sayHi", "title": "Hello: Say Hi", "keybinding": "Ctrl+Shift+H" }
  ],
  "views": [
    { "id": "hello.panel", "title": "Hello", "icon": "smile", "slot": "sidebar" }
  ],
  "settings": {
    "schema": { /* JSON Schema; see plugins/settings.md */ }
  },
  "events": {
    "subscribes": ["files:changed"],
    "publishes":  ["hello:greeted"]
  },
  "menus": [
    { "command": "hello.sayHi", "menu": "editor.context" }
  ]
}
```

A command listed under `contributes.commands` is shown in the palette
even if your plugin hasn't activated yet — selecting it triggers
activation, then runs the command.

## Example: a complete manifest

```json
{
  "id": "com.example.hello",
  "name": "Hello",
  "version": "1.0.0",
  "main": "index.js",
  "apiVersion": 1,
  "sandboxed": true,
  "description": "Demonstrates the manifest fields.",
  "author": "you",
  "homepage": "https://example.com/hello",
  "enabled": true,
  "activation": ["onCommand:hello.sayHi", "onView:hello.panel"],
  "capabilities": ["ui.notify", "kv.read", "kv.write"],
  "dependencies": {
    "com.nexus.storage": "*"
  },
  "contributes": {
    "commands": [
      { "id": "hello.sayHi", "title": "Hello: Say Hi", "keybinding": "Ctrl+Shift+H" }
    ],
    "views": [
      { "id": "hello.panel", "title": "Hello", "icon": "smile", "slot": "sidebar" }
    ]
  }
}
```

## Validation

The loader validates the manifest before activation:

- Required fields present.
- `id` matches `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)*$`.
- `apiVersion` is supported by this Nexus build.
- Every `capabilities` entry is a known capability string. Unknown
  capability strings are rejected (use the canonical strings from
  [Capabilities](capabilities.md)).
- Every `dependencies` entry refers to a present plugin (warning, not
  error — installs may run before deps).

A failing manifest is logged and the plugin is **not loaded**. The
plugin appears in the panel marked **Errored** with the validation
message.

## Where the manifest lives

After `nexus plugin install`, the manifest is copied to:

```
<forge>/.forge/plugins/<id>/plugin.json
```

Alongside the entry point (`index.js` or `plugin.wasm`) and any
runtime state (`settings.json`, plugin-owned KV in `kv.sqlite3`).

## Versioning

The `version` field is informational; the loader doesn't enforce
semver semantics. A reinstall replaces the previous version with the
new one. Migration of plugin-owned data is the plugin's
responsibility — typically done in `activate()` by checking a
schema-version key in KV.

## See also

- [Lifecycle](lifecycle.md) — what happens after the manifest is read.
- [Capabilities](capabilities.md) — the canonical capability list.
- [Settings](settings.md) — schemas under `contributes.settings`.
- [`../../adr/0002-hierarchical-capability-strings.md`](../../adr/0002-hierarchical-capability-strings.md)
  for the capability-string design rationale.
