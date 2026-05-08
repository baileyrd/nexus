# Obsidian Plugin API vs Nexus Extension API

A side-by-side comparison for porting effort estimation. Snapshot date: 2026-05-04.

## TL;DR

| Concern | Obsidian | Nexus |
|---|---|---|
| Authority model | Untrusted JS, full app access | Capability-gated; community plugins WASM-sandboxed |
| Lifecycle | `class extends Plugin { onload(); onunload() }` | Plain manifest object + `activate(api)` function |
| Cross-plugin call | Direct method (`app.vault.read(file)`) | IPC: `api.kernel.invoke(pluginId, command, args)` |
| File model | `TFile` / `TFolder` objects, virtual vault tree | Forge-relative `string` paths; "file-as-truth" on disk |
| UI | Imperative DOM helpers (`addRibbonIcon`, `addStatusBarItem`) | Declarative manifest contributions + slot-based `views.register` |
| Settings | `class MyTab extends PluginSettingTab` | JSON Schema in `contributes.configuration`, auto-rendered |
| Editor extension | Direct CodeMirror 6 + Markdown post-processors | Smaller surface (`registerFencedCodeRenderer`, `registerSnippet`); raw CM hooks not exposed |
| Events | `app.workspace.on('file-open', cb)`, ad-hoc names | Closed event enum + `Custom` variant; `api.events.on/emit` |
| Network / FS | `requestUrl()`, `app.vault.adapter.read()` | `api.platform.*`, `api.fs.*` — capability-gated |
| Plugin tiers | One (community JS, full access) | Two: Core (native Rust) + Community (WASM) |

Porting a trivial Obsidian plugin (ribbon button → run a command) is ~30 minutes of remapping. Anything that touches the editor, vault graph, metadata cache, or DOM directly is a real port — not a recompile.

---

## 1. Plugin shape

### Obsidian

```ts
import { Plugin } from "obsidian";

export default class HelloPlugin extends Plugin {
  async onload() {
    this.addRibbonIcon("dice", "Greet", () => {
      new Notice("Hello from a community plugin!");
    });

    this.addCommand({
      id: "say-hello",
      name: "Say hello",
      callback: () => new Notice("Hello"),
    });
  }

  onunload() {}
}
```

`manifest.json` carries metadata; everything else lives in JS that can touch the DOM, the workspace, the vault, and the network freely.

### Nexus

```ts
import type { Plugin, PluginAPI } from '@nexus/extension-api'

export const helloPlugin: Plugin = {
  manifest: {
    id: 'community.hello',
    name: 'Hello',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    apiVersion: 1,
    contributes: {
      commands: [
        { id: 'community.hello.say', title: 'Say hello', category: 'Hello' },
      ],
      keybindings: [
        { command: 'community.hello.say', key: 'ctrl+alt+h' },
      ],
    },
  },
  activate(api: PluginAPI) {
    api.commands.register('community.hello.say', () => {
      api.notifications.show({ type: 'info', message: 'Hello' })
    })
    api.activityBar.addItem({
      id: 'community.hello.activity',
      iconName: 'sparkle',
      title: 'Hello',
      viewId: 'community.hello.view',
      priority: 50,
      command: 'community.hello.say',
    })
  },
}
```

No class. No DOM. Contributions are declared in the manifest so the host can wire them up before activation; runtime registrations go through `api.*` methods that auto-clean on plugin unload.

## 2. Reading and writing files

### Obsidian

```ts
const file = this.app.workspace.getActiveFile();
if (file) {
  const text = await this.app.vault.read(file);
  await this.app.vault.modify(file, text + "\nappended");
}
```

Works on a `TFile` object; `app.vault.*` is the canonical interface.

### Nexus

```ts
const active = api.editor.active()
if (active) {
  // Read via the storage plugin's IPC handler — no globals.
  const bytes = await api.kernel.invoke<{ bytes: number[] }>(
    'com.nexus.storage', 'read_file', { path: active.relpath },
  )
  const text = new TextDecoder().decode(new Uint8Array(bytes.bytes))
  await api.kernel.invoke('com.nexus.storage', 'write_file', {
    path: active.relpath,
    bytes: Array.from(new TextEncoder().encode(text + '\nappended')),
  })
}
```

Every cross-plugin op routes through `kernel.invoke(pluginId, command, args)`. `api.fs` is also available for forge-internal paths; for OS-level FS use `api.platform.fs.*`.

## 3. Settings tab

### Obsidian

```ts
import { PluginSettingTab, Setting } from "obsidian";

class HelloSettingTab extends PluginSettingTab {
  display() {
    const { containerEl } = this;
    containerEl.empty();
    new Setting(containerEl)
      .setName("Greeting")
      .addText(t => t.setValue(this.plugin.settings.greeting)
        .onChange(async v => {
          this.plugin.settings.greeting = v;
          await this.plugin.saveSettings();
        }));
  }
}

// in onload:
this.addSettingTab(new HelloSettingTab(this.app, this));
```

### Nexus

```ts
manifest: {
  contributes: {
    configuration: {
      pluginId: 'community.hello',
      title: 'Hello',
      order: 50,
      schema: [
        {
          key: 'community.hello.greeting',
          title: 'Greeting',
          description: 'What to say when the command fires.',
          type: 'string',
          default: 'Hello',
        },
      ],
    },
  },
},
// in activate:
api.configuration.register(helloPlugin.manifest.contributes!.configuration!)
const greeting = api.configuration.get<string>('community.hello.greeting')
```

The settings panel auto-generates UI from the schema; the plugin reads values via `api.configuration.get`. No imperative DOM construction.

## 4. Listening for editor / file events

### Obsidian

```ts
this.registerEvent(this.app.workspace.on("file-open", (file) => {
  if (file) console.log("opened", file.path);
}));
this.registerEvent(this.app.metadataCache.on("changed", (file, data, cache) => {
  // ...
}));
```

### Nexus

```ts
const off = api.editor.onChange((active) => {
  if (active) console.log('active editor', active.relpath, active.revision)
})
// `off` is auto-swept on plugin unload.

// For arbitrary topic streams (file_created, etc.), subscribe through the kernel:
const offTopic = await api.kernel.on('com.nexus.storage.file_created', (topic, payload) => {
  console.log(topic, payload)
})
```

Editor changes go through the typed `editor.onChange` API. Storage events are delivered as a kernel topic stream (`api.kernel.on(prefix, handler)`); the prefix dispatch covers `file_created`, `file_modified`, `file_deleted`, `file_renamed`. Nexus's event enum is closed with a `Custom` variant for plugin-defined topics — no ad-hoc string events on a global event bus.

## 5. Status-bar item

### Obsidian

```ts
const item = this.addStatusBarItem();
item.setText("Words: 0");
item.addEventListener("click", () => new Notice("clicked"));
```

### Nexus

```ts
api.statusBar.add({
  id: 'community.hello.words',
  slot: 'right',
  priority: 10,
  render: (host) => {
    // host is a stable React-friendly mount handle — DOM details are
    // not exposed directly to community plugins.
    host.text('Words: 0')
    host.onClick(() => api.notifications.show({ type: 'info', message: 'clicked' }))
  },
})
```

Or the more common pattern: register a React component view targeting the `statusBarRight` slot via `api.views.register`. The component still doesn't get raw `document` access in the WASM tier; it gets a React tree the host renders into the slot.

## 6. Capabilities & sandbox

Obsidian shows a one-time "trust this plugin" warning, then it's all-trust JS. Nexus is opposite:

```jsonc
// community plugin manifest fragment
{
  "id": "community.hello",
  "capabilities": [
    "fs.read",
    "ipc.call:com.nexus.storage:read_file",
    "events.publish:custom:community.hello.*"
  ]
}
```

The shell prompts the user on first activation listing each cap; if a plugin tries an op without the capability, the kernel returns `CapabilityError` and the call rejects. This is the single biggest mental shift from Obsidian — anything you call has to be enumerated up front.

## 7. Two-tier runtime

| | Core (Nexus) | Community (Nexus) | Community (Obsidian) |
|---|---|---|---|
| Language | Rust | Rust→WASM (or TS bundle in shell) | TypeScript |
| Sandbox | None | wasmtime + caps | None |
| Bundling | Crate in workspace | `manifest.toml` + WASM/JS bundle | `manifest.json` + `main.js` |
| Distribution | Built into the binary | Drop into `<forge>/.forge/plugins/` | Community Plugins directory |
| Direct kernel access | Yes (`InternalAPI`) | No, IPC only | N/A |

Obsidian has one tier. Nexus's first-party features (storage, AI, editor, terminal, git, …) are core plugins; everything user-installable is community.

## 8. Things that don't exist on the Nexus side yet

- A `MetadataCache` analogue. Nexus has SQLite + Tantivy indexes but no plugin-facing API to query "give me the YAML / headings / links of this file" — plugins read raw and parse, or invoke storage IPC handlers.
- Markdown post-processors. Obsidian's `registerMarkdownPostProcessor` lets a plugin transform the rendered DOM after the markdown engine has run. Nexus has `registerFencedCodeRenderer` (one fenced-language → React/HTML) and that's it for now.
- Editor extension hooks. Plugins can't install raw CodeMirror extensions today. The closest is contributions through manifest-declared snippets / fenced renderers.
- Mobile. Obsidian has a parallel mobile-only API surface; Nexus is desktop-only.

## 9. Porting heuristic

When you look at an Obsidian plugin you want to bring across, scan for:

1. **Plain commands + ribbon items, no editor/vault depth** → trivial port, ~30 min.
2. **Reads frontmatter / parses links** → moderate; needs an IPC round trip per file or a host-side helper.
3. **Installs CodeMirror extensions / decorations** → today, blocked. Either lobby for the extension surface or fold into a fenced-code renderer if applicable.
4. **DOM-mutates the rendered preview** → blocked in community tier; possible only as a core plugin.
5. **Talks to remote APIs** → fine, but needs `network.*` capability declared.

## 10. References

- Obsidian API: <https://docs.obsidian.md/Reference/TypeScript+API>
- Nexus extension API package: `packages/nexus-extension-api/`
- Nexus `PluginAPI` aggregate: `shell/src/types/plugin.ts`
- Capabilities (ADR 0002): `docs/adr/0002-hierarchical-capability-strings.md`
- Single-dispatch IPC contract (ADR 0005): `docs/adr/0005-single-dispatch-handler-ids.md`
- Microkernel core/WASM split (ADR 0016): `docs/adr/0016-microkernel-native-vs-wasm-plugin-split.md`
